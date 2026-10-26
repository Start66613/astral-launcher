//! 存档与统计：首页真实数据（契约 v3 §4.7 / §4.8）。
//!
//! 只读真实文件，读不到就给 null，不编造数字。

use crate::instance::{self, Instance};
use serde::Serialize;
use std::path::Path;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SaveEntry {
    pub name: String,
    pub dir: String,
    /// 存档自报的版本（`level.dat` 里的 `Data.Version.Name`），读不到才是 null
    pub game_version: Option<String>,
    pub last_modified: String,
    pub size_bytes: u64,
    pub icon_path: Option<String>,
    /// `level.dat` 里写的名字，可能和目录名不同
    pub level_name: Option<String>,
    pub version_id: Option<i32>,
    pub game_mode: Option<String>,
    pub difficulty: Option<String>,
    pub hardcore: Option<bool>,
    pub allow_commands: Option<bool>,
    /// 上次游玩时间：优先用存档自报的 `LastPlayed`（复制/解压不会改它），
    /// 读不到才退回文件 mtime
    pub last_played: Option<String>,
    /// `level.dat` 存在但读不懂时给一句人话，列表照常返回
    pub info_note: Option<String>,
}

fn dir_size(p: &Path) -> u64 {
    let mut total = 0u64;
    let Ok(rd) = std::fs::read_dir(p) else {
        return 0;
    };
    for e in rd.flatten() {
        let path = e.path();
        if path.is_dir() {
            total += dir_size(&path);
        } else if let Ok(m) = e.metadata() {
            total += m.len();
        }
    }
    total
}

fn iso(mtime: std::time::SystemTime) -> String {
    crate::time::system_time_to_iso(mtime)
}

/// 存档列表：给定实例就读它的 saves/，否则读全局 saves/
pub fn list_saves(game: &Path, inst: Option<&Instance>) -> Vec<SaveEntry> {
    let root = match inst {
        Some(i) => Path::new(&i.dirs.saves).to_path_buf(),
        None => game.join("saves"),
    };
    let Ok(rd) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in rd.flatten() {
        let p = e.path();
        if !p.is_dir() || !p.join("level.dat").exists() {
            continue;
        }
        let meta = e.metadata().ok();
        let mtime = meta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .unwrap_or(std::time::UNIX_EPOCH);
        let icon = p.join("icon.png");
        let (info, note) = match crate::nbt::LevelInfo::read(&p) {
            Some(i) if i.looks_valid() => (Some(i), None),
            Some(_) => (None, Some("level.dat 里没有版本信息".to_string())),
            None => (None, Some("level.dat 读不出来（可能已损坏）".to_string())),
        };
        out.push(SaveEntry {
            name: e.file_name().to_string_lossy().to_string(),
            dir: p.to_string_lossy().to_string(),
            game_version: info.as_ref().and_then(|i| i.version_name.clone()),
            last_modified: iso(mtime),
            size_bytes: dir_size(&p),
            icon_path: icon.exists().then(|| icon.to_string_lossy().to_string()),
            level_name: info.as_ref().and_then(|i| i.level_name.clone()),
            version_id: info.as_ref().and_then(|i| i.version_id),
            game_mode: info.as_ref().and_then(|i| i.game_mode.clone()),
            difficulty: info.as_ref().and_then(|i| i.difficulty.clone()),
            hardcore: info.as_ref().and_then(|i| i.hardcore),
            allow_commands: info.as_ref().and_then(|i| i.allow_commands),
            last_played: info.as_ref().and_then(|i| i.last_played.clone()),
            info_note: note,
        });
    }
    out.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));
    out
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub range: String,
    pub playtime_ms: u64,
    pub sessions: u32,
    pub deaths: Option<u32>,
    pub achievements: Option<u32>,
    pub crashes: u32,
    pub by_instance: Vec<InstanceStat>,
    pub updated_at: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InstanceStat {
    pub instance_id: String,
    pub name: String,
    pub playtime_ms: u64,
}

pub fn stats(range: &str, name_of: &dyn Fn(&str) -> String) -> Stats {
    let cutoff = match range {
        "7d" => Some(crate::time::now_unix_ms() - 7 * 86_400_000),
        "30d" => Some(crate::time::now_unix_ms() - 30 * 86_400_000),
        _ => None,
    };

    let sessions = crate::session::list(None, 500);
    let mut playtime = 0u64;
    let mut count = 0u32;
    let mut crashes = 0u32;
    let mut per: std::collections::HashMap<String, u64> = std::collections::HashMap::new();

    for s in sessions.iter().filter(|s| s.state == "exited") {
        if let Some(c) = cutoff {
            let started = crate::time::iso_to_unix_ms(&s.started_at).unwrap_or(0);
            if started < c {
                continue;
            }
        }
        let ms = s.duration_ms.unwrap_or(0);
        playtime += ms;
        count += 1;
        if s.exit_code.map(|c| c != 0).unwrap_or(true) {
            crashes += 1;
        }
        *per.entry(s.instance_id.clone()).or_insert(0) += ms;
    }

    let mut by_instance: Vec<InstanceStat> = per
        .into_iter()
        .map(|(id, ms)| InstanceStat {
            name: name_of(&id),
            instance_id: id,
            playtime_ms: ms,
        })
        .collect();
    by_instance.sort_by_key(|b| std::cmp::Reverse(b.playtime_ms));

    Stats {
        range: range.to_string(),
        playtime_ms: playtime,
        sessions: count,
        deaths: None,
        achievements: None,
        crashes,
        by_instance,
        updated_at: crate::task::now_iso(),
    }
}

/// 给实例模块用的统计回调
pub fn play_stats(instance_id: &str) -> instance::PlayStats {
    crate::session::stats_for(instance_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_saves_is_empty_vec() {
        let g = std::env::temp_dir().join(format!("astral-saves-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&g);
        std::fs::create_dir_all(&g).unwrap();
        assert!(list_saves(&g, None).is_empty());
        let _ = std::fs::remove_dir_all(&g);
    }

    #[test]
    fn save_with_level_dat_is_listed() {
        let g = std::env::temp_dir().join(format!("astral-saves2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&g);
        let s = g.join("saves").join("World1");
        std::fs::create_dir_all(&s).unwrap();
        std::fs::write(s.join("level.dat"), "x").unwrap();
        let list = list_saves(&g, None);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "World1");
        let _ = std::fs::remove_dir_all(&g);
    }

    #[test]
    fn stats_defaults_are_honest() {
        let s = stats("7d", &|id| id.to_string());
        assert_eq!(s.deaths, None);
        assert_eq!(s.achievements, None);
        assert_eq!(s.range, "7d");
    }
}
