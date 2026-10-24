//! 存档目标（数据包安装用）。
//!
//! 关键点：`worldId` 不是显示名，也不是目录名，而是由目录名派生的稳定 ID。
//! 前端只能拿 `worldId` 回来，Rust 负责映射回真实目录，避免路径拼接。

use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WorldTarget {
    /// `w_<8 位十六进制>`，由目录名派生
    pub world_id: String,
    pub display_name: String,
    pub dir: String,
    pub last_modified: String,
    pub size_bytes: u64,
    pub game_version: Option<String>,
}

/// 目录名 → 稳定 ID（同名目录永远得到同一个 ID）
pub fn world_id(name: &str) -> String {
    use sha1::{Digest, Sha1};
    let mut h = Sha1::new();
    h.update(name.as_bytes());
    let digest = h.finalize();
    let hex: String = digest.iter().take(4).map(|b| format!("{b:02x}")).collect();
    format!("w_{hex}")
}

fn dir_size(path: &Path) -> u64 {
    fn walk(p: &Path, acc: &mut u64, depth: u32) {
        if depth > 6 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(p) else { return };
        for e in rd.flatten() {
            let path = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => walk(&path, acc, depth + 1),
                Ok(t) if t.is_file() => {
                    *acc += e.metadata().map(|m| m.len()).unwrap_or(0);
                }
                _ => {}
            }
        }
    }
    let mut acc = 0;
    walk(path, &mut acc, 0);
    acc
}

fn last_modified(path: &Path) -> String {
    std::fs::metadata(path.join("level.dat"))
        .and_then(|m| m.modified())
        .map(crate::time::system_time_to_iso)
        .unwrap_or_else(|_| crate::time::now_iso())
}

/// 列出某个 `saves/` 下所有真实存档（必须含 level.dat）
pub fn list(saves_dir: &Path) -> Vec<WorldTarget> {
    let Ok(rd) = std::fs::read_dir(saves_dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in rd.flatten() {
        let path = e.path();
        if !path.is_dir() || !path.join("level.dat").is_file() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        out.push(WorldTarget {
            world_id: world_id(&name),
            display_name: name.clone(),
            dir: path.to_string_lossy().to_string(),
            last_modified: last_modified(&path),
            size_bytes: dir_size(&path),
            // 需要解析 NBT 才能拿到，未实现前不猜
            game_version: None,
        });
    }
    out.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));
    out
}

/// `worldId` → 真实目录；找不到返回 None（调用方据此报错，不回退）
pub fn resolve(saves_dir: &Path, id: &str) -> Option<PathBuf> {
    let rd = std::fs::read_dir(saves_dir).ok()?;
    for e in rd.flatten() {
        let path = e.path();
        if !path.is_dir() || !path.join("level.dat").is_file() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        if world_id(&name) == id {
            return Some(path);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_is_stable_and_distinct() {
        assert_eq!(world_id("World1"), world_id("World1"));
        assert_ne!(world_id("World1"), world_id("World2"));
        assert!(world_id("x").starts_with("w_"));
        assert_eq!(world_id("x").len(), 10);
    }
}
