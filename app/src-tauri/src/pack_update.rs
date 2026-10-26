//! 整合包更新检查与更新计划（契约 V2 §8 增强）。
//!
//! 只做"读"：查作者有没有发新版本、新旧版本之间会增删哪些文件、需要多少空间。
//! 真正的替换留给后续 apply，而且必须先给玩家看这份计划。
//!
//! 安全要点：
//! - 本地导入的包（没有 Modrinth 项目 id）明确说"查不了"，不瞎猜；
//! - 只认 Modrinth CDN 的下载地址（走 `net` 白名单）；
//! - 游戏版本或加载器变了直接判为阻塞：那不是"更新"，是换一个实例；
//! - 计划里明说不会动存档。

use serde::Serialize;
use std::path::{Path, PathBuf};

const API: &str = "https://api.modrinth.com/v2";

/// 整合包文件大小上限（正常 .mrpack 远小于此）
pub const MAX_PACK: u64 = 512 * 1024 * 1024;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PackUpdateCheck {
    pub instance_id: String,
    pub pack_name: String,
    pub project_id: String,
    pub installed_version_id: String,
    pub installed_version_name: String,
    pub latest_version_id: String,
    pub latest_version_name: String,
    pub latest_published: Option<String>,
    pub latest_changelog: Option<String>,
    pub update_available: bool,
    /// 能不能自动查（本地导入的包为 false）
    pub can_auto_check: bool,
    pub notes: Vec<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PackChange {
    pub path: String,
    /// `mod` / `config` / `other`
    pub kind: String,
    pub old_size: u64,
    pub new_size: u64,
    pub url: String,
}

impl PackChange {
    fn kind_of(path: &str) -> &'static str {
        if path.starts_with("mods/") {
            "mod"
        } else if path.starts_with("config/") || path.starts_with("defaultconfigs/") {
            "config"
        } else {
            "other"
        }
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PackUpdatePlan {
    pub plan_id: String,
    pub instance_id: String,
    pub pack_name: String,
    pub from_version_id: String,
    pub from_version_name: String,
    pub to_version_id: String,
    pub to_version_name: String,
    /// 新版本要加的文件
    pub added: Vec<PackChange>,
    /// 新版本里没有、要从实例删掉的文件
    pub removed: Vec<PackChange>,
    /// 两边都有但大小不同的文件
    pub changed: Vec<PackChange>,
    /// 两边一样、不用动的文件数
    pub unchanged: u32,
    pub download_bytes: u64,
    pub disk_needed: u64,
    pub disk_free: Option<u64>,
    pub mc_version_from: String,
    pub mc_version_to: String,
    pub loader_from: Option<String>,
    pub loader_to: Option<String>,
    /// 更新过程不会碰存档（固定 true，写出来是为了让玩家看得见）
    pub keeps_worlds: bool,
    pub touches_saves: bool,
    pub issues: Vec<crate::plan::PlanIssue>,
    pub blocking: bool,
}

fn issue(code: &str, message: impl Into<String>, blocking: bool) -> crate::plan::PlanIssue {
    crate::plan::PlanIssue {
        code: code.to_string(),
        message: message.into(),
        blocking,
    }
}

fn str_of(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push('…');
    out
}

/// 拉 JSON：线上走白名单，自检时（base 不是官方 API）允许本地服务器
fn fetch(base: &str, url: &str) -> Result<serde_json::Value, String> {
    if base == API {
        crate::net::fetch_json_cached(url, None)
    } else {
        crate::net::fetch_json_cached_for_test(url, None)
    }
}

/// 下载文件：同上
fn download(base: &str, url: &str, dest: &Path) -> Result<u64, String> {
    if base == API {
        crate::net::validate_url(url)?;
        crate::net::download_atomic(url, dest, None, |_, _, _, _| {})
    } else {
        crate::net::download_verified(url, dest, None, None, |_, _, _, _| {})
    }
}

/// 查一个实例的整合包有没有新版本
pub fn check(game: &Path, instance_id: &str) -> Result<PackUpdateCheck, String> {
    check_at(API, game, instance_id)
}

/// 自检用的版本：`base` 换成本地测试服务器
#[doc(hidden)]
pub fn check_at(base: &str, game: &Path, instance_id: &str) -> Result<PackUpdateCheck, String> {
    let dir = crate::instance::instance_dir(game, instance_id);
    if !dir.is_dir() {
        return Err("INSTANCE_NOT_FOUND|实例不存在".into());
    }
    let meta = crate::instance::read_meta(game, instance_id)
        .ok_or_else(|| "INSTANCE_NOT_FOUND|实例元数据丢失".to_string())?;
    let Some(prov) = crate::modpack::read_provenance(&dir) else {
        return Err("INVALID_VALUE|这个实例不是从整合包装的，没有来源信息，没法查更新".into());
    };

    let mut notes = Vec::new();
    let mut out = PackUpdateCheck {
        instance_id: instance_id.to_string(),
        pack_name: if prov.pack_name.is_empty() {
            meta.name.clone()
        } else {
            prov.pack_name.clone()
        },
        project_id: prov.project_id.clone(),
        installed_version_id: prov.version_id.clone(),
        installed_version_name: prov.version_name.clone(),
        latest_version_id: String::new(),
        latest_version_name: String::new(),
        latest_published: None,
        latest_changelog: None,
        update_available: false,
        can_auto_check: true,
        notes: Vec::new(),
    };

    // 本地导入的包没有项目 id，查不了也不假装查得到
    if prov.source != "modrinth" || prov.project_id.trim().is_empty() {
        out.can_auto_check = false;
        notes.push(
            "这个整合包是本地导入的，没有 Modrinth 来源信息，查不到作者有没有发新版本".into(),
        );
        out.notes = notes;
        return Ok(out);
    }

    let mut url = format!("{base}/project/{}/version", prov.project_id);
    let mut query: Vec<String> = Vec::new();
    query.push(format!(
        "game_versions=%5B%22{}%22%5D",
        meta.game_version.replace('"', "%22")
    ));
    if let Some(kind) = &meta.loader_kind {
        query.push(format!("loaders=%5B%22{kind}%22%5D"));
    }
    url.push('?');
    url.push_str(&query.join("&"));

    let list = fetch(base, &url)?;
    let Some(arr) = list.as_array() else {
        return Err("INTERNAL|版本列表格式异常".into());
    };
    let Some(latest) = arr.first() else {
        notes.push(format!(
            "作者没有发布兼容 {} 的版本，所以查不到更新",
            meta.game_version
        ));
        out.notes = notes;
        return Ok(out);
    };

    out.latest_version_id = str_of(latest, "id").unwrap_or_default();
    out.latest_version_name = str_of(latest, "version_number")
        .or_else(|| str_of(latest, "name"))
        .unwrap_or_default();
    out.latest_published = str_of(latest, "date_published");
    out.latest_changelog = str_of(latest, "changelog").map(|c| truncate(&c, 4000));
    out.update_available =
        !out.latest_version_id.is_empty() && out.latest_version_id != out.installed_version_id;

    if !out.update_available {
        notes.push("已经是最新版本".into());
    } else if out.latest_version_id.is_empty() {
        notes.push("作者的最新版本没有 id，无法确认".into());
    }
    out.notes = notes;
    Ok(out)
}

/// 把一个具体版本的 .mrpack 拿到本地缓存并解析
fn fetch_index(
    base: &str,
    game: &Path,
    version_id: &str,
) -> Result<crate::modpack::ModpackIndex, String> {
    let cache = game.join(".astral").join("modpacks");
    std::fs::create_dir_all(&cache).map_err(|e| format!("PERMISSION|创建缓存目录失败|{e}"))?;

    let ver = fetch(base, &format!("{base}/version/{version_id}"))?;
    let file = ver
        .get("files")
        .and_then(|f| f.as_array())
        .and_then(|a| a.first())
        .ok_or_else(|| format!("VERSION_NOT_FOUND|版本 {version_id} 没有文件"))?;
    let url = file
        .get("url")
        .and_then(|u| u.as_str())
        .ok_or_else(|| "INTERNAL|缺少下载地址".to_string())?;
    let raw_name = file
        .get("filename")
        .and_then(|f| f.as_str())
        .unwrap_or("pack.mrpack");
    // 只取文件名，不让服务端用 ../ 决定落盘位置
    let name = Path::new(raw_name)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .filter(|n| n.ends_with(".mrpack"))
        .unwrap_or_else(|| format!("{version_id}.mrpack"));
    let path = cache.join(&name);

    if !path.is_file() {
        download(base, url, &path)?;
    }
    if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > MAX_PACK {
        return Err("INVALID_VALUE|整合包文件异常大，已拒绝".into());
    }
    crate::modpack::read_index(&path)
}

/// 新旧两个 index 的差异
struct Diff {
    added: Vec<PackChange>,
    removed: Vec<PackChange>,
    changed: Vec<PackChange>,
    unchanged: u32,
    bytes: u64,
}

fn diff(old: &crate::modpack::ModpackIndex, new: &crate::modpack::ModpackIndex) -> Diff {
    use std::collections::BTreeMap;
    let old_map: BTreeMap<&str, &crate::modpack::IndexFile> =
        old.files.iter().map(|f| (f.path.as_str(), f)).collect();
    let new_map: BTreeMap<&str, &crate::modpack::IndexFile> =
        new.files.iter().map(|f| (f.path.as_str(), f)).collect();

    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();
    let mut unchanged = 0u32;
    let mut bytes = 0u64;

    for (path, f) in &new_map {
        match old_map.get(path) {
            None => {
                added.push(PackChange {
                    path: (*path).to_string(),
                    kind: PackChange::kind_of(path).to_string(),
                    old_size: 0,
                    new_size: f.size,
                    url: f.url.clone(),
                });
                bytes = bytes.saturating_add(f.size);
            }
            Some(o) if o.size != f.size || o.sha1 != f.sha1 => {
                changed.push(PackChange {
                    path: (*path).to_string(),
                    kind: PackChange::kind_of(path).to_string(),
                    old_size: o.size,
                    new_size: f.size,
                    url: f.url.clone(),
                });
                bytes = bytes.saturating_add(f.size);
            }
            Some(_) => unchanged += 1,
        }
    }
    for (path, f) in &old_map {
        if !new_map.contains_key(path) {
            removed.push(PackChange {
                path: (*path).to_string(),
                kind: PackChange::kind_of(path).to_string(),
                old_size: f.size,
                new_size: 0,
                url: String::new(),
            });
        }
    }
    // 排序让前端显示稳定（BTreeMap 已按 key 有序，这里保证 removed 也有序）
    removed.sort_by(|a, b| a.path.cmp(&b.path));
    Diff {
        added,
        removed,
        changed,
        unchanged,
        bytes,
    }
}

/// 规划一次整合包更新：会加什么、删什么、要多少空间
pub fn plan(game: &Path, instance_id: &str, version_id: &str) -> Result<PackUpdatePlan, String> {
    plan_at(API, game, instance_id, version_id)
}

#[doc(hidden)]
pub fn plan_at(
    base: &str,
    game: &Path,
    instance_id: &str,
    version_id: &str,
) -> Result<PackUpdatePlan, String> {
    let dir = crate::instance::instance_dir(game, instance_id);
    if !dir.is_dir() {
        return Err("INSTANCE_NOT_FOUND|实例不存在".into());
    }
    let meta = crate::instance::read_meta(game, instance_id)
        .ok_or_else(|| "INSTANCE_NOT_FOUND|实例元数据丢失".to_string())?;
    let prov = crate::modpack::read_provenance(&dir)
        .ok_or_else(|| "INVALID_VALUE|这个实例不是从整合包装的，没有来源信息".to_string())?;
    if prov.version_id == version_id {
        return Err("INVALID_VALUE|这就是当前装的版本，不需要更新".into());
    }

    let old = fetch_index(base, game, &prov.version_id)?;
    let new = fetch_index(base, game, version_id)?;
    let d = diff(&old, &new);

    let mut issues = Vec::new();
    if old.mc_version != new.mc_version {
        issues.push(issue(
            "INCOMPATIBLE",
            format!(
                "新版本要 Minecraft {}，当前实例是 {}。换游戏版本等于换一个实例，不建议原地更新",
                new.mc_version, old.mc_version
            ),
            true,
        ));
    }
    if old.loader_kind != new.loader_kind {
        issues.push(issue(
            "INCOMPATIBLE",
            format!(
                "加载器从 {} 变成 {}，没法原地替换",
                old.loader_kind.clone().unwrap_or_else(|| "原版".into()),
                new.loader_kind.clone().unwrap_or_else(|| "原版".into())
            ),
            true,
        ));
    }
    let removed_mods = d.removed.iter().filter(|c| c.kind == "mod").count();
    if removed_mods > 0 {
        issues.push(issue(
            "NOTE",
            format!("新版本里有 {removed_mods} 个 Mod 被移除了，更新时会从实例里删掉"),
            false,
        ));
    }
    let added_config = d.added.iter().filter(|c| c.kind == "config").count()
        + d.changed.iter().filter(|c| c.kind == "config").count();
    if added_config > 0 {
        issues.push(issue(
            "NOTE",
            format!("有 {added_config} 个配置文件会被覆盖，改过的设置可能丢失"),
            false,
        ));
    }

    let disk_free = crate::world_backup::available_space(&dir);
    if let Some(free) = disk_free {
        if free < d.bytes {
            issues.push(issue(
                "DISK_FULL",
                format!(
                    "磁盘剩余空间不够：需要约 {} MiB，可用 {} MiB",
                    d.bytes / 1024 / 1024,
                    free / 1024 / 1024
                ),
                true,
            ));
        }
    }
    let blocking = issues.iter().any(|i| i.blocking);

    Ok(PackUpdatePlan {
        plan_id: format!("uplan_{:x}", crate::time::now_nanos()),
        instance_id: instance_id.to_string(),
        pack_name: if prov.pack_name.is_empty() {
            meta.name.clone()
        } else {
            prov.pack_name.clone()
        },
        from_version_id: prov.version_id.clone(),
        from_version_name: prov.version_name.clone(),
        to_version_id: version_id.to_string(),
        to_version_name: str_of(
            &fetch(base, &format!("{base}/version/{version_id}"))?,
            "version_number",
        )
        .unwrap_or_else(|| version_id.to_string()),
        added: d.added,
        removed: d.removed,
        changed: d.changed,
        unchanged: d.unchanged,
        download_bytes: d.bytes,
        disk_needed: d.bytes,
        disk_free,
        mc_version_from: old.mc_version.clone(),
        mc_version_to: new.mc_version.clone(),
        loader_from: old.loader_kind.clone(),
        loader_to: new.loader_kind.clone(),
        keeps_worlds: true,
        touches_saves: false,
        issues,
        blocking,
    })
}

/// 给自检用：把一段 zip 写成 .mrpack
#[doc(hidden)]
pub fn write_test_pack(path: &Path, index_json: &str) -> Result<(), String> {
    use std::io::Write;
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).map_err(|e| format!("PERMISSION|{e}"))?;
    }
    let f = std::fs::File::create(path).map_err(|e| format!("PERMISSION|{e}"))?;
    let mut zw = zip::ZipWriter::new(f);
    let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default();
    zw.start_file("modrinth.index.json", opts)
        .map_err(|e| format!("INTERNAL|{e}"))?;
    zw.write_all(index_json.as_bytes())
        .map_err(|e| format!("INTERNAL|{e}"))?;
    zw.start_file("overrides/config/x.toml", opts)
        .map_err(|e| format!("INTERNAL|{e}"))?;
    zw.write_all(b"k=1").map_err(|e| format!("INTERNAL|{e}"))?;
    zw.finish().map_err(|e| format!("INTERNAL|{e}"))?;
    Ok(())
}

/// 自检用：返回本地缓存里那个包该放的位置
#[doc(hidden)]
pub fn cache_path(game: &Path, file_name: &str) -> PathBuf {
    game.join(".astral").join("modpacks").join(file_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modpack::{IndexFile, ModpackIndex};

    fn idx(files: Vec<(&str, u64, &str)>) -> ModpackIndex {
        ModpackIndex {
            name: "p".into(),
            summary: String::new(),
            mc_version: "1.20.1".into(),
            loader_kind: Some("fabric".into()),
            loader_version: Some("0.16.9".into()),
            files: files
                .into_iter()
                .map(|(p, s, h)| IndexFile {
                    path: p.to_string(),
                    url: format!("https://cdn.modrinth.com/{p}"),
                    size: s,
                    sha1: Some(h.to_string()),
                    sha512: None,
                    env_client: "required".into(),
                    env_server: "required".into(),
                })
                .collect(),
            total_bytes: 0,
            client_files: 0,
            optional_files: 0,
            unsupported_files: 0,
            server_only_files: 0,
            override_entries: 0,
        }
    }

    #[test]
    fn diff_finds_added_removed_changed() {
        let old = idx(vec![
            ("mods/keep.jar", 10, "aa"),
            ("mods/gone.jar", 20, "bb"),
            ("mods/grow.jar", 30, "cc"),
        ]);
        let new = idx(vec![
            ("mods/keep.jar", 10, "aa"),
            ("mods/grow.jar", 99, "dd"),
            ("mods/fresh.jar", 40, "ee"),
        ]);
        let d = diff(&old, &new);
        assert_eq!(d.unchanged, 1);
        assert_eq!(d.added.len(), 1);
        assert_eq!(d.added[0].path, "mods/fresh.jar");
        assert_eq!(d.added[0].kind, "mod");
        assert_eq!(d.removed.len(), 1);
        assert_eq!(d.removed[0].path, "mods/gone.jar");
        assert_eq!(d.changed.len(), 1);
        assert_eq!(d.changed[0].path, "mods/grow.jar");
        assert_eq!(d.changed[0].old_size, 30);
        assert_eq!(d.changed[0].new_size, 99);
        // 只有要下的才计入字节：新加的 40 + 变大的 99
        assert_eq!(d.bytes, 139);
    }

    #[test]
    fn same_sha1_is_not_a_change() {
        let old = idx(vec![("mods/a.jar", 10, "aa")]);
        let new = idx(vec![("mods/a.jar", 10, "aa")]);
        let d = diff(&old, &new);
        assert!(d.added.is_empty() && d.removed.is_empty() && d.changed.is_empty());
        assert_eq!(d.unchanged, 1);
        assert_eq!(d.bytes, 0);
    }

    #[test]
    fn classifies_paths() {
        assert_eq!(PackChange::kind_of("mods/a.jar"), "mod");
        assert_eq!(PackChange::kind_of("config/a.toml"), "config");
        assert_eq!(PackChange::kind_of("defaultconfigs/a.toml"), "config");
        assert_eq!(PackChange::kind_of("resourcepacks/a.zip"), "other");
    }

    #[test]
    fn truncates_long_text() {
        let s = "字".repeat(50);
        let out = truncate(&s, 10);
        assert_eq!(out.chars().count(), 11);
        assert!(out.ends_with('…'));
        assert_eq!(truncate("short", 10), "short");
    }

    #[test]
    fn rejects_instance_without_provenance() {
        let game = std::env::temp_dir().join(format!("astral-pu-{:x}", crate::time::now_nanos()));
        std::fs::create_dir_all(crate::instance::instances_root(&game)).unwrap();
        let meta = crate::instance::InstanceMeta {
            id: "pu_test".into(),
            name: "无来源".into(),
            game_version: "1.20.1".into(),
            loader_kind: None,
            loader_version: None,
            launch_version: None,
            memory_mb: None,
            jvm_args: vec![],
            java_path: None,
            cover: None,
            created_at: crate::time::now_iso(),
        };
        crate::instance::create(&game, &meta).unwrap();
        let err = check_at("https://example.invalid", &game, "pu_test").unwrap_err();
        assert!(err.starts_with("INVALID_VALUE|"), "{err}");
        let _ = std::fs::remove_dir_all(&game);
    }
}
