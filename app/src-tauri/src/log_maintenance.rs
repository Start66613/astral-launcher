//! 日志清理（V2.5 E34）。
//!
//! `analyze_disk_usage` 把 `logs` 标成「可以安全清理」，但之前**没有任何命令能清**：
//! 玩家只能自己去文件夹里挑着删，一不小心把 `latest.log` 删了，下次出问题就没法查。
//!
//! 这里补上「先预览、再清理」的两步：
//! - `log_usage`：只读，告诉你日志占了多少、最老的是哪天；
//! - `plan_log_cleanup`：只读，列出**准备删哪些**、**保留哪些**、为什么保留；
//! - `prune_logs`：真要删，必须 `confirm`。
//!
//! 三条硬规则（不管参数怎么传都成立）：
//! 1. `latest.log` 永远不删 —— 它是当前这次启动的日志；
//! 2. 只碰 `logs/` 与 `crash-reports/` 里的文本日志，别的文件一概不认；
//! 3. 符号链接与重解析点不跟。

use serde::Serialize;
use std::path::{Path, PathBuf};

/// 默认保留最近多少天
pub const DEFAULT_KEEP_DAYS: u64 = 30;
/// 默认至少保留几份最新的日志
pub const DEFAULT_KEEP_COUNT: u32 = 5;
/// 单次最多处理多少个文件（防止一个异常目录把界面卡死）
const MAX_FILES: usize = 5000;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LogUsage {
    pub instance_id: String,
    /// `logs/` 的绝对路径
    pub logs_dir: String,
    pub files: u32,
    pub bytes: u64,
    /// 其中 `.gz` 归档的份数与占用
    pub archives: u32,
    pub archive_bytes: u64,
    pub oldest: Option<String>,
    pub newest: Option<String>,
    /// 崩溃报告单独算一份
    pub crash_files: u32,
    pub crash_bytes: u64,
    pub truncated: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LogFileItem {
    /// 相对实例目录的路径，例如 `logs/2025-01-01-1.log.gz`
    pub path: String,
    pub bytes: u64,
    pub modified_at: String,
    /// 为什么留着（只有 `keep` 里才有值）
    pub reason: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LogCleanupPlan {
    pub instance_id: String,
    pub keep_days: u64,
    pub keep_count: u32,
    /// 准备删的
    pub remove: Vec<LogFileItem>,
    pub remove_bytes: u64,
    /// 保留的（含 latest.log）
    pub keep: Vec<LogFileItem>,
    pub keep_bytes: u64,
    pub issues: Vec<String>,
    /// 删完能省多少（= `remove_bytes`，给前端直接显示）
    pub freed_bytes: u64,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LogCleanupResult {
    pub instance_id: String,
    pub removed: u32,
    pub freed_bytes: u64,
    pub kept: u32,
    /// 删失败的文件（通常是正在被占用）
    pub failed: Vec<String>,
}

fn is_log_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".log")
        || lower.ends_with(".txt")
        || lower.ends_with(".log.gz")
        || lower.ends_with(".txt.gz")
}

fn mtime_ms(p: &Path) -> i64 {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn rel_of(instance_dir: &Path, p: &Path) -> String {
    p.strip_prefix(instance_dir)
        .map(|r| r.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| p.to_string_lossy().to_string())
}

/// 收集一个目录下的日志文件：`(相对路径, 绝对路径, 字节数, mtime_ms)`
fn collect_dir(
    instance_dir: &Path,
    dir: &Path,
    out: &mut Vec<(String, PathBuf, u64, i64)>,
) -> bool {
    let mut truncated = false;
    let Ok(rd) = std::fs::read_dir(dir) else {
        return false;
    };
    for e in rd.flatten() {
        if out.len() >= MAX_FILES {
            truncated = true;
            break;
        }
        let p = e.path();
        let Ok(md) = std::fs::symlink_metadata(&p) else {
            continue;
        };
        if md.file_type().is_symlink() || crate::net::is_reparse_point(&p) {
            continue;
        }
        if !md.is_file() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        if !is_log_name(&name) {
            continue;
        }
        let ms = mtime_ms(&p);
        out.push((rel_of(instance_dir, &p), p, md.len(), ms));
    }
    truncated
}

fn all_logs(instance_dir: &Path) -> (Vec<(String, PathBuf, u64, i64)>, bool) {
    let mut out = Vec::new();
    let a = collect_dir(instance_dir, &instance_dir.join("logs"), &mut out);
    let b = collect_dir(instance_dir, &instance_dir.join("crash-reports"), &mut out);
    (out, a || b)
}

/// 只读：日志占了多少
pub fn usage(instance_dir: &Path, instance_id: &str) -> LogUsage {
    let (files, truncated) = all_logs(instance_dir);
    let mut u = LogUsage {
        instance_id: instance_id.to_string(),
        logs_dir: instance_dir.join("logs").to_string_lossy().to_string(),
        files: 0,
        bytes: 0,
        archives: 0,
        archive_bytes: 0,
        oldest: None,
        newest: None,
        crash_files: 0,
        crash_bytes: 0,
        truncated,
    };
    let mut oldest_ms = i64::MAX;
    let mut newest_ms = i64::MIN;
    for (rel, _p, size, ms) in &files {
        if rel.starts_with("crash-reports/") {
            u.crash_files += 1;
            u.crash_bytes += size;
        } else {
            u.files += 1;
            u.bytes += size;
            if rel.to_ascii_lowercase().ends_with(".gz") {
                u.archives += 1;
                u.archive_bytes += size;
            }
        }
        if *ms > 0 {
            oldest_ms = oldest_ms.min(*ms);
            newest_ms = newest_ms.max(*ms);
        }
    }
    if oldest_ms != i64::MAX {
        u.oldest = Some(crate::time::unix_ms_to_iso(oldest_ms));
    }
    if newest_ms != i64::MIN {
        u.newest = Some(crate::time::unix_ms_to_iso(newest_ms));
    }
    u
}

/// 只读：算出准备删哪些、留哪些
pub fn plan(
    instance_dir: &Path,
    instance_id: &str,
    keep_days: u64,
    keep_count: u32,
) -> LogCleanupPlan {
    let keep_days = keep_days.min(3650);
    let keep_count = keep_count.clamp(1, 200);
    let cutoff = crate::time::now_unix_ms() - (keep_days as i64) * 24 * 3600 * 1000;

    let (mut files, truncated) = all_logs(instance_dir);
    // 新的在前，方便按"最新 N 份"保留
    files.sort_by_key(|(_, _, _, ms)| std::cmp::Reverse(*ms));

    let mut keep: Vec<LogFileItem> = Vec::new();
    let mut remove: Vec<LogFileItem> = Vec::new();
    let mut kept_newest = 0u32;

    for (rel, _p, size, ms) in &files {
        let name = rel.rsplit('/').next().unwrap_or(rel);
        let modified_at = if *ms > 0 {
            crate::time::unix_ms_to_iso(*ms)
        } else {
            String::new()
        };
        let item = LogFileItem {
            path: rel.clone(),
            bytes: *size,
            modified_at,
            reason: None,
        };

        // 规则 1：latest.log 永远保留
        if name.eq_ignore_ascii_case("latest.log") {
            keep.push(LogFileItem {
                reason: Some("这是当前这次启动的日志，不能删".into()),
                ..item
            });
            continue;
        }
        // 规则 2：最新 keep_count 份保留
        if kept_newest < keep_count {
            kept_newest += 1;
            keep.push(LogFileItem {
                reason: Some(format!("最近 {keep_count} 份日志之一")),
                ..item
            });
            continue;
        }
        // 规则 3：keep_days 天内改动过的保留（keep_days = 0 表示不按天数保）
        if keep_days > 0 && *ms >= cutoff && *ms > 0 {
            keep.push(LogFileItem {
                reason: Some(format!("{keep_days} 天内还写过")),
                ..item
            });
            continue;
        }
        remove.push(item);
    }

    let mut issues = Vec::new();
    if truncated {
        issues.push(format!("日志文件超过 {MAX_FILES} 个，这次只处理了一部分"));
    }

    let remove_bytes: u64 = remove.iter().map(|i| i.bytes).sum();
    let keep_bytes: u64 = keep.iter().map(|i| i.bytes).sum();
    LogCleanupPlan {
        instance_id: instance_id.to_string(),
        keep_days,
        keep_count,
        freed_bytes: remove_bytes,
        remove,
        remove_bytes,
        keep,
        keep_bytes,
        issues,
    }
}

/// 真删。`confirm` 必须为 `true`。
pub fn prune(
    instance_dir: &Path,
    instance_id: &str,
    keep_days: u64,
    keep_count: u32,
    confirm: bool,
) -> Result<LogCleanupResult, String> {
    if !confirm {
        return Err("UNSUPPORTED|清理日志需要明确确认".into());
    }
    let p = plan(instance_dir, instance_id, keep_days, keep_count);
    let mut removed = 0u32;
    let mut freed = 0u64;
    let mut failed = Vec::new();

    for item in &p.remove {
        let full = instance_dir.join(item.path.replace('/', std::path::MAIN_SEPARATOR_STR));
        // 再确认一次没跑出实例目录（路径来自我们自己拼的相对路径，但双保险）
        if crate::net::ensure_within(instance_dir, &full).is_err() {
            failed.push(item.path.clone());
            continue;
        }
        if std::fs::symlink_metadata(&full)
            .map(|m| m.file_type().is_symlink() || crate::net::is_reparse_point(&full))
            .unwrap_or(false)
        {
            failed.push(item.path.clone());
            continue;
        }
        match std::fs::remove_file(&full) {
            Ok(()) => {
                removed += 1;
                freed += item.bytes;
            }
            // 被占用（游戏还开着）或已经没了：都记下来，不算致命
            Err(_) => failed.push(item.path.clone()),
        }
    }

    Ok(LogCleanupResult {
        instance_id: instance_id.to_string(),
        removed,
        freed_bytes: freed,
        kept: p.keep.len() as u32,
        failed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "astral-logmaint-{tag}-{}-{}",
            std::process::id(),
            crate::time::now_nanos()
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(p.join("logs")).unwrap();
        std::fs::create_dir_all(p.join("crash-reports")).unwrap();
        p
    }

    fn write(p: &Path, text: &str) {
        let mut f = std::fs::File::create(p).unwrap();
        f.write_all(text.as_bytes()).unwrap();
    }

    #[test]
    fn usage_counts_logs_and_crashes_separately() {
        let d = tmp("usage");
        write(&d.join("logs").join("latest.log"), "abc");
        write(&d.join("logs").join("2025-01-01-1.log.gz"), "0123456789");
        write(&d.join("crash-reports").join("crash-1.txt"), "xx");
        write(&d.join("logs").join("notes.md"), "ignored");
        let u = usage(&d, "i1");
        assert_eq!(u.files, 2);
        assert_eq!(u.bytes, 13);
        assert_eq!(u.archives, 1);
        assert_eq!(u.archive_bytes, 10);
        assert_eq!(u.crash_files, 1);
        assert_eq!(u.crash_bytes, 2);
        assert!(u.oldest.is_some());
        assert!(u.newest.is_some());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn plan_always_keeps_latest_log() {
        let d = tmp("latest");
        write(&d.join("logs").join("latest.log"), "now");
        // 造一个很老的归档
        let old = d.join("logs").join("2019-01-01-1.log.gz");
        write(&old, "old");
        let p = plan(&d, "i1", 30, 1);
        assert!(p.keep.iter().any(|i| i.path.ends_with("latest.log")));
        assert!(p.remove.iter().all(|i| !i.path.ends_with("latest.log")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn plan_keeps_the_newest_n() {
        let d = tmp("newest");
        for i in 0..6 {
            let p = d
                .join("logs")
                .join(format!("2025-01-0{}-{i}.log.gz", i + 1));
            write(&p, "x");
            // 手动拉开 mtime 不现实，这里只验证数量上限
        }
        let p = plan(&d, "i1", 0, 3);
        assert_eq!(p.keep.len(), 3, "只保留最新 3 份");
        assert_eq!(p.remove.len(), 3);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn plan_keeps_recent_files_within_days() {
        let d = tmp("recent");
        write(&d.join("logs").join("just-now.log"), "a");
        // keep_count 传 1，latest.log 不存在时"最新 1 份"被 just-now 占掉；
        // 再加一个刚写的，它应该因为 keep_days 被保留
        write(&d.join("logs").join("also-now.log"), "b");
        let p = plan(&d, "i1", 30, 1);
        assert_eq!(p.remove.len(), 0, "都是刚写的，不该删");
        assert_eq!(p.keep.len(), 2);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn plan_ignores_other_files() {
        let d = tmp("ignore");
        write(&d.join("logs").join("latest.log"), "a");
        write(&d.join("logs").join("settings.json"), "{}");
        write(&d.join("logs").join("debug-profile.txt"), "keep");
        let p = plan(&d, "i1", 0, 1);
        let all: Vec<&str> = p
            .keep
            .iter()
            .chain(p.remove.iter())
            .map(|i| i.path.as_str())
            .collect();
        assert!(!all.iter().any(|p| p.ends_with(".json")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn prune_needs_confirm() {
        let d = tmp("confirm");
        write(&d.join("logs").join("old.log"), "x");
        assert!(prune(&d, "i1", 0, 1, false).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn prune_removes_only_the_planned_files() {
        let d = tmp("prune");
        write(&d.join("logs").join("latest.log"), "keep me");
        write(&d.join("logs").join("2019-01-01-1.log.gz"), "bye");
        write(&d.join("logs").join("2019-01-02-1.log.gz"), "bye too");
        let before = plan(&d, "i1", 0, 1);
        let r = prune(&d, "i1", 0, 1, true).unwrap();
        // 计划里说要删几个，就真删掉几个
        assert_eq!(r.removed as usize, before.remove.len());
        assert_eq!(r.freed_bytes, before.remove_bytes);
        assert!(
            d.join("logs").join("latest.log").is_file(),
            "latest.log 不能被删"
        );
        // 计划说留的都在，计划说删的都没了
        for item in &before.keep {
            let full = d.join(item.path.replace('/', std::path::MAIN_SEPARATOR_STR));
            assert!(full.is_file(), "计划要留的 {} 不见了", item.path);
        }
        for item in &before.remove {
            let full = d.join(item.path.replace('/', std::path::MAIN_SEPARATOR_STR));
            assert!(!full.exists(), "计划要删的 {} 还在", item.path);
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn zero_keep_days_means_do_not_keep_by_age() {
        let d = tmp("zerodays");
        write(&d.join("logs").join("latest.log"), "a");
        // 两个非 latest 的日志：一个刚写，一个一小时前写
        write(&d.join("logs").join("newer.log"), "b");
        let older = d.join("logs").join("older.log");
        write(&older, "c");
        let back = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::File::options()
            .write(true)
            .open(&older)
            .unwrap()
            .set_modified(back)
            .unwrap();

        // keep_count=1 只保住最新的 newer.log；older.log 只能靠 keep_days 留下
        let p0 = plan(&d, "i1", 0, 1);
        assert!(
            p0.remove.iter().any(|i| i.path.ends_with("older.log")),
            "keep_days=0 时不该按天数保留"
        );
        let p30 = plan(&d, "i1", 30, 1);
        assert!(
            p30.keep.iter().any(|i| i.path.ends_with("older.log")),
            "keep_days=30 时一小时前的日志该留下"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn empty_dir_is_harmless() {
        let d = tmp("empty");
        let u = usage(&d, "i1");
        assert_eq!(u.files, 0);
        assert_eq!(u.bytes, 0);
        assert!(u.oldest.is_none());
        let p = plan(&d, "i1", 30, 5);
        assert!(p.remove.is_empty());
        assert!(p.keep.is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}
