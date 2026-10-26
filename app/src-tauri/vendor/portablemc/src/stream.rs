//! [Astral patch] 可取消 / 可续传的单文件流式下载。
//!
//! 上游 `download::Single` 只能整文件下完，取消只有在文件边界才生效，
//! 大文件（客户端 jar、资源包、JDK 压缩包）无法及时中断。这里补一层：
//!
//! - **边下边检查取消**：每读到一个 chunk 就看一次令牌，命中立刻收尾；
//! - **断点续传**：`Range: bytes=N-` + `If-Range`（ETag / Last-Modified），
//!   服务器换了内容时 `If-Range` 会让它回 200，我们据此从头重下，
//!   不会把两个版本拼在一起；
//! - **重定向逐跳校验**：由调用方给出校验函数，每一跳都要过白名单；
//! - **超时**：连接 15s、读 60s，避免半死不活的连接把任务挂住。
//!
//! 这一层只负责「把一个 URL 写进一个文件」，校验与原子替换由上层负责。

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use reqwest::header;
use reqwest::redirect;
use tokio::io::AsyncWriteExt;

/// 取消令牌。实现方保证 `cancelled()` 是幂等且无阻塞的。
pub trait Cancel: Send + Sync {
    fn cancelled(&self) -> bool;
}

/// 永不取消
pub struct NeverCancel;

impl Cancel for NeverCancel {
    fn cancelled(&self) -> bool {
        false
    }
}

/// 断点信息：上次已经写到哪、当时的 ETag / Last-Modified 是什么。
#[derive(Clone, Debug, Default)]
pub struct Resume {
    pub offset: u64,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub total: Option<u64>,
}

impl Resume {
    /// 有可用的续传起点，且能证明内容没变（有 ETag 或 Last-Modified）
    pub fn usable(&self) -> bool {
        self.offset > 0 && (self.etag.is_some() || self.last_modified.is_some())
    }
}

#[derive(Clone, Debug)]
pub struct Outcome {
    /// 本次真正从网络写下去的字节数
    pub bytes: u64,
    /// 文件最终大小（续传时 = 断点 + 本次字节）
    pub total_bytes: u64,
    pub resumed: bool,
    pub status: u16,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_length: Option<u64>,
}

#[derive(Debug)]
pub enum Error {
    /// 用户取消：文件停在已写入的位置，调用方可以据此保留断点
    Cancelled {
        total_bytes: u64,
        etag: Option<String>,
        last_modified: Option<String>,
        content_length: Option<u64>,
    },
    /// 429 / 5xx 等可以重试的状态码
    Status(u16),
    Network(String),
    Io(String),
    /// 续传不可信（起点对不上、服务器换了文件）
    BadRange(String),
}

impl Error {
    pub fn code(&self) -> &'static str {
        match self {
            Error::Cancelled { .. } => "CANCELLED",
            Error::Status(_) | Error::Network(_) => "NETWORK",
            Error::Io(_) => "INTERNAL",
            Error::BadRange(_) => "NETWORK",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Error::Cancelled { .. } => "任务已取消".to_string(),
            Error::Status(s) => format!("服务器返回 {s}"),
            Error::Network(e) => format!("网络错误：{e}"),
            Error::Io(e) => format!("写入失败：{e}"),
            Error::BadRange(e) => format!("断点续传不可用：{e}"),
        }
    }

    /// 这个错误是否值得重试（取消永远不重试）
    pub fn retryable(&self) -> bool {
        match self {
            Error::Cancelled { .. } => false,
            Error::Status(s) => *s == 429 || *s >= 500,
            Error::Network(_) => true,
            Error::Io(_) => false,
            Error::BadRange(_) => false,
        }
    }
}

fn io_err(e: std::io::Error) -> Error {
    if e.kind() == std::io::ErrorKind::StorageFull {
        Error::Io(format!("磁盘空间不足：{e}"))
    } else {
        Error::Io(e.to_string())
    }
}

/// `bytes 123-456/789` → Some(123)
fn range_start(v: &str) -> Option<u64> {
    let rest = v.trim().strip_prefix("bytes")?.trim_start();
    let (a, _) = rest.split_once('-')?;
    a.trim().parse().ok()
}

/// `bytes 123-456/789` → Some(789)
fn range_total(v: &str) -> Option<u64> {
    let (_, b) = v.split_once('/')?;
    let b = b.trim();
    if b == "*" { None } else { b.parse().ok() }
}

/// 下载 `url` 到 `file`。
///
/// `resume` 给出断点时按 Range 续传；`validate` 对初始地址和每一跳重定向都要过。
pub fn fetch(
    url: &str,
    file: &Path,
    resume: Option<&Resume>,
    cancel: &dyn Cancel,
    validate: Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>,
    on_progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<Outcome, Error> {
    fetch_with(url, file, resume, cancel, validate, false, on_progress)
}

/// 同 [`fetch`]，`allow_http` 只给自检连本机测试服务器用。
#[allow(clippy::too_many_arguments)]
pub fn fetch_with(
    url: &str,
    file: &Path,
    resume: Option<&Resume>,
    cancel: &dyn Cancel,
    validate: Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>,
    allow_http: bool,
    on_progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<Outcome, Error> {
    if cancel.cancelled() {
        return Err(Error::Cancelled {
            total_bytes: 0,
            etag: None,
            last_modified: None,
            content_length: None,
        });
    }

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| Error::Io(e.to_string()))?;

    rt.block_on(fetch_async(
        url,
        file,
        resume,
        cancel,
        validate,
        allow_http,
        on_progress,
    ))
}

async fn fetch_async(
    url: &str,
    file: &Path,
    resume: Option<&Resume>,
    cancel: &dyn Cancel,
    validate: Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>,
    allow_http: bool,
    on_progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<Outcome, Error> {
    let validator = Arc::clone(&validate);
    let client = crate::http::builder()
        .https_only(!allow_http)
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .pool_max_idle_per_host(4)
        .redirect(redirect::Policy::custom(move |attempt| {
            // 超过 5 跳就当异常
            if attempt.previous().len() >= 5 {
                return attempt.stop();
            }
            match validator(attempt.url().as_str()) {
                Ok(()) => attempt.follow(),
                Err(e) => attempt.error(std::io::Error::other(e)),
            }
        }))
        .build()
        .map_err(|e| Error::Network(e.to_string()))?;

    let offset = resume.map(|r| r.offset).unwrap_or(0);
    let want_resume = resume.map(|r| r.usable()).unwrap_or(false) && offset > 0;

    let mut req = client.get(url);
    if want_resume {
        req = req.header(header::RANGE, format!("bytes={offset}-"));
        // 有 ETag 用 ETag，没有就退到 Last-Modified；两者都没有就不续传
        if let Some(tag) = resume.and_then(|r| r.etag.clone()) {
            req = req.header(header::IF_RANGE, tag);
        } else if let Some(lm) = resume.and_then(|r| r.last_modified.clone()) {
            req = req.header(header::IF_RANGE, lm);
        }
    }

    let mut res = req
        .send()
        .await
        .map_err(|e| Error::Network(e.to_string()))?;

    let status = res.status().as_u16();
    let etag = res
        .headers()
        .get(header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let last_modified = res
        .headers()
        .get(header::LAST_MODIFIED)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let (append, total) = match status {
        206 => {
            let cr = res
                .headers()
                .get(header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            let start = range_start(cr)
                .ok_or_else(|| Error::BadRange(format!("缺少 Content-Range：{cr}")))?;
            if start != offset {
                return Err(Error::BadRange(format!(
                    "服务器从 {start} 开始，本地断点是 {offset}"
                )));
            }
            let total = range_total(cr).or_else(|| res.content_length().map(|l| offset + l));
            (true, total)
        }
        200 => {
            // 服务器忽略 Range 或 If-Range 判定内容变了：从头来，避免拼接两个版本
            (false, res.content_length())
        }
        416 => {
            return Err(Error::BadRange(format!(
                "服务器认为断点 {offset} 越界（文件可能已变短）"
            )));
        }
        s => return Err(Error::Status(s)),
    };

    if append {
        match tokio::fs::metadata(file).await {
            Ok(m) if m.len() == offset => {}
            _ => {
                return Err(Error::BadRange(
                    "本地临时文件长度与断点不一致，不能续传".into(),
                ));
            }
        }
    }

    let mut f = if append {
        tokio::fs::OpenOptions::new()
            .append(true)
            .open(file)
            .await
            .map_err(io_err)?
    } else {
        tokio::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(file)
            .await
            .map_err(io_err)?
    };

    let mut written = if append { offset } else { 0 };
    let mut reported = written;
    let mut delta: u64 = 0;

    on_progress(written, total);

    loop {
        if cancel.cancelled() {
            let _ = f.flush().await;
            let _ = f.sync_all().await;
            return Err(Error::Cancelled {
                total_bytes: written,
                etag,
                last_modified,
                content_length: total,
            });
        }

        let chunk = match res.chunk().await {
            Ok(Some(c)) => c,
            Ok(None) => break,
            Err(e) => return Err(Error::Network(e.to_string())),
        };
        if chunk.is_empty() {
            continue;
        }

        f.write_all(&chunk).await.map_err(io_err)?;
        written += chunk.len() as u64;
        delta += chunk.len() as u64;

        // 每 256 KiB 报一次，避免高频回调把界面刷爆
        if written - reported >= 256 * 1024 {
            on_progress(written, total);
            reported = written;
        }
    }

    f.flush().await.map_err(io_err)?;
    f.sync_all().await.map_err(io_err)?;
    drop(f);

    on_progress(written, total);

    // 长度对不上说明连接被中途掐断（chunk 流正常结束但少数据）
    if let Some(t) = total {
        if written != t {
            return Err(Error::Network(format!(
                "连接提前结束：收到 {written} 字节，应有 {t} 字节"
            )));
        }
    }

    Ok(Outcome {
        bytes: delta,
        total_bytes: written,
        resumed: append,
        status,
        etag,
        last_modified,
        content_length: total,
    })
}
