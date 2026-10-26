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
    /// Forge/NeoForge installer 产出的 version id
    #[serde(default)]
    pub launch_version: Option<String>,
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
    /// Forge/NeoForge 安装后生成的真实 version id（启动时用它）
    pub launch_version: Option<String>,
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

/// 健康检查：跟着 inheritsFrom 链看版本文件、依赖库、资源索引
///
/// 只报"能确定缺什么"，不做耗时校验（不逐个比对资源对象哈希）。
/// 按 Mojang 的 rules 判断某个库在当前平台是否适用
fn lib_allowed_for_platform(lib: &serde_json::Value) -> bool {
    let Some(rules) = lib.get("rules").and_then(|r| r.as_array()) else {
        return true;
    };
    if rules.is_empty() {
        return true;
    }
    let os = if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    };
    let mut allowed = false;
    for r in rules {
        let action = r.get("action").and_then(|a| a.as_str()).unwrap_or("");
        let os_match = match r.get("os") {
            None => true,
            Some(osv) => osv
                .get("name")
                .and_then(|n| n.as_str())
                .map(|n| n == os)
                .unwrap_or(true),
        };
        if os_match {
            allowed = action == "allow";
        }
    }
    allowed
}

fn health_of(game: &Path, version: &str) -> InstanceHealth {
    let mut reasons: Vec<String> = Vec::new();

    // 1) 沿着 inheritsFrom 收集整条链
    let mut chain: Vec<String> = Vec::new();
    let mut cur = version.to_string();
    for _ in 0..5 {
        if chain.contains(&cur) {
            break;
        }
        chain.push(cur.clone());
        let json_path = game.join("versions").join(&cur).join(format!("{cur}.json"));
        let Ok(text) = std::fs::read_to_string(&json_path) else {
            if cur == version {
                reasons.push(format!("缺少版本文件 {cur}.json"));
            }
            break;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
            reasons.push(format!("版本文件 {cur}.json 无法解析"));
            break;
        };
        match v.get("inheritsFrom").and_then(|x| x.as_str()) {
            Some(parent) => cur = parent.to_string(),
            None => break,
        }
    }

    // 2) 客户端 jar：整条链里任意一层有就行（Forge/NeoForge 继承原版）
    let has_jar = chain.iter().any(|v| {
        game.join("versions")
            .join(v)
            .join(format!("{v}.jar"))
            .exists()
    });
    if !has_jar {
        reasons.push(format!("缺少客户端 {version}.jar（含继承层）"));
    }

    // 3) 依赖库：把链上所有 json 的 artifact 路径并起来，缺的计数
    let mut missing_libs: Vec<String> = Vec::new();
    for v in &chain {
        let p = game.join("versions").join(v).join(format!("{v}.json"));
        let Ok(text) = std::fs::read_to_string(&p) else {
            continue;
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let Some(libs) = json.get("libraries").and_then(|l| l.as_array()) else {
            continue;
        };
        for lib in libs {
            if !lib_allowed_for_platform(lib) {
                continue;
            }
            let Some(path) = lib
                .get("downloads")
                .and_then(|d| d.get("artifact"))
                .and_then(|a| a.get("path"))
                .and_then(|p| p.as_str())
            else {
                continue;
            };
            if !game.join("libraries").join(path).exists() && missing_libs.len() < 3 {
                missing_libs.push(path.to_string());
            }
        }
    }
    let missing_count = if missing_libs.len() >= 3 {
        "至少 3".to_string()
    } else if missing_libs.is_empty() {
        "0".to_string()
    } else {
        missing_libs.len().to_string()
    };
    if !missing_libs.is_empty() {
        reasons.push(format!(
            "缺少 {missing_count} 个依赖库（如 {}）",
            missing_libs[0]
        ));
    }

    // 4) 资源索引：从链尾（根版本）取 assetIndex.id
    let mut asset_index: Option<String> = None;
    for v in chain.iter().rev() {
        let p = game.join("versions").join(v).join(format!("{v}.json"));
        let Ok(text) = std::fs::read_to_string(&p) else {
            continue;
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        if let Some(id) = json
            .get("assetIndex")
            .and_then(|a| a.get("id"))
            .and_then(|i| i.as_str())
        {
            asset_index = Some(id.to_string());
            break;
        }
    }
    if let Some(id) = asset_index {
        if !game
            .join("assets")
            .join("indexes")
            .join(format!("{id}.json"))
            .exists()
        {
            reasons.push(format!("缺少资源索引 {id}.json（首次启动会自动下载）"));
        }
    } else if !game.join("assets").exists() {
        reasons.push("缺少 assets 资源目录".to_string());
    }

    InstanceHealth {
        ok: reasons.is_empty(),
        reasons,
    }
}

/// 自检用的平台规则入口
#[allow(dead_code)]
pub fn lib_allowed_for_test(lib: &serde_json::Value) -> bool {
    lib_allowed_for_platform(lib)
}

/// 自检用的健康检查入口
pub fn health_for_test(game: &Path, version: &str) -> InstanceHealth {
    health_of(game, version)
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
        std::fs::create_dir_all(dir.join(sub))
            .map_err(|e| format!("PERMISSION|创建 {sub} 失败|{e}"))?;
    }
    let text =
        serde_json::to_string_pretty(meta).map_err(|e| format!("INTERNAL|序列化失败|{e}"))?;
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
    let text =
        serde_json::to_string_pretty(meta).map_err(|e| format!("INTERNAL|序列化失败|{e}"))?;
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
        launch_version: meta.launch_version.clone(),
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
                launch_version: None,
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

/// 从任意实例目录读元数据（回收站恢复时目录名还不是实例 id）
pub fn read_meta_at(dir: &Path) -> Option<InstanceMeta> {
    let text = std::fs::read_to_string(dir.join("instance.json")).ok()?;
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
            launch_version: None,
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
