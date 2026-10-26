//! 自检用的本地 HTTP 测试服务器。
//!
//! 只监听 `127.0.0.1:0`（随机端口），用来在 `--selftest` 里真实验证：
//! 分块慢速传输、取消、断点续传（Range / If-Range）、503 重试、
//! 重定向被白名单拦住、哈希不匹配。
//!
//! 生产代码不会启动它。

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

pub struct TestServer {
    pub port: u16,
    /// 已经收到多少次 `/flaky` 请求
    #[allow(dead_code)]
    pub flaky_hits: Arc<AtomicU32>,
    /// 每个 `/data` 响应实际写出的字节数（用于判断取消是否真的截断）
    pub served: Arc<AtomicU64>,
    shutdown: Arc<AtomicU32>,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.shutdown.store(1, Ordering::SeqCst);
        // 连一下把自己踢出 accept
        let _ = std::net::TcpStream::connect(("127.0.0.1", self.port));
    }
}

fn query_int(query: &str, key: &str, default: u64) -> u64 {
    for part in query.split('&') {
        if let Some((k, v)) = part.split_once('=') {
            if k == key {
                if let Ok(n) = v.parse::<u64>() {
                    return n;
                }
            }
        }
    }
    default
}

fn header_value<'a>(headers: &'a [String], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|h| h.to_ascii_lowercase().starts_with(name))
        .and_then(|h| h.split_once(':'))
        .map(|(_, v)| v.trim())
}

/// 生成确定性内容：第 i 字节 = (i * 31 + 7) % 251
fn byte_at(i: u64) -> u8 {
    ((i.wrapping_mul(31).wrapping_add(7)) % 251) as u8
}

/// 一段内容的 SHA1，供测试断言用
pub fn expected_sha1(size: u64) -> String {
    use sha1::{Digest, Sha1};
    let mut h = Sha1::new();
    let mut buf = Vec::with_capacity(64 * 1024);
    for i in 0..size {
        buf.push(byte_at(i));
        if buf.len() == 64 * 1024 {
            h.update(&buf);
            buf.clear();
        }
    }
    h.update(&buf);
    let d = h.finalize();
    let mut s = String::new();
    for b in d {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

impl TestServer {
    pub fn start() -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let flaky_hits = Arc::new(AtomicU32::new(0));
        let served = Arc::new(AtomicU64::new(0));
        let shutdown = Arc::new(AtomicU32::new(0));

        let fh = flaky_hits.clone();
        let sv = served.clone();
        let sd = shutdown.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if sd.load(Ordering::SeqCst) == 1 {
                    break;
                }
                let Ok(stream) = stream else { continue };
                let fh = fh.clone();
                let sv = sv.clone();
                std::thread::spawn(move || {
                    let _ = handle(stream, port, &fh, &sv);
                });
            }
        });

        Ok(Self {
            port,
            flaky_hits,
            served,
            shutdown,
        })
    }

    pub fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn data_url(&self, size: u64, chunk: u64, delay_ms: u64) -> String {
        format!(
            "{}/data?size={size}&chunk={chunk}&delay={delay_ms}",
            self.base()
        )
    }

    /// 返回合法 JSON 的地址（元数据缓存测试用）
    pub fn json_url(&self, size: u32) -> String {
        format!("{}/json?size={size}", self.base())
    }

    pub fn flaky_url(&self, fails: u32, size: u64) -> String {
        format!("{}/flaky?fails={fails}&size={size}", self.base())
    }

    pub fn redirect_url(&self, to: &str) -> String {
        format!(
            "{}/redirect?to={}",
            self.base(),
            to.replace(':', "%3A").replace('/', "%2F")
        )
    }
}

fn url_decode(s: &str) -> String {
    let mut out = String::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v as char);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn handle(
    mut stream: TcpStream,
    port: u16,
    flaky_hits: &AtomicU32,
    served: &AtomicU64,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut first = String::new();
    if reader.read_line(&mut first)? == 0 {
        return Ok(());
    }
    let mut headers: Vec<String> = Vec::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let t = line.trim_end().to_string();
        if t.is_empty() {
            break;
        }
        headers.push(t);
    }

    let parts: Vec<&str> = first.split_whitespace().collect();
    if parts.len() < 2 {
        return Ok(());
    }
    let target = parts[1];
    let (path, query) = target.split_once('?').unwrap_or((target, ""));

    // 重定向到白名单外，用来验证每一跳都会被校验
    if path == "/redirect" {
        let to = query
            .split('&')
            .find_map(|p| p.strip_prefix("to="))
            .map(url_decode)
            .unwrap_or_default();
        let resp = format!(
            "HTTP/1.1 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(resp.as_bytes())?;
        return Ok(());
    }

    if path == "/flaky" {
        let fails = query_int(query, "fails", 0) as u32;
        let n = flaky_hits.fetch_add(1, Ordering::SeqCst);
        if n < fails {
            let resp = "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            stream.write_all(resp.as_bytes())?;
            return Ok(());
        }
        let size = query_int(query, "size", 1024);
        return send_body(&mut stream, &headers, size, size.max(1), 0, port, served);
    }

    if path == "/json" {
        let size = query_int(query, "size", 64) as u32;
        let body = format!("{{\"ok\":true,\"size\":{size}}}");
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
             Connection: close\r\n\r\n{body}",
            body.len()
        );
        served.fetch_add(body.len() as u64, Ordering::SeqCst);
        stream.write_all(resp.as_bytes())?;
        return Ok(());
    }

    if path == "/data" {
        let size = query_int(query, "size", 1024);
        let chunk = query_int(query, "chunk", 64 * 1024).max(1);
        let delay = query_int(query, "delay", 0);
        return send_body(&mut stream, &headers, size, chunk, delay, port, served);
    }

    let resp = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    stream.write_all(resp.as_bytes())?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn send_body(
    stream: &mut TcpStream,
    headers: &[String],
    size: u64,
    chunk: u64,
    delay_ms: u64,
    port: u16,
    served: &AtomicU64,
) -> std::io::Result<()> {
    let etag = format!("\"v1-{size}\"");
    let range = header_value(headers, "range");
    let if_range = header_value(headers, "if-range");

    let mut start = 0u64;
    let mut partial = false;
    if let Some(r) = range {
        if if_range
            .map(|v| v.trim_matches('"') == etag.trim_matches('"'))
            .unwrap_or(false)
        {
            if let Some(rest) = r.trim().strip_prefix("bytes=") {
                if let Some((a, _)) = rest.split_once('-') {
                    if let Ok(n) = a.trim().parse::<u64>() {
                        start = n.min(size);
                        partial = true;
                    }
                }
            }
        }
    }

    let remaining = size.saturating_sub(start);
    let head = if partial {
        format!(
            "HTTP/1.1 206 Partial Content\r\nContent-Type: application/octet-stream\r\n\
             Content-Length: {remaining}\r\nContent-Range: bytes {start}-{}/{size}\r\n\
             ETag: {etag}\r\nLast-Modified: Wed, 01 Jan 2025 00:00:00 GMT\r\n\
             Accept-Ranges: bytes\r\nConnection: close\r\n\r\n",
            size.saturating_sub(1)
        )
    } else {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\n\
             Content-Length: {size}\r\nETag: {etag}\r\n\
             Last-Modified: Wed, 01 Jan 2025 00:00:00 GMT\r\n\
             Accept-Ranges: bytes\r\nConnection: close\r\n\r\n"
        )
    };
    let _ = port;
    stream.write_all(head.as_bytes())?;
    stream.flush()?;

    let mut pos = start;
    while pos < size {
        let end = (pos + chunk).min(size);
        let buf: Vec<u8> = (pos..end).map(byte_at).collect();
        // 客户端取消会直接断开连接，写失败是预期行为
        if stream.write_all(&buf).is_err() {
            return Ok(());
        }
        if stream.flush().is_err() {
            return Ok(());
        }
        served.fetch_add(buf.len() as u64, Ordering::SeqCst);
        pos = end;
        if delay_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        }
    }
    Ok(())
}
