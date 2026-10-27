//! 批量启用 / 禁用 Mod、资源包、光影包、数据包。
//!
//! 一个装了 200 个 Mod 的整合包，玩家想「全部关掉试试是不是 Mod 的问题」，
//! 现在只能在列表里一个一个点——点 200 次，中途还得记住点到哪了。
//!
//! 这里提供三件事：
//! 1. **预览**（`plan`）：说清楚要动几个文件、一共多大、有没有动不了的，
//!    不写盘；
//! 2. **执行**（`apply`）：逐个改名，单个失败不影响其他，最后报告结果；
//! 3. **只动文件层**：和单个启停一样靠改 `.disabled` 后缀，**不删文件、
//!    不改 `options.txt`、不动游戏存档**，玩家随时能在游戏里改回来。
//!
//! 失败的常见原因是文件被游戏或杀毒软件占着——这时只报告，不重试、不猜。

use serde::Serialize;
use std::path::Path;

/// 一次最多处理多少个文件。再多就是异常情况（或者玩家该分文件夹了）。
pub const MAX_FILES: usize = 2000;

/// 支持批量的种类，和 `packs::normalize_kind` 保持一致。
pub const KINDS: [&str; 4] = ["mod", "resourcepack", "shaderpack", "datapack"];

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BatchItem {
    /// 当前文件名（禁用态带 `.disabled`）
    pub file_name: String,
    /// 改完之后的名字
    pub target_name: String,
    /// 现在是不是启用的
    pub enabled: bool,
    /// 会不会真的动（已经是目标状态的不会动）
    pub will_change: bool,
    pub size_bytes: u64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BatchPlan {
    pub kind: String,
    /// 目标状态：true = 全部启用，false = 全部禁用
    pub enabled: bool,
    pub items: Vec<BatchItem>,
    /// 真正会被改名的数量
    pub changing: u32,
    /// 已经是目标状态、不用动的数量
    pub unchanged: u32,
    /// 会被改名的文件总字节数
    pub bytes: u64,
    /// 动不了的文件（被占用、名字异常等）
    pub blocked: Vec<String>,
    pub issues: Vec<String>,
    /// 有没有阻塞问题
    pub blocking: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BatchResult {
    pub kind: String,
    pub enabled: bool,
    /// 成功改名的文件名（改完之后的）
    pub changed: Vec<String>,
    /// 本来就是目标状态，没动
    pub unchanged: u32,
    /// 改失败的，带原因
    pub failed: Vec<BatchFailure>,
    pub bytes: u64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BatchFailure {
    pub file_name: String,
    pub reason: String,
}

/// 认不认这个种类。
pub fn normalize_kind(kind: &str) -> Result<&'static str, String> {
    let k = kind.trim().to_lowercase();
    let k = match k.as_str() {
        "mods" => "mod",
        other => other,
    };
    KINDS
        .iter()
        .find(|x| **x == k)
        .copied()
        .ok_or_else(|| format!("INVALID_VALUE|不支持的资源类型：{kind}"))
}

/// 拆出「主体名 + 是否启用」。和 `mods::split_name` / `packs::split_name` 同规则。
fn split_name(file_name: &str) -> Option<(String, bool)> {
    let lower = file_name.to_lowercase();
    if lower.ends_with(".disabled") {
        let stem = &file_name[..file_name.len() - ".disabled".len()];
        // 只认 `xxx.jar.disabled` / `xxx.zip.disabled`
        let l = stem.to_lowercase();
        if l.ends_with(".jar") || l.ends_with(".zip") {
            return Some((stem.to_string(), false));
        }
        return None;
    }
    if lower.ends_with(".jar") || lower.ends_with(".zip") {
        return Some((file_name.to_string(), true));
    }
    None
}

/// 数据包只认 zip，Mod 只认 jar——和各自的单文件列表保持一致。
fn accepts(kind: &str, file_name: &str) -> bool {
    let lower = file_name.to_lowercase();
    let body = lower.strip_suffix(".disabled").unwrap_or(&lower);
    match kind {
        "mod" => body.ends_with(".jar"),
        "datapack" => body.ends_with(".zip"),
        _ => body.ends_with(".zip") || body.ends_with(".jar"),
    }
}

/// 目标名冲突时依次加 `-1`、`-2`……（和单文件启停同一套规则）
fn unique_target(dir: &Path, stem: &str, suffix: &str) -> std::path::PathBuf {
    let first = dir.join(format!("{stem}{suffix}"));
    if !first.exists() {
        return first;
    }
    for i in 1..=50 {
        let p = dir.join(format!("{stem}-{i}{suffix}"));
        if !p.exists() {
            return p;
        }
    }
    dir.join(format!("{stem}-{}{suffix}", crate::time::now_unix_ms()))
}

/// 启用态就是原扩展名，禁用态在末尾加 `.disabled`
fn suffix_for(enabled: bool) -> &'static str {
    if enabled {
        ""
    } else {
        ".disabled"
    }
}

/// 只读预览：要动哪些文件、多大、有没有动不了的。
pub fn plan(dir: &Path, kind: &str, enabled: bool) -> Result<BatchPlan, String> {
    let kind = normalize_kind(kind)?;
    let mut items: Vec<BatchItem> = Vec::new();
    let mut blocked: Vec<String> = Vec::new();
    let mut issues: Vec<String> = Vec::new();

    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(BatchPlan {
                kind: kind.to_string(),
                enabled,
                items: Vec::new(),
                changing: 0,
                unchanged: 0,
                bytes: 0,
                blocked: Vec::new(),
                issues: vec!["这个目录还不存在（没装过这类文件）".into()],
                blocking: false,
            })
        }
        Err(e) => return Err(format!("PERMISSION|读不了目录|{e}")),
    };

    let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();

    for path in entries {
        if !path.is_file() {
            continue;
        }
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if !accepts(kind, &file_name) {
            continue;
        }
        let Some((stem, currently)) = split_name(&file_name) else {
            continue;
        };
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);

        // 符号链接一律不碰：改名会把链接本身移走，玩家的真实文件就"丢"了
        if crate::net::is_reparse_point(&path) {
            blocked.push(file_name.clone());
            continue;
        }

        let will_change = currently != enabled;
        let target_name = if will_change {
            let base = stem
                .strip_suffix(".jar")
                .or_else(|| stem.strip_suffix(".zip"))
                .unwrap_or(&stem);
            let ext = if stem.to_lowercase().ends_with(".zip") {
                ".zip"
            } else {
                ".jar"
            };
            unique_target(dir, base, &format!("{ext}{}", suffix_for(enabled)))
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| file_name.clone())
        } else {
            file_name.clone()
        };

        items.push(BatchItem {
            file_name,
            target_name,
            enabled: currently,
            will_change,
            size_bytes: size,
        });

        if items.len() > MAX_FILES {
            return Err(format!(
                "INVALID_VALUE|这个目录里的文件超过 {MAX_FILES} 个，请分批处理"
            ));
        }
    }

    let changing = items.iter().filter(|i| i.will_change).count() as u32;
    let unchanged = items.len() as u32 - changing;
    let bytes: u64 = items
        .iter()
        .filter(|i| i.will_change)
        .map(|i| i.size_bytes)
        .sum();

    if !blocked.is_empty() {
        issues.push(format!(
            "有 {} 个是符号链接，改名会把链接本身移走，已跳过",
            blocked.len()
        ));
    }
    if changing == 0 {
        issues.push(if enabled {
            "已经全部是启用状态了".into()
        } else {
            "已经全部是禁用状态了".into()
        });
    }

    Ok(BatchPlan {
        kind: kind.to_string(),
        enabled,
        items,
        changing,
        unchanged,
        bytes,
        blocked,
        issues,
        blocking: false,
    })
}

/// 真去改名。单个失败不影响其他，最后一起报告。
///
/// `only` 给了就只处理这些文件（前端可以只对筛选出来的那批动手），
/// 不给就整个目录。
pub fn apply(
    dir: &Path,
    kind: &str,
    enabled: bool,
    only: Option<&[String]>,
    running: bool,
) -> Result<BatchResult, String> {
    if running {
        return Err(
            "INSTANCE_BUSY|游戏正在运行，先退出游戏再批量改（游戏退出时可能覆盖文件）".into(),
        );
    }
    let plan = plan(dir, kind, enabled)?;
    let wanted: Option<std::collections::HashSet<&String>> = only.map(|list| list.iter().collect());

    let mut changed = Vec::new();
    let mut failed = Vec::new();
    let mut unchanged = 0u32;
    let mut bytes = 0u64;

    for item in &plan.items {
        if let Some(set) = &wanted {
            if !set.contains(&item.file_name) {
                continue;
            }
        }
        if !item.will_change {
            unchanged += 1;
            continue;
        }
        let src = dir.join(&item.file_name);
        let dest = dir.join(&item.target_name);
        match std::fs::rename(&src, &dest) {
            Ok(()) => {
                // 账本跟着改名，否则依赖解析会以为这个 Mod 被删了
                if plan.kind == "mod" {
                    crate::ledger::rename(dir, &item.file_name, &item.target_name);
                }
                changed.push(item.target_name.clone());
                bytes += item.size_bytes;
            }
            Err(e) => failed.push(BatchFailure {
                file_name: item.file_name.clone(),
                reason: format!("{e}"),
            }),
        }
    }

    Ok(BatchResult {
        kind: plan.kind,
        enabled,
        changed,
        unchanged,
        failed,
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d =
            std::env::temp_dir().join(format!("astral-batch-{tag}-{:x}", crate::time::now_nanos()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn splits_jar_and_zip_names() {
        assert_eq!(
            split_name("sodium.jar"),
            Some(("sodium.jar".to_string(), true))
        );
        assert_eq!(
            split_name("sodium.jar.disabled"),
            Some(("sodium.jar".to_string(), false))
        );
        assert_eq!(
            split_name("Faithful.zip.disabled"),
            Some(("Faithful.zip".to_string(), false))
        );
        assert_eq!(split_name("readme.txt"), None);
        assert_eq!(split_name("odd.disabled"), None);
    }

    #[test]
    fn accepts_only_matching_extensions() {
        assert!(accepts("mod", "a.jar"));
        assert!(accepts("mod", "a.jar.disabled"));
        assert!(!accepts("mod", "a.zip"));
        assert!(accepts("datapack", "a.zip"));
        assert!(!accepts("datapack", "a.jar"));
    }

    #[test]
    fn plan_counts_what_will_change() {
        let d = tmp("plan");
        std::fs::write(d.join("a.jar"), b"aaaa").unwrap();
        std::fs::write(d.join("b.jar.disabled"), b"bb").unwrap();
        std::fs::write(d.join("readme.txt"), b"x").unwrap();

        let p = plan(&d, "mod", false).unwrap();
        assert_eq!(p.changing, 1);
        assert_eq!(p.unchanged, 1);
        assert_eq!(p.bytes, 4);
        assert_eq!(p.items.len(), 2);
        let a = p.items.iter().find(|i| i.file_name == "a.jar").unwrap();
        assert_eq!(a.target_name, "a.jar.disabled");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn plan_is_read_only() {
        let d = tmp("readonly");
        std::fs::write(d.join("a.jar"), b"aaaa").unwrap();
        let _ = plan(&d, "mod", false).unwrap();
        assert!(d.join("a.jar").exists());
        assert!(!d.join("a.jar.disabled").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn apply_renames_everything() {
        let d = tmp("apply");
        std::fs::write(d.join("a.jar"), b"aaaa").unwrap();
        std::fs::write(d.join("b.jar"), b"bb").unwrap();
        let r = apply(&d, "mod", false, None, false).unwrap();
        assert_eq!(r.changed.len(), 2);
        assert!(r.failed.is_empty());
        assert_eq!(r.bytes, 6);
        assert!(d.join("a.jar.disabled").exists());
        assert!(d.join("b.jar.disabled").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn apply_can_be_limited_to_a_subset() {
        let d = tmp("subset");
        std::fs::write(d.join("a.jar"), b"a").unwrap();
        std::fs::write(d.join("b.jar"), b"b").unwrap();
        let only = vec!["a.jar".to_string()];
        let r = apply(&d, "mod", false, Some(&only), false).unwrap();
        assert_eq!(r.changed, vec!["a.jar.disabled".to_string()]);
        assert!(d.join("a.jar.disabled").exists());
        assert!(d.join("b.jar").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn apply_refuses_while_running() {
        let d = tmp("running");
        std::fs::write(d.join("a.jar"), b"a").unwrap();
        let e = apply(&d, "mod", false, None, true).unwrap_err();
        assert!(e.starts_with("INSTANCE_BUSY|"));
        assert!(d.join("a.jar").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn missing_dir_is_not_an_error() {
        let d = tmp("missing").join("nope");
        let p = plan(&d, "mod", false).unwrap();
        assert_eq!(p.changing, 0);
        assert!(p.items.is_empty());
        assert!(!p.blocking);
        let _ = std::fs::remove_dir_all(d.parent().unwrap());
    }

    #[test]
    fn already_in_target_state_changes_nothing() {
        let d = tmp("same");
        std::fs::write(d.join("a.jar"), b"a").unwrap();
        let r = apply(&d, "mod", true, None, false).unwrap();
        assert!(r.changed.is_empty());
        assert_eq!(r.unchanged, 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn rejects_unknown_kind() {
        let d = tmp("kind");
        assert!(plan(&d, "taco", false).is_err());
        assert!(apply(&d, "taco", false, None, false).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn normalizes_plural_kind() {
        assert_eq!(normalize_kind("mods").unwrap(), "mod");
        assert_eq!(normalize_kind("ResourcePack").unwrap(), "resourcepack");
        assert_eq!(normalize_kind("datapack").unwrap(), "datapack");
    }
}
