//! 实时日志跟随（增量读）。
//!
//! `logs::read` 是一次性读整个文件（或尾部 N 行），用来事后看崩溃还行，
//! 但**启动过程中**用它就是不停地重读整个文件：文件越大越卡，而且
//! 每次都从头解析，前端还得自己 diff。
//!
//! 这里提供的是"给我从 offset 开始的新内容"：前端记住 `nextOffset`，
//! 下次带上它再来要，每次只传新增的那一段。PCL/HMCL 的启动日志页
//! 都是这个模型。
//!
//! 三条硬规则：
//! 1. **只返回完整行**。切到半个 UTF-8 字符或半行就返回，前端会拼出乱码；
//!    所以末尾没换行的那段不返回，offset 停在它的开头，下次补齐了再给。
//! 2. **文件被截断/轮转要能自愈**。offset 超过当前文件长度说明日志被重建了
//!    （游戏重启会重写 latest.log），这时回 `reset: true` 并从头读，前端清屏重来。
//! 3. **单次有上限**。日志暴涨时不能一次把几百 MB 塞进 IPC，超了就截断并
//!    告诉前端还有多少没读（`pendingBytes`），让它继续追。

use serde::Serialize;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// 单次返回的默认字节上限（前端一次轮询拿这么多就够刷屏了）
pub const DEFAULT_CHUNK: u64 = 64 * 1024;
/// 单次返回的硬上限，防止前端传个巨大的数把内存打满
pub const MAX_CHUNK: u64 = 256 * 1024;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LogChunk {
    pub id: String,
    pub name: String,
    /// 这次是从文件的第几个字节开始读的
    pub from_offset: u64,
    /// 下次调用要带的 offset
    pub next_offset: u64,
    /// 新增的完整行（已脱敏）
    pub text: String,
    /// 这次返回了多少行
    pub lines: u32,
    /// 是否读到了文件末尾
    pub eof: bool,
    /// 文件被截断/重建，前端应该清屏后从头显示
    pub reset: bool,
    /// 还剩多少字节没读（>0 说明日志涨得比轮询快，应该立刻再追一次）
    pub pending_bytes: u64,
    /// 这次是否因为上限而截断（等价于 pendingBytes > 0，留一个直白字段给前端）
    pub truncated: bool,
}

/// 读一次增量。`offset` 传上次返回的 `nextOffset`，第一次传 0。
pub fn tail(
    instance_dir: &Path,
    id: &str,
    offset: u64,
    max_bytes: Option<u64>,
) -> Result<LogChunk, String> {
    let (path, name) = super::logs::resolve_path(instance_dir, id)
        .ok_or_else(|| "INVALID_PATH|日志不存在".to_string())?;
    // 归档日志（`.gz`）是压缩流，没法按字节偏移追。要看得用 logs::read 整份读。
    if name.to_lowercase().ends_with(".gz") {
        return Err("INVALID_VALUE|这是归档日志，不能实时跟随，请直接打开查看".to_string());
    }
    let size = std::fs::metadata(&path)
        .map_err(|e| format!("INVALID_PATH|读不到日志信息|{e}"))?
        .len();

    // 文件比 offset 还短：被重建了（游戏重启重写 latest.log 是常态）
    let reset = offset > size;
    let mut from = if reset { 0 } else { offset };

    let cap = max_bytes.unwrap_or(DEFAULT_CHUNK).clamp(1, MAX_CHUNK);
    let pending = size.saturating_sub(from);

    if pending == 0 {
        return Ok(LogChunk {
            id: id.to_string(),
            name,
            from_offset: from,
            next_offset: from,
            text: String::new(),
            lines: 0,
            eof: true,
            reset,
            pending_bytes: 0,
            truncated: false,
        });
    }

    let mut f = std::fs::File::open(&path).map_err(|e| format!("PERMISSION|打不开日志|{e}"))?;
    f.seek(SeekFrom::Start(from))
        .map_err(|e| format!("INTERNAL|定位日志失败|{e}"))?;

    // 多读 1 字节也没意义，按上限读
    let want = pending.min(cap) as usize;
    let mut buf = vec![0u8; want];
    let mut got = 0usize;
    while got < want {
        match f.read(&mut buf[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(format!("INTERNAL|读日志失败|{e}")),
        }
    }
    buf.truncate(got);

    // 只保留到最后一个换行为止；剩下半行下次再给
    let cut = match buf.iter().rposition(|b| *b == b'\n') {
        Some(p) => p + 1,
        None => {
            // 整个缓冲里一个换行都没有：
            //   - 没读满上限 → 说明这是文件末尾的半行，先不给
            //   - 读满了上限 → 说明有一行超长（堆栈/JSON 日志），不能无限等，
            //     整段给出去，避免前端永远看不到内容
            if got < want || pending <= cap {
                0
            } else {
                got
            }
        }
    };

    let text = String::from_utf8_lossy(&buf[..cut]).to_string();
    let (redacted, _) = super::logs::redact(&text);
    let lines = redacted.lines().count() as u32;

    from += cut as u64;
    let pending_after = size.saturating_sub(from);

    Ok(LogChunk {
        id: id.to_string(),
        name,
        from_offset: from - cut as u64,
        next_offset: from,
        text: redacted,
        lines,
        eof: pending_after == 0,
        reset,
        pending_bytes: pending_after,
        truncated: cut < got,
    })
}

/// 找出该实例"当前正在写"的游戏日志（最新的 logs/*.log）。
///
/// 启动后前端不用猜文件名，直接用它拿 id 再 `tail`。
pub fn active_game_log(instance_dir: &Path) -> Option<super::logs::LogFile> {
    // 不能按 `modified_at` 字符串比：它只精确到秒，同一秒内写两个日志就分不出新旧。
    // 这里直接用文件系统的真实修改时间，取不到时间戳的排最后。
    super::logs::list(instance_dir)
        .into_iter()
        // 归档日志不能实时跟随，跳过（不然前端会拿到一个 tail 不了的文件）
        .filter(|l| l.kind == "game" && !l.compressed)
        .map(|l| {
            let stamp = super::logs::resolve_path(instance_dir, &l.id)
                .and_then(|(p, _)| std::fs::metadata(p).ok())
                .and_then(|m| m.modified().ok());
            (l, stamp)
        })
        .max_by(|a, b| match (a.1, b.1) {
            (Some(x), Some(y)) => x.cmp(&y),
            (Some(_), None) => std::cmp::Ordering::Greater,
            (None, Some(_)) => std::cmp::Ordering::Less,
            (None, None) => std::cmp::Ordering::Equal,
        })
        .map(|(l, _)| l)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d =
            std::env::temp_dir().join(format!("astral-live-{}-{tag}", crate::time::now_nanos()));
        std::fs::create_dir_all(d.join("logs")).unwrap();
        d
    }

    #[test]
    fn reads_only_complete_lines() {
        let d = tmp("partial");
        let p = d.join("logs").join("latest.log");
        std::fs::write(&p, b"[INFO] one\n[INFO] two\nhalf").unwrap();
        let l = crate::logs::list(&d);
        assert_eq!(l.len(), 1);
        let c = tail(&d, &l[0].id, 0, None).unwrap();
        assert_eq!(c.text, "[INFO] one\n[INFO] two\n");
        assert_eq!(c.lines, 2);
        assert!(!c.eof, "还有半行没读完，不该说 eof");
        // 半行补齐后再读一次就能拿到
        let mut all = std::fs::read(&p).unwrap();
        all.extend_from_slice(b" line\n");
        std::fs::write(&p, all).unwrap();
        let c2 = tail(&d, &l[0].id, c.next_offset, None).unwrap();
        assert_eq!(c2.text, "half line\n");
        assert!(c2.eof);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn resumes_from_offset() {
        let d = tmp("resume");
        let p = d.join("logs").join("latest.log");
        std::fs::write(&p, b"a\nb\n").unwrap();
        let id = crate::logs::list(&d)[0].id.clone();
        let c1 = tail(&d, &id, 0, None).unwrap();
        assert_eq!(c1.text, "a\nb\n");
        let c2 = tail(&d, &id, c1.next_offset, None).unwrap();
        assert_eq!(c2.text, "", "没有新内容时不该重复给旧行");
        assert_eq!(c2.next_offset, c1.next_offset);
        std::fs::write(&p, b"a\nb\nc\n").unwrap();
        let c3 = tail(&d, &id, c2.next_offset, None).unwrap();
        assert_eq!(c3.text, "c\n");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn resets_when_file_shrinks() {
        let d = tmp("shrink");
        let p = d.join("logs").join("latest.log");
        std::fs::write(&p, b"aaaaaaaaaa\nbbbbbbbbbb\n").unwrap();
        let id = crate::logs::list(&d)[0].id.clone();
        let c1 = tail(&d, &id, 0, None).unwrap();
        assert_eq!(c1.next_offset, 22);
        // 游戏重启，日志被重写
        std::fs::write(&p, b"new\n").unwrap();
        let c2 = tail(&d, &id, c1.next_offset, None).unwrap();
        assert!(c2.reset, "offset 超过文件长度必须报 reset");
        assert_eq!(c2.from_offset, 0);
        assert_eq!(c2.text, "new\n");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn caps_chunk_and_reports_pending() {
        let d = tmp("cap");
        let p = d.join("logs").join("latest.log");
        let mut body = Vec::new();
        for i in 0..200 {
            body.extend_from_slice(format!("line {i:04}\n").as_bytes());
        }
        std::fs::write(&p, &body).unwrap();
        let id = crate::logs::list(&d)[0].id.clone();
        let c = tail(&d, &id, 0, Some(100)).unwrap();
        assert!(c.text.len() <= 100);
        assert!(c.pending_bytes > 0);
        assert!(!c.eof);
        // 一直追到底，累计内容必须与原文一致
        let mut acc = String::new();
        let mut off = 0u64;
        for _ in 0..100 {
            let c = tail(&d, &id, off, Some(100)).unwrap();
            acc.push_str(&c.text);
            off = c.next_offset;
            if c.eof {
                break;
            }
        }
        assert_eq!(acc.as_bytes(), body.as_slice());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn missing_log_is_an_error() {
        let d = tmp("missing");
        assert!(tail(&d, "log_deadbeef", 0, None).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn finds_newest_game_log() {
        let d = tmp("active");
        std::fs::write(d.join("logs").join("old.log"), b"x\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(d.join("logs").join("latest.log"), b"y\n").unwrap();
        let a = active_game_log(&d).unwrap();
        assert_eq!(a.name, "latest.log");
        let _ = std::fs::remove_dir_all(&d);
    }
}
