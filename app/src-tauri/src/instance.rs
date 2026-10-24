//! 实例模型与目录管理（契约 v3 §4.5 / §7）。
//!
//! 两类实例：
//! - **托管实例**：`<gameDir>/instances/<id>/`，带 `instance.json`，数据隔离；
//! - **兼容实例**：`<gameDir>/versions/<ver>/` 里已经装好的版本，只读呈现，
//!   让老用户不用重新建实例就能看到。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const SUB_DIRS: [&str; 7] = [
    "mods",
    "config",
    "saves",
    "logs",
    "resourcepacks",
    "shaderpacks",
    "screenshots",
];

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InstanceMeta {
    pub id: String,
    pub name: String,
    pub game_version: String,
    #[serde(default)]
    pub loader_kind: Option<String>,
    #[serde(default)]
    pub loader_version: Option<String>,
    #[serde(default)]
    pub memory_mb: Option<u32>,
    #[serde(default)]
    pub jvm_args: Vec<String>,
    #[serde(default)]
    pub java_path: Option<String>,
    #[serde(default)]
    pub cover: Option<String>,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LoaderRef {
    pub kind: String,
    pub version: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InstanceDirs {
    pub mods: String,
    pub config: String,
    pub saves: String,
    pub logs: String,
    pub resourcepacks: String,
    pub shaderpacks: String,
    pub screenshots: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InstanceHealth {
    pub ok: bool,
    pub reasons: Vec<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    pub id: String,
    pub name: String,
    pub game_version: String,
    pub loader: Option<LoaderRef>,
    pub dir: String,
    pub dirs: InstanceDirs,
    pub mods_count: u32,
    pub mods_disabled: u32,
    pub cover: Option<String>,
    pub health: InstanceHealth,
    pub last_played: Option<String>,
    pub total_playtime_ms: u64,
    pub memory_mb: Option<u32>,
    pub jvm_args: Vec<String>,
    pub java_path: Option<String>,
    pub created_at: String,
    /// true = instances/ 下的托管实例；false = 从 versions/ 兼容出来的
    pub managed: bool,
}

/// 每个实例的统计（由 stats 模块提供，避免循环依赖）
#[derive(Default, Clone)]
pub struct PlayStats {
    pub last_played: Option<String>,
    pub total_playtime_ms: u64,
}

pub fn instances_root(game: &Path) -> PathBuf {
    game.join("instances")
}

pub fn instance_dir(game: &Path, id: &str) -> PathBuf {
    instances_root(game).join(id)
}

fn dirs_for(root: &Path) -> InstanceDirs {
    let j = |s: &str| root.join(s).to_string_lossy().to_string();
    InstanceDirs {
        mods: j("mods"),
        config: j("config"),
        saves: j("saves"),
        logs: j("logs"),
        resourcepacks: j("resourcepacks"),
        shaderpacks: j("shaderpacks"),
        screenshots: j("screenshots"),
    }
}

fn count_mods(mods: &Path) -> (u32, u32) {
    let Ok(rd) = std::fs::read_dir(mods) else {
        return (0, 0);
    };
    let mut on = 0u32;
    let mut off = 0u32;
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_lowercase();
        if name.ends_with(".jar") {
            on += 1;
        } else if name.ends_with(".jar.disabled") {
            off += 1;
        }
    }
    (on, off)
}

fn health_of(game: &Path, version: &str) -> InstanceHealth {
    let vdir = game.join("versions").join(version);
    let json = vdir.join(format!("{version}.json"));
    let jar = vdir.join(format!("{version}.jar"));
    let mut reasons = Vec::new();
    if !json.exists() {
        reasons.push(format!("缺少版本文件 {version}.json"));
    }
    if !jar.exists() {
        reasons.push(format!("缺少客户端 {version}.jar"));
    }
    if !game.join("assets").exists() {
        reasons.push("缺少 assets 资源目录".to_string());
    }
    InstanceHealth {
        ok: reasons.is_empty(),
        reasons,
    }
}

/// 新建实例：建目录 + 写 instance.json
pub fn create(game: &Path, meta: &InstanceMeta) -> Result<Instance, String> {
    let dir = instance_dir(game, &meta.id);
    // 同 id 已存在 → 拒绝覆盖，不静默重建
    if dir.join("instance.json").exists() {
        return Err("INSTANCE_EXISTS|该实例已存在，拒绝覆盖".into());
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("PERMISSION|创建实例目录失败|{e}"))?;
    for sub in SUB_DIRS {
        std::fs::create_dir_all(dir.join(sub)).map_err(|e| format!("PERMISSION|创建 {sub} 失败|{e}"))?;
    }
    let text = serde_json::to_string_pretty(meta).map_err(|e| format!("INTERNAL|序列化失败|{e}"))?;
    if let Err(e) = write_atomic(&dir.join("instance.json"), &text) {
        // 失败不留半成品目录
        let _ = std::fs::remove_dir_all(&dir);
        return Err(e);
    }
    Ok(build(game, meta.clone(), PlayStats::default(), true))
}

/// 先写 `.part` 再替换，避免半截 JSON
fn write_atomic(path: &std::path::Path, text: &str) -> Result<(), String> {
    let tmp = path.with_extension("json.part");
    std::fs::write(&tmp, text).map_err(|e| format!("PERMISSION|写入临时文件失败|{e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("PERMISSION|替换 instance.json 失败|{e}")
    })
}

pub fn save_meta(game: &Path, meta: &InstanceMeta) -> Result<(), String> {
    let dir = instance_dir(game, &meta.id);
    let text = serde_json::to_string_pretty(meta).map_err(|e| format!("INTERNAL|序列化失败|{e}"))?;
    write_atomic(&dir.join("instance.json"), &text)
}

fn build(game: &Path, meta: InstanceMeta, stats: PlayStats, managed: bool) -> Instance {
    let root = if managed {
        instance_dir(game, &meta.id)
    } else {
        game.to_path_buf()
    };
    let dirs = dirs_for(&root);
    let (mods_count, mods_disabled) = count_mods(Path::new(&dirs.mods));
    Instance {
        id: meta.id.clone(),
        name: meta.name.clone(),
        game_version: meta.game_version.clone(),
        loader: match (&meta.loader_kind, &meta.loader_version) {
            (Some(k), Some(v)) => Some(LoaderRef {
                kind: k.clone(),
                version: v.clone(),
            }),
            _ => None,
        },
        dir: root.to_string_lossy().to_string(),
        dirs,
        mods_count,
        mods_disabled,
        cover: meta.cover.clone(),
        health: health_of(game, &meta.game_version),
        last_played: stats.last_played,
        total_playtime_ms: stats.total_playtime_ms,
        memory_mb: meta.memory_mb,
        jvm_args: meta.jvm_args.clone(),
        java_path: meta.java_path.clone(),
        created_at: meta.created_at.clone(),
        managed,
    }
}

/// 扫描全部实例：托管 + 兼容
pub fn discover(game: &Path, stats: &dyn Fn(&str) -> PlayStats) -> Vec<Instance> {
    let mut out = Vec::new();

    if let Ok(rd) = std::fs::read_dir(instances_root(game)) {
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_dir() {
                continue;
            }
            if p.file_name().map(|n| n == ".trash").unwrap_or(false) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(p.join("instance.json")) else {
                continue;
            };
            let Ok(meta) = serde_json::from_str::<InstanceMeta>(&text) else {
                continue;
            };
            out.push(build(game, meta.clone(), stats(&meta.id), true));
        }
    }

    if let Ok(rd) = std::fs::read_dir(game.join("versions")) {
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_dir() {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            if !p.join(format!("{name}.json")).exists() {
                continue;
            }
            let id = format!("ver_{name}");
            let meta = InstanceMeta {
                id: id.clone(),
                name: name.clone(),
                game_version: name.clone(),
                loader_kind: None,
                loader_version: None,
                memory_mb: None,
                jvm_args: vec![],
                java_path: None,
                cover: None,
                created_at: String::new(),
            };
            out.push(build(game, meta, stats(&id), false));
        }
    }

    out.sort_by(|a, b| {
        b.last_played
            .cmp(&a.last_played)
            .then_with(|| a.name.cmp(&b.name))
    });
    out
}

pub fn find(game: &Path, id: &str, stats: &dyn Fn(&str) -> PlayStats) -> Option<Instance> {
    discover(game, stats).into_iter().find(|i| i.id == id)
}

pub fn read_meta(game: &Path, id: &str) -> Option<InstanceMeta> {
    let text = std::fs::read_to_string(instance_dir(game, id).join("instance.json")).ok()?;
    serde_json::from_str(&text).ok()
}

/// 删除：默认进回收站
pub fn delete(game: &Path, id: &str, trash: bool) -> Result<(), String> {
    let dir = instance_dir(game, id);
    if !dir.exists() {
        return Err("INSTANCE_NOT_FOUND|实例不存在".into());
    }
    if trash {
        let trash_dir = instances_root(game).join(".trash");
        std::fs::create_dir_all(&trash_dir)
            .map_err(|e| format!("PERMISSION|创建回收站失败|{e}"))?;
        let stamp = crate::time::now_stamp();
        let dest = trash_dir.join(format!("{id}-{stamp}"));
        std::fs::rename(&dir, &dest).map_err(|e| format!("PERMISSION|移入回收站失败|{e}"))?;
        Ok(())
    } else {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("PERMISSION|删除实例失败|{e}"))
    }
}

pub fn new_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("inst_{:08x}", (nanos as u64) & 0xffff_ffff)
}

pub fn validate_name(name: &str) -> Result<(), String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("INVALID_NAME|实例名不能为空".into());
    }
    if n.chars().count() > 60 {
        return Err("INVALID_NAME|实例名不能超过 60 个字符".into());
    }
    if n.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) {
        return Err("INVALID_NAME|实例名不能包含路径或非法字符".into());
    }
    if n == "." || n == ".." {
        return Err("INVALID_NAME|实例名非法".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_game(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("astral-inst-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(p.join("versions").join("1.20.1")).unwrap();
        std::fs::write(p.join("versions/1.20.1/1.20.1.json"), "{}").unwrap();
        std::fs::write(p.join("versions/1.20.1/1.20.1.jar"), "x").unwrap();
        std::fs::create_dir_all(p.join("assets")).unwrap();
        p
    }

    fn meta(id: &str) -> InstanceMeta {
        InstanceMeta {
            id: id.into(),
            name: "测试实例".into(),
            game_version: "1.20.1".into(),
            loader_kind: None,
            loader_version: None,
            memory_mb: None,
            jvm_args: vec![],
            java_path: None,
            cover: None,
            created_at: "2026-10-23T00:00:00Z".into(),
        }
    }

    #[test]
    fn create_then_discover() {
        let g = tmp_game("create");
        let m = meta("inst_00000001");
        create(&g, &m).unwrap();
        assert!(instance_dir(&g, &m.id).join("mods").exists());

        let list = discover(&g, &|_| PlayStats::default());
        let mine = list.iter().find(|i| i.id == m.id).expect("应能扫到");
        assert!(mine.managed);
        assert!(mine.health.ok, "{:?}", mine.health.reasons);
        // 兼容实例也在
        assert!(list.iter().any(|i| i.id == "ver_1.20.1" && !i.managed));
        let _ = std::fs::remove_dir_all(&g);
    }

    #[test]
    fn healthy_reports_reason_when_jar_missing() {
        let g = tmp_game("health");
        std::fs::remove_file(g.join("versions/1.20.1/1.20.1.jar")).unwrap();
        let h = health_of(&g, "1.20.1");
        assert!(!h.ok);
        assert!(h.reasons.iter().any(|r| r.contains("1.20.1.jar")));
        let _ = std::fs::remove_dir_all(&g);
    }

    #[test]
    fn delete_moves_to_trash() {
        let g = tmp_game("trash");
        let m = meta("inst_00000002");
        create(&g, &m).unwrap();
        delete(&g, &m.id, true).unwrap();
        assert!(!instance_dir(&g, &m.id).exists());
        let trash = instances_root(&g).join(".trash");
        assert_eq!(std::fs::read_dir(&trash).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&g);
    }

    #[test]
    fn name_validation() {
        assert!(validate_name("").is_err());
        assert!(validate_name("a/b").is_err());
        assert!(validate_name("..").is_err());
        assert!(validate_name("我的生存").is_ok());
    }

    #[test]
    fn mods_counted_by_suffix() {
        let g = tmp_game("mods");
        let m = meta("inst_00000003");
        create(&g, &m).unwrap();
        let mods = instance_dir(&g, &m.id).join("mods");
        std::fs::write(mods.join("a.jar"), "x").unwrap();
        std::fs::write(mods.join("b.jar.disabled"), "x").unwrap();
        let list = discover(&g, &|_| PlayStats::default());
        let mine = list.iter().find(|i| i.id == m.id).unwrap();
        assert_eq!((mine.mods_count, mine.mods_disabled), (1, 1));
        let _ = std::fs::remove_dir_all(&g);
    }
}
