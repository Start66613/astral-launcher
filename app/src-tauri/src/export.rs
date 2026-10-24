//! 整合包导出（.mrpack）。
//!
//! 规则：
//! - Modrinth 能查到的 mod 写进 `modrinth.index.json`（带 sha1/sha512/下载地址）；
//! - 查不到的 jar 与 `config/` 一起放进 `overrides/`（随包分发）；
//! - 只读实例，写到 `<gameDir>/exports/`，不覆盖已有文件。

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;

const API: &str = "https://api.modrinth.com/v2";

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ExportRequest {
    pub instance_id: String,
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub include_config: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub path: String,
    /// 写进 index 的 Modrinth 文件数
    pub indexed_files: u32,
    /// 打进 overrides 的文件数
    pub override_files: u32,
    pub bytes: u64,
}

fn sha1_of(p: &Path) -> Option<String> {
    crate::net::sha1_file(p).ok()
}

fn version_file(sha1: &str) -> Option<serde_json::Value> {
    crate::net::fetch_json(&format!("{API}/version_file/{sha1}?algorithm=sha1")).ok()
}

fn str_of(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

fn loader_dep(kind: &str) -> &'static str {
    match kind {
        "fabric" => "fabric-loader",
        "quilt" => "quilt-loader",
        "forge" => "forge",
        "neoforge" => "neoforge",
        _ => "fabric-loader",
    }
}

fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "pack".to_string()
    } else {
        trimmed
    }
}

/// 导出实例为 .mrpack，返回产物信息
pub fn export(
    game: &Path,
    req: &ExportRequest,
    stats: &dyn Fn(&str) -> crate::instance::PlayStats,
) -> Result<ExportResult, String> {
    let inst = crate::instance::find(game, &req.instance_id, stats)
        .ok_or_else(|| "INSTANCE_NOT_FOUND|实例不存在".to_string())?;

    let out_dir = game.join("exports");
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("PERMISSION|创建导出目录失败|{e}"))?;
    let version = req.version.clone().unwrap_or_else(|| "1.0.0".into());
    let file_name = format!("{}-{}.mrpack", slug(&req.name), slug(&version));
    let pack_path = out_dir.join(&file_name);
    if pack_path.exists() {
        return Err("FILE_EXISTS|同名整合包已存在，请换个版本号".into());
    }

    // 先把 mods 分类：Modrinth 可查的进 index，其余进 overrides
    let mods_dir = Path::new(&inst.dirs.mods);
    let mut index_files: Vec<serde_json::Value> = Vec::new();
    let mut override_mods: Vec<(String, std::path::PathBuf)> = Vec::new();

    if let Ok(rd) = std::fs::read_dir(mods_dir) {
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_file() {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            let lower = name.to_lowercase();
            if !lower.ends_with(".jar") {
                continue;
            }
            let Some(sha1) = sha1_of(&p) else {
                override_mods.push((name, p));
                continue;
            };
            let Some(v) = version_file(&sha1) else {
                override_mods.push((name, p));
                continue;
            };
            let files = v.get("files").and_then(|f| f.as_array());
            let primary = files.and_then(|a| {
                a.iter()
                    .find(|f| f.get("primary").and_then(|p| p.as_bool()).unwrap_or(false))
                    .or_else(|| a.first())
            });
            let Some(f) = primary else {
                override_mods.push((name, p));
                continue;
            };
            let mut hashes = serde_json::Map::new();
            if let Some(h) = f.get("hashes") {
                for key in ["sha1", "sha512"] {
                    if let Some(val) = h.get(key).and_then(|x| x.as_str()) {
                        hashes.insert(key.to_string(), serde_json::Value::String(val.into()));
                    }
                }
            }
            index_files.push(serde_json::json!({
                "path": format!("mods/{name}"),
                "hashes": hashes,
                "downloads": [str_of(f, "url").unwrap_or_default()],
                "fileSize": f.get("size").and_then(|s| s.as_u64()).unwrap_or(0),
            }));
        }
    }

    // 组装 index
    let mut deps = serde_json::Map::new();
    deps.insert(
        "minecraft".into(),
        serde_json::Value::String(inst.game_version.clone()),
    );
    if let Some(loader) = &inst.loader {
        deps.insert(
            loader_dep(&loader.kind).into(),
            serde_json::Value::String(loader.version.clone()),
        );
    }
    let index = serde_json::json!({
        "formatVersion": 1,
        "game": "minecraft",
        "versionId": version,
        "name": req.name,
        "summary": format!("从 Astral 实例 {} 导出", inst.name),
        "files": index_files,
        "dependencies": deps,
    });

    let file = std::fs::File::create(&pack_path)
        .map_err(|e| format!("PERMISSION|创建整合包失败|{e}"))?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let mut override_files = 0u32;
    let index_text = serde_json::to_string_pretty(&index)
        .map_err(|e| format!("INTERNAL|序列化 index 失败|{e}"))?;
    zip.start_file("modrinth.index.json", opts)
        .map_err(|e| format!("INTERNAL|写入 index 失败|{e}"))?;
    zip.write_all(index_text.as_bytes())
        .map_err(|e| format!("INTERNAL|写入 index 失败|{e}"))?;

    // 非 Modrinth 的 mod
    for (name, p) in override_mods {
        let bytes = std::fs::read(&p).map_err(|e| format!("PERMISSION|读取失败|{e}"))?;
        zip.start_file(format!("overrides/mods/{name}"), opts)
            .map_err(|e| format!("INTERNAL|写入 overrides 失败|{e}"))?;
        zip.write_all(&bytes)
            .map_err(|e| format!("INTERNAL|写入 overrides 失败|{e}"))?;
        override_files += 1;
    }

    // config 目录（可选）
    if req.include_config {
        let config_dir = Path::new(&inst.dirs.config);
        if let Ok(rd) = std::fs::read_dir(config_dir) {
            for e in rd.flatten() {
                let p = e.path();
                if !p.is_file() {
                    continue;
                }
                let name = e.file_name().to_string_lossy().to_string();
                let bytes = std::fs::read(&p).map_err(|e| format!("PERMISSION|读取失败|{e}"))?;
                zip.start_file(format!("overrides/config/{name}"), opts)
                    .map_err(|e| format!("INTERNAL|写入 overrides 失败|{e}"))?;
                zip.write_all(&bytes)
                    .map_err(|e| format!("INTERNAL|写入 overrides 失败|{e}"))?;
                override_files += 1;
            }
        }
    }

    zip.finish()
        .map_err(|e| format!("INTERNAL|收尾失败|{e}"))?;

    let bytes = std::fs::metadata(&pack_path).map(|m| m.len()).unwrap_or(0);
    Ok(ExportResult {
        path: pack_path.to_string_lossy().to_string(),
        indexed_files: index_files.len() as u32,
        override_files,
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify() {
        assert_eq!(slug("Spunky Optimized"), "spunky-optimized");
        assert_eq!(slug("  "), "pack");
        assert_eq!(slug("a/b?c"), "a-b-c");
    }
}
