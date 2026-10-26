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
        let info = crate::nbt::LevelInfo::read(&path);
        out.push(WorldTarget {
            world_id: world_id(&name),
            display_name: name.clone(),
            dir: path.to_string_lossy().to_string(),
            last_modified: last_modified(&path),
            size_bytes: dir_size(&path),
            // 从 level.dat 里读真实版本；读不到就是 null，不猜
            game_version: info.as_ref().and_then(|i| i.version_name.clone()),
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

/// 单个存档的细节（只读）。列表里只给概要，点开详情时才读这些。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WorldDetail {
    pub world_id: String,
    pub display_name: String,
    pub dir: String,
    pub last_modified: String,
    pub size_bytes: u64,
    /// `level.dat` 里自报的版本，读不到是 null
    pub game_version: Option<String>,
    pub level_name: Option<String>,
    pub version_id: Option<i32>,
    pub game_mode: Option<String>,
    pub difficulty: Option<String>,
    pub hardcore: Option<bool>,
    pub allow_commands: Option<bool>,
    pub last_played: Option<String>,
    pub data_version: Option<i32>,
    /// `level.dat` 存在但读不懂时的一句人话
    pub info_note: Option<String>,
    /// 区域文件数量（`region/*.mca`），大致对应探索过多少区块
    pub region_files: u32,
    /// `playerdata/` 里的玩家数据文件数
    pub player_files: u32,
    /// 是否有自定义图标
    pub has_icon: bool,
    /// 是否是 Bukkit 系服务器的存档（存在 `level.dat_old` 之类的服务端痕迹）
    pub looks_like_server: bool,
}

fn count_files(dir: &Path, ext: Option<&str>) -> u32 {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut n = 0u32;
    for e in rd.flatten() {
        if !e.path().is_file() {
            continue;
        }
        if let Some(ext) = ext {
            let ok = e
                .path()
                .extension()
                .map(|x| x.eq_ignore_ascii_case(ext))
                .unwrap_or(false);
            if !ok {
                continue;
            }
        }
        n = n.saturating_add(1);
    }
    n
}

/// 取某个 `worldId` 的详情。找不到存档直接报错，不返回半成品。
pub fn detail(saves_dir: &Path, id: &str) -> Result<WorldDetail, String> {
    let dir = resolve(saves_dir, id).ok_or_else(|| "WORLD_NOT_FOUND|存档不存在".to_string())?;
    let name = dir
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let (info, note) = match crate::nbt::LevelInfo::read(&dir) {
        Some(i) if i.looks_valid() => (Some(i), None),
        Some(_) => (None, Some("level.dat 里没有版本信息".to_string())),
        None => (None, Some("level.dat 读不出来（可能已损坏）".to_string())),
    };
    Ok(WorldDetail {
        world_id: world_id(&name),
        display_name: name,
        dir: dir.to_string_lossy().to_string(),
        last_modified: last_modified(&dir),
        size_bytes: dir_size(&dir),
        game_version: info.as_ref().and_then(|i| i.version_name.clone()),
        level_name: info.as_ref().and_then(|i| i.level_name.clone()),
        version_id: info.as_ref().and_then(|i| i.version_id),
        game_mode: info.as_ref().and_then(|i| i.game_mode.clone()),
        difficulty: info.as_ref().and_then(|i| i.difficulty.clone()),
        hardcore: info.as_ref().and_then(|i| i.hardcore),
        allow_commands: info.as_ref().and_then(|i| i.allow_commands),
        last_played: info.as_ref().and_then(|i| i.last_played.clone()),
        data_version: info.as_ref().and_then(|i| i.data_version),
        info_note: note,
        region_files: count_files(&dir.join("region"), Some("mca")),
        player_files: count_files(&dir.join("playerdata"), Some("dat")),
        has_icon: dir.join("icon.png").is_file(),
        looks_like_server: dir.join("session.lock").is_file()
            && dir.join("level.dat_old").is_file(),
    })
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

    fn tmp_world(tag: &str) -> (PathBuf, PathBuf) {
        let saves =
            std::env::temp_dir().join(format!("astral-worlds-{tag}-{}", crate::time::now_nanos()));
        let world = saves.join("测试世界");
        std::fs::create_dir_all(world.join("region")).unwrap();
        std::fs::create_dir_all(world.join("playerdata")).unwrap();
        std::fs::write(
            world.join("level.dat"),
            crate::nbt::sample_level_dat("1.21.1", 0, false),
        )
        .unwrap();
        std::fs::write(world.join("region/r.0.0.mca"), b"x").unwrap();
        std::fs::write(world.join("region/r.0.1.mca"), b"x").unwrap();
        std::fs::write(world.join("region/notes.txt"), b"x").unwrap();
        std::fs::write(world.join("playerdata/abc.dat"), b"x").unwrap();
        (saves, world)
    }

    #[test]
    fn detail_reads_level_info_and_counts() {
        let (saves, _world) = tmp_world("detail");
        let id = world_id("测试世界");
        let d = detail(&saves, &id).unwrap();
        assert_eq!(d.game_version.as_deref(), Some("1.21.1"));
        assert_eq!(d.game_mode.as_deref(), Some("生存"));
        assert_eq!(d.hardcore, Some(false));
        assert_eq!(d.region_files, 2, "只数 .mca");
        assert_eq!(d.player_files, 1);
        assert!(!d.has_icon);
        assert!(d.info_note.is_none());
        assert_eq!(d.world_id, id);
        let _ = std::fs::remove_dir_all(&saves);
    }

    #[test]
    fn detail_reports_broken_level_dat() {
        let (saves, world) = tmp_world("broken");
        std::fs::write(world.join("level.dat"), b"not nbt at all").unwrap();
        let d = detail(&saves, &world_id("测试世界")).unwrap();
        assert!(d.game_version.is_none());
        assert!(d.info_note.is_some());
        // 坏文件不影响其余信息
        assert_eq!(d.region_files, 2);
        let _ = std::fs::remove_dir_all(&saves);
    }

    #[test]
    fn detail_rejects_unknown_world() {
        let (saves, _world) = tmp_world("unknown");
        let err = detail(&saves, "w_00000000").unwrap_err();
        assert!(err.starts_with("WORLD_NOT_FOUND|"), "{err}");
        let _ = std::fs::remove_dir_all(&saves);
    }

    #[test]
    fn list_carries_version() {
        let (saves, _world) = tmp_world("listver");
        let ws = list(&saves);
        assert_eq!(ws.len(), 1);
        assert_eq!(ws[0].game_version.as_deref(), Some("1.21.1"));
        let _ = std::fs::remove_dir_all(&saves);
    }
}
