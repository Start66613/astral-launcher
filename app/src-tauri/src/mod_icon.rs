//! 从 mod 的 jar 里取出图标（契约 v3 §5.6 增量）。
//!
//! 背景：`mods::list` 的 `iconPath` 一直是 `None`，Mod 列表里只能显示文件名。
//! 四种加载器声明图标的方式不一样：
//! - Fabric：`fabric.mod.json` 的 `icon`（字符串，或 `{ "1x": "a.png" }` 这种对象）
//! - Quilt：`quilt.mod.json` 的 `quilt_loader.metadata.icon`（同上）
//! - Forge / NeoForge：`mods.toml` / `neoforge.mods.toml` 里的 `logoFile = "x.png"`
//!
//! 还有一批老 Mod 什么都不声明，图标就放在 jar 根目录的 `icon.png` / `logo.png`。
//!
//! 取舍：
//! - 列表只给 `hasIcon` 布尔（打开 jar 时顺手判断，不额外解压），
//!   真正要图时再调 `read_mod_icon` 拿 data URL，避免一次列 100 个 Mod 就把几 MB 图片塞进 IPC。
//! - 图标上限 512 KiB，超了就当没有（有些包里塞了整张壁纸）。
//! - 只读：不往玩家目录写任何缓存文件。

use serde::Serialize;
use std::io::{Read, Seek};
use std::path::Path;

/// 图标文件大小上限
pub const MAX_ICON: u64 = 512 * 1024;
/// 元数据文件读取上限（与 mods.rs 保持一致）
const MAX_META: u64 = 1024 * 1024;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModIcon {
    pub file_name: String,
    /// 在 jar 里的路径
    pub entry: String,
    pub mime: String,
    pub size_bytes: u64,
    pub data_url: String,
    /// 从哪儿找到的：fabric / quilt / forge / root
    pub source: String,
}

fn read_entry<R: Read + Seek>(zip: &mut zip::ZipArchive<R>, name: &str) -> Option<String> {
    let mut f = zip.by_name(name).ok()?;
    if f.size() > MAX_META {
        return None;
    }
    let mut s = String::new();
    f.read_to_string(&mut s).ok()?;
    Some(s)
}

/// `"icon": "a.png"` 或 `"icon": { "1x": "a.png" }` 都能读
fn icon_from_json(v: &serde_json::Value, key: &str) -> Option<String> {
    let node = v.get(key)?;
    if let Some(s) = node.as_str() {
        return (!s.trim().is_empty()).then(|| s.trim().to_string());
    }
    if let Some(obj) = node.as_object() {
        // 优先取最大的那张，回退任意一张
        for want in ["2x", "1x", "4x", "8x"] {
            if let Some(s) = obj.get(want).and_then(|x| x.as_str()) {
                if !s.trim().is_empty() {
                    return Some(s.trim().to_string());
                }
            }
        }
        for (_, x) in obj {
            if let Some(s) = x.as_str() {
                if !s.trim().is_empty() {
                    return Some(s.trim().to_string());
                }
            }
        }
    }
    None
}

/// 从 TOML 文本里取 `logoFile = "x.png"`（不引 toml 依赖）
fn logo_file(text: &str) -> Option<String> {
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('#') {
            continue;
        }
        let Some(rest) = l.strip_prefix("logoFile") else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix('=') else {
            continue;
        };
        let rest = rest.trim();
        // 去掉引号；Forge 允许 "a.png,b.png"，取第一个
        let raw = rest.trim_matches(|c| c == '"' || c == '\'');
        let first = raw.split(',').next().unwrap_or("").trim();
        if !first.is_empty() {
            return Some(first.to_string());
        }
    }
    None
}

/// 在 jar 里找图标条目，返回 (路径, 来源)
pub fn entry_in<R: Read + Seek>(zip: &mut zip::ZipArchive<R>) -> Option<(String, String)> {
    if let Some(text) = read_entry(zip, "fabric.mod.json") {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(e) = icon_from_json(&v, "icon") {
                if zip.by_name(&e).is_ok() {
                    return Some((e, "fabric".into()));
                }
            }
        }
    }
    if let Some(text) = read_entry(zip, "quilt.mod.json") {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            let icon = v
                .get("quilt_loader")
                .and_then(|q| q.get("metadata"))
                .and_then(|m| icon_from_json(m, "icon"));
            if let Some(e) = icon {
                if zip.by_name(&e).is_ok() {
                    return Some((e, "quilt".into()));
                }
            }
        }
    }
    for name in ["META-INF/neoforge.mods.toml", "META-INF/mods.toml"] {
        if let Some(text) = read_entry(zip, name) {
            if let Some(e) = logo_file(&text) {
                if zip.by_name(&e).is_ok() {
                    return Some((e, "forge".into()));
                }
            }
        }
    }
    // 兜底：根目录常见命名
    for name in ["icon.png", "logo.png", "icon.jpg"] {
        if zip.by_name(name).is_ok() {
            return Some((name.to_string(), "root".into()));
        }
    }
    None
}

/// 按魔数判断图片类型
pub(crate) fn mime_of(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some("image/png");
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    None
}

fn safe_name(file_name: &str) -> Result<&str, String> {
    let p = Path::new(file_name);
    let ok = p
        .file_name()
        .map(|n| n.to_string_lossy() == file_name)
        .unwrap_or(false);
    if !ok || file_name.is_empty() {
        return Err("INVALID_NAME|模组文件名非法".into());
    }
    Ok(file_name)
}

/// 读出某个 Mod 的图标（data URL）。没有图标时返回 `Ok(None)`。
pub fn read(dir: &Path, file_name: &str) -> Result<Option<ModIcon>, String> {
    let file_name = safe_name(file_name)?;
    let path = dir.join(file_name);
    if !path.is_file() {
        return Err("NOT_FOUND|找不到这个 Mod".into());
    }
    if crate::net::is_reparse_point(&path) {
        return Err("INVALID_PATH|这是一个符号链接，已拒绝读取".into());
    }
    let file = std::fs::File::open(&path).map_err(|e| format!("PERMISSION|打不开这个 jar|{e}"))?;
    let mut zip =
        zip::ZipArchive::new(file).map_err(|e| format!("INVALID_VALUE|这个 jar 读不出来|{e}"))?;
    let Some((entry, source)) = entry_in(&mut zip) else {
        return Ok(None);
    };
    let mut f = match zip.by_name(&entry) {
        Ok(f) => f,
        Err(_) => return Ok(None),
    };
    if f.size() > MAX_ICON {
        return Ok(None);
    }
    let mut bytes = Vec::with_capacity(f.size() as usize);
    f.read_to_end(&mut bytes)
        .map_err(|e| format!("INVALID_VALUE|读图标失败|{e}"))?;
    let Some(mime) = mime_of(&bytes) else {
        return Ok(None);
    };
    let data_url = format!(
        "data:{mime};base64,{}",
        crate::screenshots::base64_encode(&bytes)
    );
    Ok(Some(ModIcon {
        file_name: file_name.to_string(),
        entry,
        mime: mime.to_string(),
        size_bytes: bytes.len() as u64,
        data_url,
        source,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d =
            std::env::temp_dir().join(format!("astral-mi-{tag}-{:x}", crate::time::now_nanos()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    const PNG: &[u8] = &[
        0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, b'I', b'H', b'D',
        b'R', 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0x10, 0x08, 0x06, 0x00, 0x00, 0x00,
    ];

    fn make_jar(path: &Path, files: &[(&str, &[u8])]) {
        let f = std::fs::File::create(path).unwrap();
        let mut w = zip::ZipWriter::new(f);
        let opts = zip::write::SimpleFileOptions::default();
        for (name, data) in files {
            w.start_file(*name, opts).unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap();
    }

    #[test]
    fn reads_a_fabric_icon() {
        let d = tmp("fabric");
        let meta = r#"{"id":"demo","name":"Demo","version":"1","icon":"assets/demo/icon.png"}"#;
        make_jar(
            &d.join("demo.jar"),
            &[
                ("fabric.mod.json", meta.as_bytes()),
                ("assets/demo/icon.png", PNG),
            ],
        );
        let icon = read(&d, "demo.jar").unwrap().unwrap();
        assert_eq!(icon.entry, "assets/demo/icon.png");
        assert_eq!(icon.source, "fabric");
        assert_eq!(icon.mime, "image/png");
        assert!(icon.data_url.starts_with("data:image/png;base64,"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn reads_an_icon_object_with_sizes() {
        let d = tmp("sizes");
        let meta = r#"{"id":"demo","icon":{"1x":"a.png","2x":"b.png"}}"#;
        make_jar(
            &d.join("demo.jar"),
            &[
                ("fabric.mod.json", meta.as_bytes()),
                ("a.png", PNG),
                ("b.png", PNG),
            ],
        );
        let icon = read(&d, "demo.jar").unwrap().unwrap();
        assert_eq!(icon.entry, "b.png", "有 2x 时优先取大的");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn reads_a_forge_logo_file() {
        let d = tmp("forge");
        let toml = "modLoader=\"javafml\"\n[[mods]]\ndisplayName=\"Demo\"\nlogoFile=\"pack.png\"\n";
        make_jar(
            &d.join("demo.jar"),
            &[("META-INF/mods.toml", toml.as_bytes()), ("pack.png", PNG)],
        );
        let icon = read(&d, "demo.jar").unwrap().unwrap();
        assert_eq!(icon.entry, "pack.png");
        assert_eq!(icon.source, "forge");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn falls_back_to_root_icon() {
        let d = tmp("root");
        make_jar(&d.join("demo.jar"), &[("icon.png", PNG)]);
        let icon = read(&d, "demo.jar").unwrap().unwrap();
        assert_eq!(icon.entry, "icon.png");
        assert_eq!(icon.source, "root");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn no_icon_is_not_an_error() {
        let d = tmp("none");
        make_jar(&d.join("demo.jar"), &[("readme.txt", b"hi")]);
        assert!(read(&d, "demo.jar").unwrap().is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn rejects_bad_names_and_missing_files() {
        let d = tmp("bad");
        assert!(read(&d, "../evil.jar").is_err());
        assert!(read(&d, "nope.jar").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn declared_but_missing_entry_is_skipped() {
        let d = tmp("declared");
        let meta = r#"{"id":"demo","icon":"gone.png"}"#;
        make_jar(
            &d.join("demo.jar"),
            &[("fabric.mod.json", meta.as_bytes()), ("icon.png", PNG)],
        );
        // 声明的不存在 → 退回根目录兜底
        let icon = read(&d, "demo.jar").unwrap().unwrap();
        assert_eq!(icon.entry, "icon.png");
        assert_eq!(icon.source, "root");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn rejects_oversized_icons() {
        let d = tmp("big");
        let big = vec![0u8; (MAX_ICON + 1) as usize];
        make_jar(&d.join("demo.jar"), &[("icon.png", &big)]);
        assert!(read(&d, "demo.jar").unwrap().is_none());
        let _ = std::fs::remove_dir_all(&d);
    }
}
