//! 资源包 / 光影包 / 数据包的文件层管理。
//!
//! 和 `mods.rs` 是同一类事，但资源包有自己的 `pack.mcmeta`（`pack_format`
//! 决定它适配哪个游戏版本），光影包则是看包里有没有 `shaders/` 目录。
//! 这里只读元数据、只做重命名与删除，不改 `options.txt` 里的启用顺序
//! ——那是游戏自己的事，动它风险大于收益。
//!
//! 所有函数都要求传入"已经解析好的目录"，调用方负责按实例与类型取目录。

use serde::Serialize;
use std::io::Read;
use std::path::{Path, PathBuf};

/// 支持的包类型
#[allow(dead_code)]
pub const KINDS: [&str; 3] = ["resourcepack", "shaderpack", "datapack"];

/// 数据包嵌在存档里，不在这里管理；这里只允许前两种
pub fn normalize_kind(kind: &str) -> Result<&'static str, String> {
    match kind {
        "resourcepack" => Ok("resourcepack"),
        "shaderpack" => Ok("shaderpack"),
        "datapack" => Ok("datapack"),
        other => Err(format!("INVALID_VALUE|不支持的包类型：{other}")),
    }
}

/// 该类型对应的子目录名（相对实例目录或游戏目录）
#[allow(dead_code)]
pub fn sub_dir(kind: &str) -> Result<&'static str, String> {
    match normalize_kind(kind)? {
        "resourcepack" => Ok("resourcepacks"),
        "shaderpack" => Ok("shaderpacks"),
        "datapack" => Ok("datapacks"),
        _ => unreachable!(),
    }
}

/// 该类型认的扩展名（小写比较）
fn extensions(kind: &str) -> &'static [&'static str] {
    match kind {
        "shaderpack" => &[".zip"],
        // 数据包只可能是 zip；带 .jar 的一律不认
        "datapack" => &[".zip"],
        _ => &[".zip", ".jar"],
    }
}

/// 数据包住在某个存档自己的 `datapacks/` 里，得先按 world_id 找到存档。
///
/// 存档不存在（可能被删了或改名了）时给 `TARGET_CHANGED`，
/// 因为这不是"参数写错"，而是玩家看到的东西已经变了。
pub fn world_dir(saves_dir: &Path, world_id: &str) -> Result<PathBuf, String> {
    let world = crate::worlds::resolve(saves_dir, world_id)
        .ok_or_else(|| "TARGET_CHANGED|存档不存在（可能已被移动或删除）".to_string())?;
    Ok(world.join("datapacks"))
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PackFile {
    pub file_name: String,
    pub base_name: String,
    pub kind: String,
    pub enabled: bool,
    pub size_bytes: u64,
    /// 资源包/数据包的 `pack_format`，光影包恒为 None
    pub pack_format: Option<u32>,
    /// `pack.mcmeta` 里的 `description`（可以是纯文本或 JSON 文本组件，原样返回）
    pub description: Option<String>,
    /// 由 `pack_format` 推出来的游戏版本区间，推断不出来就是空
    pub compatible_versions: Vec<String>,
    /// 包里有没有 `pack.png`（列目录时顺手判断，不额外解压）
    pub has_icon: bool,
    /// 给人看的补充说明，例如"这个包是给 1.20.1 做的"
    pub note: Option<String>,
}

/// 图标文件大小上限（和 Mod 图标一致）
pub const MAX_ICON: u64 = 512 * 1024;

/// 一个包图标（以 data URL 形式交给前端）
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PackIcon {
    pub file_name: String,
    /// 在 zip 里的路径
    pub entry: String,
    pub mime: String,
    pub size_bytes: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub data_url: String,
}

/// 资源包/光影包的图标就在 zip 根目录，命名是约定俗成的几种。
fn icon_entry<R: Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>) -> Option<String> {
    for name in ["pack.png", "pack.jpg", "pack.jpeg", "logo.png", "icon.png"] {
        if zip.by_name(name).is_ok() {
            return Some(name.to_string());
        }
    }
    None
}

/// `pack_format` → 游戏版本区间。Minecraft 官方没给完整对照表，
/// 这里只覆盖资源包格式实际变更过的版本，查不到就返回空。
///
/// 数据来源是 wiki 的 pack_format 表；只写"能确定"的区间，
/// 不确定的宁可空着也不猜。
pub fn versions_for_format(format: u32) -> Vec<String> {
    let (from, to) = match format {
        1 => ("1.6.1", "1.8.9"),
        2 => ("1.9", "1.10.2"),
        3 => ("1.11", "1.12.2"),
        4 => ("1.13", "1.14.4"),
        5 => ("1.15", "1.16.1"),
        6 => ("1.16.2", "1.16.5"),
        7 => ("1.17", "1.17.1"),
        8 => ("1.18", "1.18.2"),
        9 => ("1.19", "1.19.2"),
        12 => ("1.19.3", "1.19.3"),
        13 => ("1.19.4", "1.19.4"),
        15 => ("1.20", "1.20.1"),
        18 => ("1.20.2", "1.20.2"),
        22 => ("1.20.3", "1.20.4"),
        32 => ("1.20.5", "1.20.6"),
        34 => ("1.21", "1.21.1"),
        42 => ("1.21.2", "1.21.3"),
        46 => ("1.21.4", "1.21.4"),
        55 => ("1.21.5", "1.21.5"),
        _ => return Vec::new(),
    };
    if from == to {
        vec![from.to_string()]
    } else {
        vec![format!("{from} – {to}")]
    }
}

/// 把 `a.zip` / `a.zip.disabled` 拆成 (主体, 是否启用)
fn split_name(file_name: &str) -> Option<(String, bool)> {
    let lower = file_name.to_ascii_lowercase();
    if let Some(stem) = lower.strip_suffix(".disabled") {
        if stem.ends_with(".zip") || stem.ends_with(".jar") {
            // 保留原始大小写，只去掉 .disabled
            let cut = file_name.len() - ".disabled".len();
            return Some((file_name[..cut].to_string(), false));
        }
        return None;
    }
    if lower.ends_with(".zip") || lower.ends_with(".jar") {
        return Some((file_name.to_string(), true));
    }
    None
}

fn read_entry<R: Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>, name: &str) -> Option<String> {
    const MAX_META: u64 = 1024 * 1024;
    let mut f = zip.by_name(name).ok()?;
    if f.size() > MAX_META {
        return None;
    }
    let mut s = String::new();
    f.read_to_string(&mut s).ok()?;
    Some(s)
}

#[derive(Default)]
struct PackMeta {
    format: Option<u32>,
    description: Option<String>,
    has_shaders: bool,
    has_icon: bool,
}

/// 描述字段在 `pack.mcmeta` 里可能是字符串，也可能是文本组件对象。
/// 对象时取 `text`，取不到就把整个 JSON 压成一行给人看。
fn description_of(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => {
            let s = s.trim();
            (!s.is_empty()).then(|| s.to_string())
        }
        serde_json::Value::Object(_) => {
            if let Some(t) = v.get("text").and_then(|x| x.as_str()) {
                return Some(t.to_string());
            }
            let extra = v
                .get("extra")
                .and_then(|e| e.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.get("text").and_then(|t| t.as_str()))
                        .collect::<Vec<_>>()
                        .join("")
                })
                .unwrap_or_default();
            if !extra.trim().is_empty() {
                return Some(extra);
            }
            serde_json::to_string(v).ok()
        }
        _ => None,
    }
}

fn read_pack_meta(path: &Path, kind: &str) -> PackMeta {
    let mut meta = PackMeta::default();
    let Ok(file) = std::fs::File::open(path) else {
        return meta;
    };
    let Ok(mut zip) = zip::ZipArchive::new(file) else {
        return meta;
    };

    if kind == "shaderpack" {
        // 光影包没有 pack.mcmeta，靠 shaders/ 目录判定
        meta.has_shaders = zip
            .file_names()
            .filter_map(|n| n.ok())
            .any(|n| n.starts_with("shaders/") || n == "shaders");
        meta.has_icon = icon_entry(&mut zip).is_some();
        return meta;
    }

    meta.has_icon = icon_entry(&mut zip).is_some();

    if let Some(text) = read_entry(&mut zip, "pack.mcmeta") {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            let pack = v.get("pack");
            meta.format = pack
                .and_then(|p| p.get("pack_format"))
                .and_then(|f| f.as_u64())
                .and_then(|f| u32::try_from(f).ok());
            meta.description = pack
                .and_then(|p| p.get("description"))
                .and_then(description_of);
            // 1.20.2 之后允许 supported_formats 声明兼容区间，有就用它覆盖
            if let Some(sf) = pack.and_then(|p| p.get("supported_formats")) {
                if let Some(f) = sf.as_u64().and_then(|f| u32::try_from(f).ok()) {
                    meta.format = Some(f);
                }
            }
        }
    }
    meta
}

fn build(dir: &Path, file_name: &str, enabled: bool, kind: &str, meta: PackMeta) -> PackFile {
    let path = dir.join(file_name);
    let size_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let base_name = split_name(file_name)
        .map(|(s, _)| s)
        .unwrap_or_else(|| file_name.to_string());
    let compatible_versions = meta.format.map(versions_for_format).unwrap_or_default();
    let note = match (kind, meta.format) {
        ("shaderpack", _) if !meta.has_shaders => {
            Some("包里没有 shaders/ 目录，可能不是光影包".to_string())
        }
        ("shaderpack", _) => None,
        (_, Some(f)) => {
            let v = versions_for_format(f);
            if v.is_empty() {
                Some(format!("pack_format {f} 不在已知对照表里"))
            } else {
                Some(format!("pack_format {f}，适配 {}", v.join("、")))
            }
        }
        (_, None) => Some("包里没有 pack.mcmeta，游戏可能不认".to_string()),
    };
    PackFile {
        file_name: file_name.to_string(),
        base_name,
        kind: kind.to_string(),
        enabled,
        size_bytes,
        pack_format: meta.format,
        description: meta.description,
        compatible_versions,
        has_icon: meta.has_icon,
        note,
    }
}

/// 列出某个目录下的全部包（含已禁用的）
pub fn list(dir: &Path, kind: &str) -> Result<Vec<PackFile>, String> {
    let kind = normalize_kind(kind)?;
    let Ok(rd) = std::fs::read_dir(dir) else {
        // 目录不存在不算错误：还没装过任何包而已
        return Ok(Vec::new());
    };
    let exts = extensions(kind);
    let mut out = Vec::new();
    for e in rd.flatten() {
        let path = e.path();
        if !path.is_file() {
            continue;
        }
        let file_name = e.file_name().to_string_lossy().to_string();
        let Some((_, enabled)) = split_name(&file_name) else {
            continue;
        };
        let lower = file_name.to_ascii_lowercase();
        let stem = lower.trim_end_matches(".disabled");
        if !exts.iter().any(|x| stem.ends_with(x)) {
            continue;
        }
        let meta = read_pack_meta(&path, kind);
        out.push(build(dir, &file_name, enabled, kind, meta));
    }
    out.sort_by_key(|p| p.base_name.to_lowercase());
    Ok(out)
}

fn safe_name(file_name: &str) -> Result<(), String> {
    if file_name.is_empty() || file_name.contains(['/', '\\', ':']) || file_name.contains("..") {
        return Err("INVALID_PATH|文件名不安全".into());
    }
    if Path::new(file_name).components().count() != 1 {
        return Err("INVALID_PATH|文件名不安全".into());
    }
    Ok(())
}

/// 目标名冲突时依次加 -1、-2……
fn unique_target(dir: &Path, stem: &str, suffix: &str) -> PathBuf {
    let first = dir.join(format!("{stem}{suffix}"));
    if !first.exists() {
        return first;
    }
    for i in 1..=20 {
        let p = dir.join(format!("{stem}-{i}{suffix}"));
        if !p.exists() {
            return p;
        }
    }
    dir.join(format!("{stem}-{}{suffix}", crate::time::now_unix_ms()))
}

/// 启用/禁用：靠改文件名后缀 `.disabled` 实现，和 `mods.rs` 一致。
///
/// 不用"把文件移到别处"，因为那样游戏就看不到它了，玩家也没法在
/// 游戏的资源包界面里重新启用。
pub fn set_enabled(
    dir: &Path,
    kind: &str,
    file_name: &str,
    enabled: bool,
) -> Result<PackFile, String> {
    let kind = normalize_kind(kind)?;
    safe_name(file_name)?;
    let (stem, currently) = split_name(file_name).ok_or("INVALID_NAME|不是可识别的包文件")?;
    if currently == enabled {
        let meta = read_pack_meta(&dir.join(file_name), kind);
        return Ok(build(dir, file_name, enabled, kind, meta));
    }
    let src = dir.join(file_name);
    if !src.is_file() {
        return Err("INVALID_NAME|文件不存在".into());
    }

    let dest = if enabled {
        unique_target(dir, &stem, "")
    } else {
        unique_target(dir, &stem, ".disabled")
    };
    std::fs::rename(&src, &dest).map_err(|e| format!("PERMISSION|重命名失败|{e}"))?;
    let new_name = dest
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| file_name.to_string());
    let meta = read_pack_meta(&dest, kind);
    Ok(build(dir, &new_name, enabled, kind, meta))
}

/// 删除必须显式确认；只删这一个文件，不动同目录其他东西。
pub fn delete(dir: &Path, kind: &str, file_name: &str, confirm: bool) -> Result<(), String> {
    normalize_kind(kind)?;
    if !confirm {
        return Err("UNSUPPORTED|删除文件需要明确确认".into());
    }
    safe_name(file_name)?;
    if split_name(file_name).is_none() {
        return Err("INVALID_NAME|不是可识别的包文件".into());
    }
    let p = dir.join(file_name);
    if !p.is_file() {
        return Err("INVALID_NAME|文件不存在".into());
    }
    if crate::net::is_reparse_point(&p) {
        return Err("INVALID_PATH|这是一个符号链接，已拒绝删除".into());
    }
    std::fs::remove_file(&p).map_err(|e| format!("PERMISSION|删除失败|{e}"))
}

/// 读取单个包的图标，返回 data URL。
///
/// 和 Mod 图标一样的取舍：列表只给 `hasIcon` 布尔，真要显示时再调这个，
/// 避免一次列几十个包就把几 MB 图片塞进 IPC。
pub fn read_icon(dir: &Path, kind: &str, file_name: &str) -> Result<Option<PackIcon>, String> {
    normalize_kind(kind)?;
    safe_name(file_name)?;
    if split_name(file_name).is_none() {
        return Err("INVALID_NAME|不是可识别的包文件".into());
    }
    let path = dir.join(file_name);
    if !path.is_file() {
        return Err("INVALID_NAME|文件不存在".into());
    }
    if crate::net::is_reparse_point(&path) {
        return Err("INVALID_PATH|这是一个符号链接，已拒绝读取".into());
    }
    let Ok(file) = std::fs::File::open(&path) else {
        return Err("PERMISSION|打不开这个文件".into());
    };
    let Ok(mut zip) = zip::ZipArchive::new(file) else {
        // 坏包不报错，只是没图标 —— 列表里那个包本身还能禁用/删除
        return Ok(None);
    };
    let Some(entry) = icon_entry(&mut zip) else {
        return Ok(None);
    };
    let Ok(mut f) = zip.by_name(&entry) else {
        return Ok(None);
    };
    if f.size() > MAX_ICON {
        return Ok(None);
    }
    let mut bytes = Vec::with_capacity(f.size() as usize);
    if f.read_to_end(&mut bytes).is_err() {
        return Ok(None);
    }
    drop(f);
    if bytes.is_empty() {
        return Ok(None);
    }
    let mime = match crate::mod_icon::mime_of(&bytes) {
        Some(m) => m,
        None => return Ok(None),
    };
    let (width, height) = match crate::cover::size_of(mime, &bytes) {
        Some((w, h)) => (Some(w), Some(h)),
        None => (None, None),
    };
    Ok(Some(PackIcon {
        file_name: file_name.to_string(),
        entry,
        mime: mime.to_string(),
        size_bytes: bytes.len() as u64,
        width,
        height,
        data_url: format!(
            "data:{};base64,{}",
            mime,
            crate::screenshots::base64_encode(&bytes)
        ),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_split_for_packs() {
        assert_eq!(
            split_name("Better Leaves.zip"),
            Some(("Better Leaves.zip".to_string(), true))
        );
        assert_eq!(
            split_name("Better Leaves.zip.disabled"),
            Some(("Better Leaves.zip".to_string(), false))
        );
        assert_eq!(split_name("readme.txt"), None);
        assert_eq!(split_name("a.zip.disabled.disabled"), None);
    }

    #[test]
    fn format_table_covers_modern_versions() {
        assert_eq!(versions_for_format(15), vec!["1.20 – 1.20.1"]);
        assert_eq!(versions_for_format(46), vec!["1.21.4"]);
        assert!(versions_for_format(999).is_empty());
    }

    #[test]
    fn description_handles_text_and_object() {
        let s = serde_json::json!("Simple pack");
        assert_eq!(description_of(&s).as_deref(), Some("Simple pack"));
        let o = serde_json::json!({"text": "Hi"});
        assert_eq!(description_of(&o).as_deref(), Some("Hi"));
        let e = serde_json::json!({"extra": [{"text": "A"}, {"text": "B"}]});
        assert_eq!(description_of(&e).as_deref(), Some("AB"));
    }

    #[test]
    fn rejects_unsafe_names() {
        assert!(safe_name("../x.zip").is_err());
        assert!(safe_name("a/b.zip").is_err());
        assert!(safe_name("C:x.zip").is_err());
        assert!(safe_name("ok.zip").is_ok());
    }

    #[test]
    fn lists_only_matching_extensions() {
        let dir = std::env::temp_dir().join(format!("astral-packs-{}", crate::time::now_nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.zip"), b"x").unwrap();
        std::fs::write(dir.join("b.zip.disabled"), b"y").unwrap();
        std::fs::write(dir.join("notes.txt"), b"z").unwrap();
        let list = list(&dir, "resourcepack").unwrap();
        assert_eq!(list.len(), 2);
        assert!(list.iter().any(|p| p.file_name == "a.zip" && p.enabled));
        assert!(list
            .iter()
            .any(|p| p.file_name == "b.zip.disabled" && !p.enabled));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_requires_confirm() {
        let dir = std::env::temp_dir().join(format!("astral-packs-{}", crate::time::now_nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.zip"), b"x").unwrap();
        assert!(delete(&dir, "resourcepack", "a.zip", false).is_err());
        assert!(dir.join("a.zip").exists());
        assert!(delete(&dir, "resourcepack", "a.zip", true).is_ok());
        assert!(!dir.join("a.zip").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn datapacks_only_accept_zip() {
        let dir = std::env::temp_dir().join(format!("astral-packs-{}", crate::time::now_nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.zip"), b"x").unwrap();
        // 数据包目录里放 jar 不认：游戏也读不出来
        std::fs::write(dir.join("b.jar"), b"y").unwrap();
        let list = list(&dir, "datapack").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].file_name, "a.zip");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn datapack_world_dir_needs_a_real_world() {
        let saves = std::env::temp_dir().join(format!("astral-saves-{}", crate::time::now_nanos()));
        let world = saves.join("World A");
        std::fs::create_dir_all(&world).unwrap();
        std::fs::write(world.join("level.dat"), b"x").unwrap();

        let id = crate::worlds::world_id("World A");
        let found = world_dir(&saves, &id).unwrap();
        assert!(found.ends_with("World A/datapacks"));

        // 不存在的 world_id 不能返回一个"随便找个目录"的路径
        let missing = world_dir(&saves, "w_deadbeef").unwrap_err();
        assert!(missing.starts_with("TARGET_CHANGED|"));
        let _ = std::fs::remove_dir_all(&saves);
    }

    /// 最小可用的 PNG 头（够 `cover::size_of` 读出宽高，不需要真的能显示）
    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut v = vec![
            0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, b'I', b'H',
            b'D', b'R',
        ];
        v.extend_from_slice(&w.to_be_bytes());
        v.extend_from_slice(&h.to_be_bytes());
        v.extend_from_slice(&[0x08, 0x06, 0x00, 0x00, 0x00]);
        v
    }

    /// 造一个最小可用的 zip（带 pack.mcmeta 与可选的 pack.png）
    fn make_pack(path: &Path, with_icon: bool) {
        use std::io::Write;
        let f = std::fs::File::create(path).unwrap();
        let mut z = zip::ZipWriter::new(f);
        let opts = zip::write::SimpleFileOptions::default();
        z.start_file("pack.mcmeta", opts).unwrap();
        z.write_all(br#"{"pack":{"pack_format":15,"description":"Demo"}}"#)
            .unwrap();
        if with_icon {
            z.start_file("pack.png", opts).unwrap();
            z.write_all(&png(8, 8)).unwrap();
        }
        z.finish().unwrap();
    }

    #[test]
    fn lists_icon_flag_without_reading_the_image() {
        let dir = std::env::temp_dir().join(format!("astral-packs-{}", crate::time::now_nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        make_pack(&dir.join("with.zip"), true);
        make_pack(&dir.join("without.zip"), false);
        let list = list(&dir, "resourcepack").unwrap();
        assert_eq!(list.len(), 2);
        let with = list.iter().find(|p| p.file_name == "with.zip").unwrap();
        let without = list.iter().find(|p| p.file_name == "without.zip").unwrap();
        assert!(with.has_icon);
        assert!(!without.has_icon);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reads_a_pack_icon_as_data_url() {
        let dir = std::env::temp_dir().join(format!("astral-packs-{}", crate::time::now_nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        make_pack(&dir.join("with.zip"), true);
        let icon = read_icon(&dir, "resourcepack", "with.zip")
            .unwrap()
            .unwrap();
        assert_eq!(icon.entry, "pack.png");
        assert_eq!(icon.mime, "image/png");
        assert_eq!(icon.width, Some(8));
        assert_eq!(icon.height, Some(8));
        assert!(icon.data_url.starts_with("data:image/png;base64,"));

        // 没有图标的包返回 None，不是错误
        make_pack(&dir.join("without.zip"), false);
        assert!(read_icon(&dir, "resourcepack", "without.zip")
            .unwrap()
            .is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pack_icon_rejects_bad_names_and_missing_files() {
        let dir = std::env::temp_dir().join(format!("astral-packs-{}", crate::time::now_nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(read_icon(&dir, "resourcepack", "../x.zip").is_err());
        assert!(read_icon(&dir, "resourcepack", "nope.zip").is_err());
        assert!(read_icon(&dir, "taco", "a.zip").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
