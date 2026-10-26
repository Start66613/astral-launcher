//! 模组管理（契约 v3 §5.6）。
//!
//! 只做文件层：列出、启停、删除。jar 元数据是「尽力读取」，
//! 读不到就回退文件名，不臆造版本号。

use serde::Serialize;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModFile {
    /// 实际文件名（禁用态带 .disabled）
    pub file_name: String,
    /// 不带扩展名的主体名
    pub base_name: String,
    pub name: String,
    pub version: Option<String>,
    pub loader: Option<String>,
    pub enabled: bool,
    pub size_bytes: u64,
    pub icon_path: Option<String>,
    pub project_id: Option<String>,
}

#[derive(Default)]
struct JarMeta {
    name: Option<String>,
    version: Option<String>,
    loader: Option<String>,
    project_id: Option<String>,
}

fn read_entry<R: Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
    name: &str,
) -> Option<String> {
    const MAX_META: u64 = 1024 * 1024;
    let mut f = zip.by_name(name).ok()?;
    // 元数据文件超过 1 MiB 视为异常，直接放弃解析，避免解压炸弹撑爆内存
    if f.size() > MAX_META {
        return None;
    }
    let mut s = String::new();
    f.read_to_string(&mut s).ok()?;
    Some(s)
}

fn json_str(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

/// 从 `mods.toml` 里取一个字段（不引 toml 依赖，只认 `key = "value"` 这种平铺写法）
fn toml_value(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('#') {
            continue;
        }
        let Some(rest) = l.strip_prefix(key) else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(v) = rest.strip_prefix('=') else {
            continue;
        };
        let v = v.split('#').next().unwrap_or(v).trim();
        let v = v.trim_matches('"').trim_matches('\'').trim();
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    None
}

fn read_jar_meta(path: &Path) -> JarMeta {
    let mut meta = JarMeta::default();
    let Ok(file) = std::fs::File::open(path) else {
        return meta;
    };
    let Ok(mut zip) = zip::ZipArchive::new(file) else {
        return meta;
    };

    // fabric 与 quilt 分开判定，不能一律标成 fabric
    let fabric_json = read_entry(&mut zip, "fabric.mod.json");
    let quilt_json = read_entry(&mut zip, "quilt.mod.json");
    if let Some((text, kind)) = fabric_json
        .map(|t| (t, "fabric"))
        .or_else(|| quilt_json.map(|t| (t, "quilt")))
    {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            meta.name = json_str(&v, "name").or_else(|| {
                v.get("quilt_loader")
                    .and_then(|q| q.get("metadata"))
                    .and_then(|m| m.get("name"))
                    .and_then(|n| n.as_str())
                    .map(|s| s.to_string())
            });
            meta.version = json_str(&v, "version").or_else(|| {
                v.get("quilt_loader")
                    .and_then(|q| q.get("version"))
                    .and_then(|n| n.as_str())
                    .map(|s| s.to_string())
            });
            meta.loader = Some(kind.into());
            meta.project_id = v
                .get("contact")
                .and_then(|c| c.get("homepage"))
                .and_then(|h| h.as_str())
                .and_then(modrinth_id);
        }
    }

    if meta.name.is_none() {
        for name in ["META-INF/neoforge.mods.toml", "META-INF/mods.toml"] {
            if let Some(text) = read_entry(&mut zip, name) {
                meta.loader = Some(if name.contains("neoforge") {
                    "neoforge".into()
                } else {
                    "forge".into()
                });
                meta.name = toml_value(&text, "displayName");
                meta.version = toml_value(&text, "version");
                break;
            }
        }
    }

    if meta.name.is_none() {
        if let Some(text) = read_entry(&mut zip, "META-INF/mcmod.info") {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                let first = v.as_array().and_then(|a| a.first());
                if let Some(first) = first {
                    meta.name = json_str(first, "name");
                    meta.version = json_str(first, "version");
                    meta.loader = Some("forge".into());
                }
            }
        }
    }

    meta
}

/// `https://modrinth.com/mod/xxx` → `xxx`
fn modrinth_id(url: &str) -> Option<String> {
    let rest = url.split("modrinth.com/mod/").nth(1)?;
    let id = rest.split(['/', '?']).next()?;
    (!id.is_empty()).then(|| id.to_string())
}

/// 把 `a.jar` / `a.jar.disabled` 拆成 (主体, 是否启用)
fn split_name(file_name: &str) -> Option<(String, bool)> {
    let lower = file_name.to_lowercase();
    if lower.ends_with(".jar.disabled") {
        Some((file_name[..file_name.len() - ".disabled".len()].to_string(), false))
    } else if lower.ends_with(".jar") {
        Some((file_name.to_string(), true))
    } else {
        None
    }
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

fn build(dir: &Path, file_name: &str, enabled: bool, meta: JarMeta) -> ModFile {
    let path = dir.join(file_name);
    let base = file_name
        .trim_end_matches(".disabled")
        .trim_end_matches(".jar")
        .to_string();
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    ModFile {
        file_name: file_name.to_string(),
        base_name: base.clone(),
        name: meta.name.unwrap_or(base),
        version: meta.version,
        loader: meta.loader,
        enabled,
        size_bytes: size,
        icon_path: None,
        project_id: meta.project_id,
    }
}

pub fn list(dir: &Path) -> Vec<ModFile> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
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
        let meta = read_jar_meta(&path);
        out.push(build(dir, &file_name, enabled, meta));
    }
    out.sort_by_key(|m| m.name.to_lowercase());
    out
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
    dir.join(format!("{stem}-{}{suffix}", chrono_free_stamp()))
}

fn chrono_free_stamp() -> String {
    crate::time::now_unix_ms().to_string()
}

pub fn set_enabled(dir: &Path, file_name: &str, enabled: bool) -> Result<ModFile, String> {
    let file_name = safe_name(file_name)?;
    let (jar_name, currently) = split_name(file_name).ok_or("INVALID_NAME|不是 .jar 文件")?;
    if currently == enabled {
        return Ok(build(dir, file_name, enabled, read_jar_meta(&dir.join(file_name))));
    }

    let src = dir.join(file_name);
    if !src.is_file() {
        return Err("INVALID_NAME|模组文件不存在".into());
    }

    let stem = jar_name.trim_end_matches(".jar");
    let dest = if enabled {
        unique_target(dir, stem, ".jar")
    } else {
        unique_target(dir, stem, ".jar.disabled")
    };
    std::fs::rename(&src, &dest).map_err(|e| format!("PERMISSION|重命名失败|{e}"))?;

    let new_name = dest
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| file_name.to_string());
    Ok(build(dir, &new_name, enabled, read_jar_meta(&dest)))
}

pub fn delete(dir: &Path, file_name: &str) -> Result<(), String> {
    let file_name = safe_name(file_name)?;
    if split_name(file_name).is_none() {
        return Err("INVALID_NAME|不是 .jar 文件".into());
    }
    let p = dir.join(file_name);
    if !p.is_file() {
        return Err("INVALID_NAME|模组文件不存在".into());
    }
    std::fs::remove_file(&p).map_err(|e| format!("PERMISSION|删除失败|{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_split() {
        assert_eq!(split_name("a.jar"), Some(("a.jar".to_string(), true)));
        assert_eq!(
            split_name("a.jar.disabled"),
            Some(("a.jar".to_string(), false))
        );
        assert_eq!(split_name("a.txt"), None);
    }

    #[test]
    fn toml_pick() {
        let t = "[mods]\n    displayName=\"JEI\"\n    version=\"15.2\"\n";
        assert_eq!(toml_value(t, "displayName").as_deref(), Some("JEI"));
        assert_eq!(toml_value(t, "version").as_deref(), Some("15.2"));
        assert_eq!(toml_value(t, "modId"), None);
    }

    #[test]
    fn modrinth_url() {
        assert_eq!(
            modrinth_id("https://modrinth.com/mod/jei").as_deref(),
            Some("jei")
        );
        assert_eq!(modrinth_id("https://example.com"), None);
    }
}
