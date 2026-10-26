//! 可恢复的版本/实例回收站（C1）。
//!
//! 设计原则：
//! - 删除默认只是移动进 `instances/.trash/<id>-<时间戳>`，**永不自动清空**；
//! - 清空回收站是独立危险操作，必须显式 `confirm`；
//! - 恢复时如果原 id 已被占用，自动换一个新 id，绝不覆盖现存实例；
//! - 所有列表操作只读，不做任何删除。

use serde::Serialize;
use std::path::{Path, PathBuf};

/// 回收站目录名（相对 instances 根）
pub const TRASH_DIR: &str = ".trash";

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TrashEntry {
    /// 回收站内目录名（恢复与永久删除都用它做定位）
    pub trash_name: String,
    /// 原来的实例 id
    pub instance_id: String,
    /// 原实例名（读 instance.json；读不到就用 id）
    pub name: String,
    pub game_version: String,
    pub loader_kind: Option<String>,
    pub loader_version: Option<String>,
    /// 删除时间（ISO8601，取自目录名里的时间戳）
    pub deleted_at: String,
    pub bytes: u64,
    pub file_count: u64,
    /// 回收站里还留着 instance.json，说明结构完整，能安全恢复
    pub restorable: bool,
    /// 不能恢复的原因（能恢复时为空）
    pub reason: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResult {
    pub instance_id: String,
    pub trash_name: String,
    /// 因为 id 冲突被改名时是 true
    pub renamed: bool,
    /// 恢复后的健康检查问题（空 = 看起来正常）
    pub issues: Vec<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PurgeResult {
    pub removed: u32,
    pub bytes: u64,
    pub failed: Vec<(String, String)>,
}

fn trash_root(game: &Path) -> PathBuf {
    crate::instance::instances_root(game).join(TRASH_DIR)
}

fn dir_bytes(dir: &Path) -> (u64, u64) {
    let mut bytes = 0u64;
    let mut files = 0u64;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            // 不跟随符号链接，避免统计时跑到别的盘
            match std::fs::symlink_metadata(&p) {
                Ok(m) if m.is_dir() => stack.push(p),
                Ok(m) if m.is_file() => {
                    bytes = bytes.saturating_add(m.len());
                    files += 1;
                }
                _ => {}
            }
        }
    }
    (bytes, files)
}

/// 从 `<id>-<YYYYMMDD-HHMMSS>` 里拆出 id 与删除时间
fn split_name(name: &str) -> (String, String) {
    // 时间戳固定 15 个字符：YYYYMMDD-HHMMSS
    if name.len() > 16 {
        let (head, tail) = name.split_at(name.len() - 15);
        if tail.as_bytes().get(8) == Some(&b'-')
            && tail.chars().filter(|c| c.is_ascii_digit()).count() == 14
        {
            let id = head.trim_end_matches('-').to_string();
            let iso = format!(
                "{}-{}-{}T{}:{}:{}Z",
                &tail[0..4],
                &tail[4..6],
                &tail[6..8],
                &tail[9..11],
                &tail[11..13],
                &tail[13..15]
            );
            return (id, iso);
        }
    }
    (name.to_string(), String::new())
}

/// 列出回收站内容（只读）
pub fn list(game: &Path) -> Vec<TrashEntry> {
    let root = trash_root(game);
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(&root) else {
        return out;
    };
    for e in rd.flatten() {
        let path = e.path();
        if !path.is_dir() {
            continue;
        }
        let trash_name = e.file_name().to_string_lossy().to_string();
        let (instance_id, deleted_at) = split_name(&trash_name);
        let meta = crate::instance::read_meta_at(&path);
        let (bytes, file_count) = dir_bytes(&path);
        let restorable = meta.is_some();
        let reason = if restorable {
            String::new()
        } else {
            "回收站里缺少 instance.json，可能是旧版本删除或文件损坏".to_string()
        };
        out.push(TrashEntry {
            trash_name,
            name: meta
                .as_ref()
                .map(|m| m.name.clone())
                .unwrap_or_else(|| instance_id.clone()),
            game_version: meta
                .as_ref()
                .map(|m| m.game_version.clone())
                .unwrap_or_default(),
            loader_kind: meta.as_ref().and_then(|m| m.loader_kind.clone()),
            loader_version: meta.as_ref().and_then(|m| m.loader_version.clone()),
            instance_id,
            deleted_at,
            bytes,
            file_count,
            restorable,
            reason,
        });
    }
    // 最近删除的排前面
    out.sort_by(|a, b| b.deleted_at.cmp(&a.deleted_at));
    out
}

/// 恢复：id 被占用就换新 id，绝不覆盖
pub fn restore(game: &Path, trash_name: &str) -> Result<RestoreResult, String> {
    if trash_name.contains("..") || trash_name.contains('/') || trash_name.contains('\\') {
        return Err("INVALID_PATH|回收站名称不合法".into());
    }
    let root = trash_root(game);
    let src = root.join(trash_name);
    if !src.is_dir() {
        return Err("NOT_FOUND|回收站里没有这一项".into());
    }
    let (orig_id, _) = split_name(trash_name);
    let mut meta = crate::instance::read_meta_at(&src)
        .ok_or_else(|| "INVALID_VALUE|回收站里缺少 instance.json，无法恢复".to_string())?;

    let instances = crate::instance::instances_root(game);
    let mut target_id = orig_id.clone();
    let mut renamed = false;
    if crate::instance::instance_dir(game, &target_id).exists() || target_id.is_empty() {
        target_id = crate::instance::new_id();
        renamed = true;
        // 极端情况下 new_id 也撞了，再退一步
        let mut guard = 0;
        while crate::instance::instance_dir(game, &target_id).exists() && guard < 8 {
            target_id = crate::instance::new_id();
            guard += 1;
        }
    }
    let dest = instances.join(&target_id);
    if dest.exists() {
        return Err("INSTANCE_BUSY|目标目录已存在，已中止恢复".into());
    }
    std::fs::create_dir_all(&instances).map_err(|e| format!("PERMISSION|创建实例目录失败|{e}"))?;
    std::fs::rename(&src, &dest).map_err(|e| format!("PERMISSION|恢复失败|{e}"))?;

    // 元数据里的 id 必须跟目录名一致，否则实例发现逻辑会认错
    if meta.id != target_id {
        meta.id = target_id.clone();
        let text = serde_json::to_string_pretty(&meta)
            .map_err(|e| format!("INTERNAL|序列化实例元数据失败|{e}"))?;
        let file = dest.join("instance.json");
        let tmp = dest.join("instance.json.part");
        if let Err(e) = std::fs::write(&tmp, text) {
            return Err(format!("PERMISSION|写入实例元数据失败|{e}"));
        }
        if std::fs::rename(&tmp, &file).is_err() {
            let _ = std::fs::remove_file(&tmp);
            return Err("PERMISSION|保存实例元数据失败".into());
        }
    }

    // 恢复后做一次健康检查，把问题如实报给玩家
    let report = crate::verify::check(
        game,
        &meta.game_version,
        &crate::verify::CheckOptions::default(),
    );
    let mut issues = Vec::new();
    if report.missing > 0 {
        issues.push(format!("缺少 {} 个游戏文件", report.missing));
    }
    if report.corrupt > 0 {
        issues.push(format!("{} 个游戏文件损坏", report.corrupt));
    }
    if !report.compatible {
        issues.push("当前 Java 环境不满足要求".to_string());
    }
    Ok(RestoreResult {
        instance_id: target_id,
        trash_name: trash_name.to_string(),
        renamed,
        issues,
    })
}

/// 永久删除回收站里的指定项（必须显式列出，不做"全删"）
pub fn purge(game: &Path, trash_names: &[String]) -> Result<PurgeResult, String> {
    if trash_names.is_empty() {
        return Err("INVALID_VALUE|没有指定要永久删除的项目".into());
    }
    let root = trash_root(game);
    let mut removed = 0u32;
    let mut bytes = 0u64;
    let mut failed = Vec::new();
    for name in trash_names {
        if name.contains("..") || name.contains('/') || name.contains('\\') {
            failed.push((name.clone(), "名称不合法".to_string()));
            continue;
        }
        let p = root.join(name);
        if !p.is_dir() {
            failed.push((name.clone(), "不存在".to_string()));
            continue;
        }
        let (b, _) = dir_bytes(&p);
        match std::fs::remove_dir_all(&p) {
            Ok(()) => {
                removed += 1;
                bytes = bytes.saturating_add(b);
            }
            Err(e) => failed.push((name.clone(), e.to_string())),
        }
    }
    Ok(PurgeResult {
        removed,
        bytes,
        failed,
    })
}

/// 清空整个回收站（危险操作，必须由调用方确认过）
pub fn purge_all(game: &Path) -> Result<PurgeResult, String> {
    let root = trash_root(game);
    let Ok(rd) = std::fs::read_dir(&root) else {
        return Ok(PurgeResult {
            removed: 0,
            bytes: 0,
            failed: vec![],
        });
    };
    let names: Vec<String> = rd
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    purge(game, &names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_name_recovers_id_and_time() {
        let (id, at) = split_name("inst_0000abcd-20261026-223000");
        assert_eq!(id, "inst_0000abcd");
        assert_eq!(at, "2026-10-26T22:30:00Z");
    }

    #[test]
    fn split_name_tolerates_odd_names() {
        let (id, at) = split_name("whatever");
        assert_eq!(id, "whatever");
        assert!(at.is_empty());
    }

    #[test]
    fn rejects_traversal_names() {
        let g = std::env::temp_dir().join("astral-recycle-test");
        let _ = std::fs::create_dir_all(&g);
        assert!(restore(&g, "../evil").is_err());
        assert!(purge(&g, &["..".to_string()]).map(|r| r.failed.len()).unwrap_or(1) >= 1);
        let _ = std::fs::remove_dir_all(&g);
    }
}
