//! 存档里的玩家统计：玩了多久、死了几次、挖了多少方块、走了多远、进度做到哪。
//!
//! 数据来自存档目录下游戏自己写的两份文件：
//!
//! - `stats/<uuid>.json` —— 原版统计（`minecraft:custom` 里是游戏时长、死亡数、
//!   行走距离等，`minecraft:mined` 是挖掉的方块，`minecraft:killed` 是击杀数）
//! - `advancements/<uuid>.json` —— 进度完成情况（`done: true` 的算完成）
//!
//! 这两份文件都是**只读**的：我们只解析，不改一个字节。文件坏了或者版本太老
//! 读不出来时返回空统计 + 一句说明，不让整页失败。
//!
//! 老版本（1.12 之前）的统计键名不一样（`stat.playOneMinute`、`stat.deaths` 之类），
//! 这里一并兼容，免得打开老存档显示全是 0。

use serde::Serialize;
use std::path::Path;

/// 统计文件夹名（在存档目录下）
pub const STATS_DIR: &str = "stats";
/// 进度文件夹名（在存档目录下）
pub const ADVANCEMENTS_DIR: &str = "advancements";
/// 单个统计文件最大读多少字节。正常就几十 KB，超过这个数肯定是坏文件。
const MAX_FILE: u64 = 4 * 1024 * 1024;

/// 一个玩家的统计
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlayerStats {
    /// 存档里的玩家 id（文件名，不带扩展名）
    pub uuid: String,
    /// 游戏时长（毫秒）
    pub play_time_ms: u64,
    pub deaths: u64,
    /// 击杀的生物数
    pub mob_kills: u64,
    /// 挖掉的方块数
    pub blocks_mined: u64,
    /// 走过的距离（厘米，游戏里就是这么记的）
    pub distance_cm: u64,
    pub jumps: u64,
    /// 完成的进度数（不含配方）
    pub advancements_done: u32,
    /// 这个存档里一共有多少条进度（不含配方）
    pub advancements_total: u32,
}

/// 整个存档的统计（多个玩家合并）
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WorldStats {
    pub world_id: String,
    /// 每个玩家一条，按游戏时长倒序
    pub players: Vec<PlayerStats>,
    pub play_time_ms: u64,
    pub deaths: u64,
    pub mob_kills: u64,
    pub blocks_mined: u64,
    pub distance_cm: u64,
    pub jumps: u64,
    pub advancements_done: u32,
    pub advancements_total: u32,
    /// 读不出来时的说明（文件不存在不算错，只是没数据）
    pub note: Option<String>,
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// 从 `minecraft:custom` 里取一个数；取不到返回 0。
fn custom(custom: &serde_json::Value, key: &str) -> u64 {
    custom.get(key).and_then(|v| v.as_u64()).unwrap_or(0)
}

/// 把 `minecraft:mined` / `minecraft:killed` 这类「按物品分项」的对象求和。
fn sum_map(v: Option<&serde_json::Value>) -> u64 {
    match v {
        Some(serde_json::Value::Object(m)) => m.values().filter_map(|x| x.as_u64()).sum(),
        _ => 0,
    }
}

/// 解析一份统计 JSON。新老格式都认。
pub fn parse_stats(uuid: &str, v: &serde_json::Value) -> PlayerStats {
    let stats = v.get("stats").unwrap_or(v);
    let modern = stats.get("minecraft:custom");

    // 游戏时长：新格式是 tick 数（20 tick = 1 秒），老格式 stat.playOneMinute 也是 tick
    let ticks = match modern {
        Some(c) => custom(c, "minecraft:play_time"),
        None => stats
            .get("stat.playOneMinute")
            .and_then(|x| x.as_u64())
            .unwrap_or(0),
    };

    let deaths = match modern {
        Some(c) => custom(c, "minecraft:deaths"),
        None => stats
            .get("stat.deaths")
            .and_then(|x| x.as_u64())
            .unwrap_or(0),
    };

    let jumps = match modern {
        Some(c) => custom(c, "minecraft:jump"),
        None => stats.get("stat.jump").and_then(|x| x.as_u64()).unwrap_or(0),
    };

    // 距离：新格式分好几种（走、跑、飞、游……），全加起来才算"走过多少路"
    let distance = match modern {
        Some(c) => [
            "minecraft:walk_one_cm",
            "minecraft:sprint_one_cm",
            "minecraft:fly_one_cm",
            "minecraft:swim_one_cm",
            "minecraft:aviate_one_cm",
            "minecraft:boat_one_cm",
            "minecraft:minecart_one_cm",
            "minecraft:horse_one_cm",
            "minecraft:climb_one_cm",
            "minecraft:fall_one_cm",
            "minecraft:crouch_one_cm",
            "minecraft:strider_one_cm",
            "minecraft:pig_one_cm",
        ]
        .iter()
        .map(|k| custom(c, k))
        .sum(),
        None => stats
            .get("stat.walkOneCm")
            .and_then(|x| x.as_u64())
            .unwrap_or(0),
    };

    let mob_kills = match modern {
        Some(_) => sum_map(stats.get("minecraft:killed")),
        None => stats
            .get("stat.mobKills")
            .and_then(|x| x.as_u64())
            .unwrap_or(0),
    };

    let blocks_mined = match modern {
        Some(_) => sum_map(stats.get("minecraft:mined")),
        None => sum_map(stats.get("stat.mineBlock")),
    };

    PlayerStats {
        uuid: uuid.to_string(),
        play_time_ms: ticks.saturating_mul(50),
        deaths,
        mob_kills,
        blocks_mined,
        distance_cm: distance,
        jumps,
        advancements_done: 0,
        advancements_total: 0,
    }
}

/// 数一份进度文件里「完成了几条 / 一共几条」。
///
/// 配方（`minecraft:recipes/...`）也是进度，但玩家不会觉得自己"做了 300 个进度"，
/// 所以统计时把它们排除掉。
pub fn count_advancements(v: &serde_json::Value) -> (u32, u32) {
    let Some(map) = v.as_object() else {
        return (0, 0);
    };
    let mut done = 0u32;
    let mut total = 0u32;
    for (key, item) in map {
        if key.starts_with("minecraft:recipes/") || key.starts_with("minecraft:recipes\\") {
            continue;
        }
        // 有些整合包会用别的命名空间，只要是 recipe 就跳过
        if key.contains(":recipes/") {
            continue;
        }
        total = total.saturating_add(1);
        if item.get("done").and_then(|d| d.as_bool()).unwrap_or(false) {
            done = done.saturating_add(1);
        }
    }
    (done, total)
}

/// 列出目录下所有 `.json` 文件的 `(主文件名, 完整路径)`，按名字排序保证稳定。
fn json_files(dir: &Path) -> Vec<(String, std::path::PathBuf)> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in rd.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().map(|s| s.to_string_lossy().to_string()) else {
            continue;
        };
        if !name.to_ascii_lowercase().ends_with(".json") {
            continue;
        }
        let stem = name[..name.len() - 5].to_string();
        out.push((stem, path));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// 统计一个存档。
///
/// `saves_dir` 是实例的 `saves/`，`world_id` 是 `worlds::world_id()` 算出来的 id。
pub fn world_stats(saves_dir: &Path, world_id: &str) -> Result<WorldStats, String> {
    let dir = crate::worlds::resolve(saves_dir, world_id)
        .ok_or_else(|| "WORLD_NOT_FOUND|存档不存在".to_string())?;

    let stats_dir = dir.join(STATS_DIR);
    let adv_dir = dir.join(ADVANCEMENTS_DIR);

    let mut players: Vec<PlayerStats> = Vec::new();
    let mut note: Option<String> = None;
    let mut broken = 0u32;

    let stats_files = json_files(&stats_dir);
    if stats_files.is_empty() && !stats_dir.is_dir() {
        note = Some("这个存档还没有统计文件（没进过游戏，或者是很老的版本）".to_string());
    }

    for (uuid, path) in &stats_files {
        match read_json(path) {
            Some(v) => {
                let mut p = parse_stats(uuid, &v);
                if let Some(adv) = read_json(&adv_dir.join(format!("{uuid}.json"))) {
                    let (done, total) = count_advancements(&adv);
                    p.advancements_done = done;
                    p.advancements_total = total;
                }
                players.push(p);
            }
            None => broken = broken.saturating_add(1),
        }
    }

    // 只有进度没有统计的情况：也算有数据，别显示成"没有统计"
    if players.is_empty() && !json_files(&adv_dir).is_empty() {
        for (uuid, path) in json_files(&adv_dir) {
            if let Some(v) = read_json(&path) {
                let (done, total) = count_advancements(&v);
                players.push(PlayerStats {
                    uuid,
                    play_time_ms: 0,
                    deaths: 0,
                    mob_kills: 0,
                    blocks_mined: 0,
                    distance_cm: 0,
                    jumps: 0,
                    advancements_done: done,
                    advancements_total: total,
                });
            }
        }
    }

    if broken > 0 && note.is_none() {
        note = Some(format!("有 {broken} 份统计文件读不出来（可能已损坏）"));
    }

    players.sort_by(|a, b| {
        b.play_time_ms
            .cmp(&a.play_time_ms)
            .then_with(|| a.uuid.cmp(&b.uuid))
    });

    let mut out = WorldStats {
        world_id: world_id.to_string(),
        players,
        play_time_ms: 0,
        deaths: 0,
        mob_kills: 0,
        blocks_mined: 0,
        distance_cm: 0,
        jumps: 0,
        advancements_done: 0,
        advancements_total: 0,
        note,
    };
    for p in &out.players {
        out.play_time_ms = out.play_time_ms.saturating_add(p.play_time_ms);
        out.deaths = out.deaths.saturating_add(p.deaths);
        out.mob_kills = out.mob_kills.saturating_add(p.mob_kills);
        out.blocks_mined = out.blocks_mined.saturating_add(p.blocks_mined);
        out.distance_cm = out.distance_cm.saturating_add(p.distance_cm);
        out.jumps = out.jumps.saturating_add(p.jumps);
        out.advancements_done = out.advancements_done.saturating_add(p.advancements_done);
        out.advancements_total = out.advancements_total.saturating_add(p.advancements_total);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d =
            std::env::temp_dir().join(format!("astral-wstats-{tag}-{}", crate::time::now_nanos()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 造一个能被 `worlds::resolve` 认出来的存档目录（它要求有 `level.dat`）。
    fn make_world(saves: &Path, name: &str) -> std::path::PathBuf {
        let dir = saves.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("level.dat"),
            crate::nbt::sample_level_dat("1.21.1", 0, false),
        )
        .unwrap();
        dir
    }

    fn write_stats(dir: &Path, uuid: &str, body: &str) {
        std::fs::create_dir_all(dir.join(STATS_DIR)).unwrap();
        std::fs::write(dir.join(STATS_DIR).join(format!("{uuid}.json")), body).unwrap();
    }

    fn write_adv(dir: &Path, uuid: &str, body: &str) {
        std::fs::create_dir_all(dir.join(ADVANCEMENTS_DIR)).unwrap();
        std::fs::write(
            dir.join(ADVANCEMENTS_DIR).join(format!("{uuid}.json")),
            body,
        )
        .unwrap();
    }

    #[test]
    fn parses_modern_stats() {
        let body = r#"{"stats":{"minecraft:custom":{
            "minecraft:play_time":12000,
            "minecraft:deaths":3,
            "minecraft:jump":40,
            "minecraft:walk_one_cm":1000,
            "minecraft:sprint_one_cm":500
        },"minecraft:mined":{"minecraft:stone":7,"minecraft:dirt":3},
          "minecraft:killed":{"minecraft:zombie":2}}}"#;
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        let p = parse_stats("abc", &v);
        assert_eq!(p.play_time_ms, 600_000, "12000 tick = 600 秒");
        assert_eq!(p.deaths, 3);
        assert_eq!(p.jumps, 40);
        assert_eq!(p.distance_cm, 1500);
        assert_eq!(p.blocks_mined, 10);
        assert_eq!(p.mob_kills, 2);
    }

    #[test]
    fn parses_legacy_stats() {
        let body = r#"{"stats":{
            "stat.playOneMinute":2400,
            "stat.deaths":1,
            "stat.jump":5,
            "stat.walkOneCm":800,
            "stat.mobKills":9,
            "stat.mineBlock":{"minecraft:stone":4}
        }}"#;
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        let p = parse_stats("old", &v);
        assert_eq!(p.play_time_ms, 120_000);
        assert_eq!(p.deaths, 1);
        assert_eq!(p.distance_cm, 800);
        assert_eq!(p.mob_kills, 9);
        assert_eq!(p.blocks_mined, 4);
    }

    #[test]
    fn counts_advancements_without_recipes() {
        let body = r#"{
            "minecraft:story/root":{"done":true},
            "minecraft:story/mine_stone":{"done":true},
            "minecraft:story/smelt_iron":{"done":false},
            "minecraft:recipes/building_blocks/oak_planks":{"done":true},
            "minecraft:recipes/decorations/crafting_table":{"done":true}
        }"#;
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(count_advancements(&v), (2, 3));
    }

    #[test]
    fn aggregates_players_and_sorts() {
        let saves = tmp("agg");
        let world = make_world(&saves, "World");
        write_stats(
            &world,
            "aaa",
            r#"{"stats":{"minecraft:custom":{"minecraft:play_time":200,"minecraft:deaths":1}}}"#,
        );
        write_stats(
            &world,
            "bbb",
            r#"{"stats":{"minecraft:custom":{"minecraft:play_time":1000,"minecraft:deaths":4}}}"#,
        );
        write_adv(
            &world,
            "bbb",
            r#"{"minecraft:story/root":{"done":true},"minecraft:story/x":{"done":false}}"#,
        );
        let id = crate::worlds::world_id("World");
        let s = world_stats(&saves, &id).unwrap();
        assert_eq!(s.players.len(), 2);
        assert_eq!(s.players[0].uuid, "bbb", "玩得久的排前面");
        assert_eq!(s.play_time_ms, 60_000);
        assert_eq!(s.deaths, 5);
        assert_eq!(s.players[0].advancements_done, 1);
        assert_eq!(s.players[0].advancements_total, 2);
        assert!(s.note.is_none());
        let _ = std::fs::remove_dir_all(&saves);
    }

    #[test]
    fn missing_stats_is_not_an_error() {
        let saves = tmp("none");
        let _world = make_world(&saves, "Empty");
        let id = crate::worlds::world_id("Empty");
        let s = world_stats(&saves, &id).unwrap();
        assert!(s.players.is_empty());
        assert_eq!(s.play_time_ms, 0);
        assert!(s.note.is_some(), "要告诉玩家为什么是空的");
        let _ = std::fs::remove_dir_all(&saves);
    }

    #[test]
    fn broken_file_is_reported_not_fatal() {
        let saves = tmp("broken");
        let world = make_world(&saves, "Bad");
        write_stats(&world, "ccc", "this is not json");
        let id = crate::worlds::world_id("Bad");
        let s = world_stats(&saves, &id).unwrap();
        assert!(s.players.is_empty());
        assert!(s.note.unwrap().contains("读不出来"));
        let _ = std::fs::remove_dir_all(&saves);
    }

    #[test]
    fn unknown_world_is_an_error() {
        let saves = tmp("unknown");
        let err = world_stats(&saves, "w_00000000").unwrap_err();
        assert!(err.starts_with("WORLD_NOT_FOUND|"), "{err}");
        let _ = std::fs::remove_dir_all(&saves);
    }
}
