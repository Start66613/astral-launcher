//! 日志搜索与按级别筛选（契约 V2 §10 增量）。
//!
//! 只读本地日志：在已经读进内存的那一段里找关键词、按级别过滤，顺便
//! 统计各级别各有多少条，让玩家一眼看到「这次启动报了几个错」。
//!
//! 设计取舍：
//! - 超大日志只扫尾部（`MAX_SCAN`），最近的才有意义；此时 `lineNo` 从这段
//!   开头算起，用 `scannedTailOnly` + `note` 告诉前端，不假装是全文行号。
//! - 命中太多时只返回**最近**的 `limit` 条（`skipped` 是没返回的旧条数），
//!   因为排查问题基本都在看最后发生了什么。

use serde::Serialize;
use std::path::Path;

/// 一次最多返回多少条命中
pub const DEFAULT_LIMIT: u32 = 200;
/// 上限，防止一次把几十万行全塞给前端
pub const MAX_LIMIT: u32 = 2000;
/// 最多扫描多少字节（超大日志只扫尾部）
const MAX_SCAN: u64 = 16 * 1024 * 1024;
/// `.gz` 解压后的硬上限
const MAX_UNPACKED: u64 = 64 * 1024 * 1024;

/// 支持的级别名（小写）
pub const LEVELS: [&str; 5] = ["error", "warn", "info", "debug", "trace"];

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct LevelCounts {
    pub error: u32,
    pub warn: u32,
    pub info: u32,
    pub debug: u32,
    pub trace: u32,
    /// 没有级别的行（堆栈、原始输出等）
    pub other: u32,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LogMatch {
    /// 行号，从 1 开始（只扫尾部时从这段开头算起）
    pub line_no: u32,
    pub level: Option<String>,
    pub text: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LogSearchResult {
    pub id: String,
    pub name: String,
    pub query: String,
    /// 生效的级别过滤（`None` 表示不过滤）
    pub level: Option<String>,
    pub case_sensitive: bool,
    /// 扫到的命中总数
    pub total_matches: u32,
    /// 实际返回条数
    pub returned: u32,
    /// 因为 limit 没返回的旧条数
    pub skipped: u32,
    pub matches: Vec<LogMatch>,
    pub level_counts: LevelCounts,
    pub scanned_lines: u32,
    pub scanned_bytes: u64,
    /// 只扫了尾部（日志太大）
    pub scanned_tail_only: bool,
    pub note: Option<String>,
}

/// 把各种写法归一到五个级别
fn normalize_level(tag: &str) -> Option<&'static str> {
    let t = tag.trim();
    // `[main/ERROR]` 这种取斜杠后面
    let t = t.rsplit('/').next().unwrap_or(t).trim();
    match t.to_ascii_uppercase().as_str() {
        "ERROR" | "FATAL" | "SEVERE" => Some("error"),
        "WARN" | "WARNING" => Some("warn"),
        "INFO" => Some("info"),
        "DEBUG" | "FINE" => Some("debug"),
        "TRACE" | "FINEST" => Some("trace"),
        _ => None,
    }
}

/// 从一行里认出级别。只看行首 200 个字符，避免把正文里的方括号当级别。
fn level_in(line: &str) -> Option<&'static str> {
    let head = match line.char_indices().nth(200) {
        Some((i, _)) => &line[..i],
        None => line,
    };
    let mut rest = head;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let Some(close) = after.find(']') else { break };
        if let Some(lv) = normalize_level(&after[..close]) {
            return Some(lv);
        }
        rest = &after[close + 1..];
    }
    None
}

/// 校验并归一调用方传进来的级别
pub fn normalize_filter(level: Option<&str>) -> Result<Option<&'static str>, String> {
    let Some(raw) = level else { return Ok(None) };
    let t = raw.trim();
    if t.is_empty() {
        return Ok(None);
    }
    let lower = t.to_ascii_lowercase();
    let lower = match lower.as_str() {
        "warning" => "warn".to_string(),
        other => other.to_string(),
    };
    LEVELS
        .iter()
        .copied()
        .find(|l| *l == lower)
        .map(Some)
        .ok_or_else(|| format!("INVALID_VALUE|不支持的日志级别：{raw}"))
}

fn count_of(counts: &mut LevelCounts, lv: Option<&str>) {
    match lv {
        Some("error") => counts.error += 1,
        Some("warn") => counts.warn += 1,
        Some("info") => counts.info += 1,
        Some("debug") => counts.debug += 1,
        Some("trace") => counts.trace += 1,
        _ => counts.other += 1,
    }
}

/// 在文本里扫一遍。返回 (要返回的命中, 级别统计, 命中总数, 扫过的行数)
fn scan(
    text: &str,
    query: &str,
    level: Option<&str>,
    case_sensitive: bool,
    limit: usize,
) -> (Vec<LogMatch>, LevelCounts, u32, u32) {
    let needle = if case_sensitive {
        query.to_string()
    } else {
        query.to_lowercase()
    };
    let mut counts = LevelCounts::default();
    let mut hits: Vec<LogMatch> = Vec::new();
    let mut total = 0u32;
    let mut lines = 0u32;

    for (i, raw) in text.lines().enumerate() {
        lines = lines.saturating_add(1);
        let lv = level_in(raw);
        count_of(&mut counts, lv);
        if let Some(want) = level {
            if lv != Some(want) {
                continue;
            }
        }
        let matched = if needle.is_empty() {
            true
        } else if case_sensitive {
            raw.contains(&needle)
        } else {
            raw.to_lowercase().contains(&needle)
        };
        if !matched {
            continue;
        }
        total = total.saturating_add(1);
        hits.push(LogMatch {
            line_no: (i + 1) as u32,
            level: lv.map(|s| s.to_string()),
            text: raw.to_string(),
        });
    }

    // 只留最近的 limit 条
    let dropped = hits.len().saturating_sub(limit);
    let tail = if dropped > 0 {
        hits.split_off(dropped)
    } else {
        hits
    };
    (tail, counts, total, lines)
}

/// 读日志正文：普通文件只读尾部，`.gz` 先解压
fn read_text(path: &Path, compressed: bool) -> Result<(String, u64, bool), String> {
    if compressed {
        use std::io::Read;
        let file = std::fs::File::open(path).map_err(|e| format!("PERMISSION|读取失败|{e}"))?;
        let mut dec = flate2::read::GzDecoder::new(std::io::BufReader::new(file));
        let mut all: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 64 * 1024];
        loop {
            let n = match dec.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => return Err(format!("INVALID_VALUE|这个日志解压失败|{e}")),
            };
            all.extend_from_slice(&chunk[..n]);
            if all.len() as u64 > MAX_UNPACKED {
                return Err(format!(
                    "INVALID_VALUE|日志解压后超过 {} MB，没法整份搜索",
                    MAX_UNPACKED / (1024 * 1024)
                ));
            }
        }
        let total = all.len() as u64;
        let tail_only = total > MAX_SCAN;
        if tail_only {
            let drop = all.len() - MAX_SCAN as usize;
            all.drain(..drop);
            // 丢掉开头那半行
            if let Some(p) = all.iter().position(|b| *b == b'\n') {
                all.drain(..p + 1);
            }
        }
        return Ok((String::from_utf8_lossy(&all).to_string(), total, tail_only));
    }

    let size = std::fs::metadata(path)
        .map_err(|e| format!("PERMISSION|读取失败|{e}"))?
        .len();
    if size > MAX_SCAN {
        use std::io::{Read, Seek, SeekFrom};
        let mut f = std::fs::File::open(path).map_err(|e| format!("PERMISSION|读取失败|{e}"))?;
        f.seek(SeekFrom::Start(size - MAX_SCAN))
            .map_err(|e| format!("PERMISSION|定位失败|{e}"))?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)
            .map_err(|e| format!("PERMISSION|读取失败|{e}"))?;
        // 开头是半行，丢掉
        let start = buf
            .iter()
            .position(|b| *b == b'\n')
            .map(|p| p + 1)
            .unwrap_or(0);
        return Ok((
            String::from_utf8_lossy(&buf[start..]).to_string(),
            size,
            true,
        ));
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("PERMISSION|读取失败|{e}"))?;
    Ok((text, size, false))
}

/// 在指定日志里搜索
pub fn search(
    instance_dir: &Path,
    id: &str,
    query: &str,
    level: Option<&str>,
    case_sensitive: bool,
    limit: Option<u32>,
) -> Result<LogSearchResult, String> {
    let want = normalize_filter(level)?;
    let (path, name) = crate::logs::resolve_path(instance_dir, id)
        .ok_or_else(|| "INVALID_PATH|日志不存在".to_string())?;
    let compressed = name.to_lowercase().ends_with(".gz");
    let (text, scanned_bytes, tail_only) = read_text(&path, compressed)?;
    let limit = limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT) as usize;
    let (matches, counts, total, lines) = scan(&text, query, want, case_sensitive, limit);
    let returned = matches.len() as u32;
    let skipped = total.saturating_sub(returned);
    let note = if tail_only {
        Some(format!(
            "日志很大，只搜了最后 {} MB；行号从这一段开头算起",
            MAX_SCAN / (1024 * 1024)
        ))
    } else {
        None
    };
    Ok(LogSearchResult {
        id: id.to_string(),
        name,
        query: query.to_string(),
        level: want.map(|s| s.to_string()),
        case_sensitive,
        total_matches: total,
        returned,
        skipped,
        matches,
        level_counts: counts,
        scanned_lines: lines,
        scanned_bytes,
        scanned_tail_only: tail_only,
        note,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d =
            std::env::temp_dir().join(format!("astral-ls-{tag}-{:x}", crate::time::now_nanos()));
        std::fs::create_dir_all(d.join("logs")).unwrap();
        d
    }

    const SAMPLE: &str = "[10:00:01] [main/INFO]: Loading Minecraft 1.20.1\n\
[10:00:02] [main/WARN]: Option not found\n\
[10:00:03] [Render thread/ERROR]: Failed to load fabric api\n\
[10:00:04] [Worker-1/ERROR]: Another fabric problem\n\
[10:00:05] [main/INFO]: Done\n\
java.lang.RuntimeException: boom\n";

    #[test]
    fn finds_matching_lines() {
        let d = tmp("find");
        std::fs::write(d.join("logs/latest.log"), SAMPLE).unwrap();
        let r = search(
            &d,
            &crate::logs::list(&d)[0].id,
            "fabric",
            None,
            false,
            None,
        )
        .unwrap();
        assert_eq!(r.total_matches, 2);
        assert_eq!(r.returned, 2);
        assert_eq!(r.matches[0].line_no, 3);
        assert!(r.matches[0].text.contains("Failed to load"));
        assert_eq!(r.level_counts.error, 2);
        assert_eq!(r.level_counts.warn, 1);
        assert_eq!(r.level_counts.info, 2);
        assert_eq!(r.level_counts.other, 1);
        assert!(!r.scanned_tail_only);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn filters_by_level() {
        let d = tmp("level");
        std::fs::write(d.join("logs/latest.log"), SAMPLE).unwrap();
        let id = crate::logs::list(&d)[0].id.clone();
        let r = search(&d, &id, "", Some("error"), false, None).unwrap();
        assert_eq!(r.returned, 2);
        assert!(r
            .matches
            .iter()
            .all(|m| m.level.as_deref() == Some("error")));
        // 级别过滤不影响统计
        assert_eq!(r.level_counts.info, 2);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn is_case_insensitive_by_default() {
        let d = tmp("case");
        std::fs::write(d.join("logs/latest.log"), SAMPLE).unwrap();
        let id = crate::logs::list(&d)[0].id.clone();
        let loose = search(&d, &id, "FABRIC", None, false, None).unwrap();
        assert_eq!(loose.total_matches, 2);
        let strict = search(&d, &id, "FABRIC", None, true, None).unwrap();
        assert_eq!(strict.total_matches, 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn caps_results_and_reports_skipped() {
        let d = tmp("cap");
        std::fs::write(d.join("logs/latest.log"), SAMPLE).unwrap();
        let id = crate::logs::list(&d)[0].id.clone();
        let r = search(&d, &id, "", Some("info"), false, Some(1)).unwrap();
        assert_eq!(r.total_matches, 2);
        assert_eq!(r.returned, 1);
        assert_eq!(r.skipped, 1);
        // 留的是最近那条
        assert!(r.matches[0].text.contains("Done"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn rejects_unknown_level_and_missing_log() {
        let d = tmp("bad");
        std::fs::write(d.join("logs/latest.log"), SAMPLE).unwrap();
        let id = crate::logs::list(&d)[0].id.clone();
        assert!(search(&d, &id, "", Some("verbose"), false, None).is_err());
        assert!(search(&d, "log_nope", "", None, false, None).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn recognizes_level_tags() {
        assert_eq!(level_in("[10:00:01] [main/INFO]: hi"), Some("info"));
        assert_eq!(
            level_in("[10:00:01] [Render thread/ERROR]: hi"),
            Some("error")
        );
        assert_eq!(level_in("[main/warn] lowercase"), Some("warn"));
        assert_eq!(level_in("[WARNING] bare tag"), Some("warn"));
        assert_eq!(level_in("java.lang.RuntimeException: boom"), None);
        assert_eq!(
            level_in("at net.fabricmc.Loader.start(Loader.java:1)"),
            None
        );
        // 正文里的方括号不该被当成级别
        assert_eq!(
            level_in("[10:00:01] [main/INFO]: see [FATAL] option"),
            Some("info")
        );
    }
}
