//! 整合包导出（.mrpack）。
//!
//! 规则：
//! - Modrinth 能查到的 mod 写进 `modrinth.index.json`（带 sha1/sha512/下载地址）；
//! - 查不到的 jar 与 `config/` 一起放进 `overrides/`（随包分发）；
//! - 只读实例，写到 `<gameDir>/exports/`，不覆盖已有文件。
//!
//! 导出前可以先要一份计划（`plan`），看清楚会打进去多少文件、多大、
//! 目标文件是否已存在、磁盘够不够，再决定要不要真写。

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;

const API: &str = "https://api.modrinth.com/v2";

/// overrides 最多打包多少个文件。再多玩家下载也会炸。
const MAX_OVERRIDE_ENTRIES: usize = 20_000;
/// overrides 总大小上限 2 GiB。
const MAX_OVERRIDE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// 目录递归深度上限，防软链环（虽然软链本身已经跳过）。
const MAX_DEPTH: u32 = 12;

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ExportRequest {
    pub instance_id: String,
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub include_config: bool,
    /// 是否把 resourcepacks / shaderpacks 一起打进去
    #[serde(default)]
    pub include_resources: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub path: String,
    /// 写进 index 的 Modrinth 文件数
    pub indexed_files: u32,
    /// 打进 overrides 的文件数
    pub override_files: u32,
    /// 其中来自 config/ 的数量
    pub config_files: u32,
    /// 其中来自 resourcepacks/ 与 shaderpacks/ 的数量
    pub resource_files: u32,
    pub bytes: u64,
}

/// 导出预览：只读，不写任何文件。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ExportPlan {
    pub instance_id: String,
    pub name: String,
    pub version: String,
    pub file_name: String,
    pub target_path: String,
    /// 目标文件已经存在（导出会失败，需要换版本号）
    pub exists: bool,
    /// 能对上 Modrinth 的 mod 数（写进 index，包里不含它们的字节）
    pub indexed_mods: u32,
    /// 对不上的 mod 数（随包分发）
    pub override_mods: u32,
    pub config_files: u32,
    pub resource_files: u32,
    /// 会打进 .mrpack 的文件总数（override_mods + config + resource）
    pub pack_files: u32,
    /// .mrpack 大致多大（不含 index 里要另外下载的 mod）
    pub estimated_bytes: u64,
    pub free_bytes: Option<u64>,
    /// 阻塞原因与非阻塞提示混在一起，`blocking` 说明能不能直接导
    pub issues: Vec<String>,
    pub blocking: bool,
}

fn sha1_of(p: &Path) -> Option<String> {
    crate::net::sha1_file(p).ok()
}

fn version_file(sha1: &str) -> Option<serde_json::Value> {
    crate::net::fetch_json_cached(&format!("{API}/version_file/{sha1}?algorithm=sha1"), None).ok()
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

/// 递归收集目录下的文件，返回 (zip 内路径, 磁盘路径, 字节数)。
///
/// 跳过符号链接与 Windows junction —— 否则一个指到 `C:\` 的链接就能把整块盘
/// 打进整合包里。超过条数或总字节上限直接报错，不做"悄悄截断"。
fn collect_dir(
    root: &Path,
    prefix: &str,
    max_entries: usize,
    max_bytes: u64,
) -> Result<Vec<(String, std::path::PathBuf, u64)>, String> {
    let mut out: Vec<(String, std::path::PathBuf, u64)> = Vec::new();
    if !root.is_dir() {
        return Ok(out);
    }
    let mut total = 0u64;
    let mut stack: Vec<(std::path::PathBuf, String, u32)> =
        vec![(root.to_path_buf(), String::new(), 0)];
    while let Some((dir, rel, depth)) = stack.pop() {
        if depth > MAX_DEPTH {
            return Err(format!("INVALID_VALUE|目录层级太深：{rel}"));
        }
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            let child = if rel.is_empty() {
                name
            } else {
                format!("{rel}/{name}")
            };
            let Ok(meta) = std::fs::symlink_metadata(&p) else {
                continue;
            };
            if meta.file_type().is_symlink() || crate::net::is_reparse_point(&p) {
                continue;
            }
            if meta.is_dir() {
                stack.push((p, child, depth + 1));
            } else if meta.is_file() {
                if out.len() >= max_entries {
                    return Err(format!(
                        "INVALID_VALUE|要打包的文件超过 {max_entries} 个，请只选必要的目录"
                    ));
                }
                total = total.saturating_add(meta.len());
                if total > max_bytes {
                    return Err(format!(
                        "INVALID_VALUE|要打包的内容超过 {} MiB，请只选必要的目录",
                        max_bytes / (1024 * 1024)
                    ));
                }
                out.push((format!("{prefix}/{child}"), p, meta.len()));
            }
        }
    }
    // 固定顺序，导出的包才可复现（同样的实例导两次内容一致）
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

struct Classified {
    index_files: Vec<serde_json::Value>,
    override_mods: Vec<(String, std::path::PathBuf)>,
}

/// 把 mods 分成"Modrinth 能查到的"和"只能随包带的"。
fn classify_mods(mods_dir: &Path) -> Classified {
    let mut index_files: Vec<serde_json::Value> = Vec::new();
    let mut override_mods: Vec<(String, std::path::PathBuf)> = Vec::new();

    let Ok(rd) = std::fs::read_dir(mods_dir) else {
        return Classified {
            index_files,
            override_mods,
        };
    };
    for e in rd.flatten() {
        let p = e.path();
        if !p.is_file() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        if !name.to_lowercase().ends_with(".jar") {
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
    override_mods.sort_by(|a, b| a.0.cmp(&b.0));
    Classified {
        index_files,
        override_mods,
    }
}

/// 计算目标文件名与路径（导出与预览共用，保证两边一致）
fn target_of(game: &Path, req: &ExportRequest) -> (String, String, std::path::PathBuf) {
    let version = req.version.clone().unwrap_or_else(|| "1.0.0".into());
    let file_name = format!("{}-{}.mrpack", slug(&req.name), slug(&version));
    let path = game.join("exports").join(&file_name);
    (version, file_name, path)
}

/// 导出预览：只读。看清楚了再决定要不要真的写。
pub fn plan(
    game: &Path,
    req: &ExportRequest,
    stats: &dyn Fn(&str) -> crate::instance::PlayStats,
) -> Result<ExportPlan, String> {
    let inst = crate::instance::find(game, &req.instance_id, stats)
        .ok_or_else(|| "INSTANCE_NOT_FOUND|实例不存在".to_string())?;
    let (version, file_name, target) = target_of(game, req);

    let cls = classify_mods(Path::new(&inst.dirs.mods));
    let mut issues: Vec<String> = Vec::new();
    let mut blocking = false;

    let mut config_files = 0u32;
    let mut resource_files = 0u32;
    let mut pack_bytes: u64 = cls.override_mods.iter().map(|(_, p)| file_len(p)).sum();

    if req.include_config {
        let files = collect_dir(
            Path::new(&inst.dirs.config),
            "overrides/config",
            MAX_OVERRIDE_ENTRIES,
            MAX_OVERRIDE_BYTES,
        )?;
        config_files = files.len() as u32;
        pack_bytes = pack_bytes.saturating_add(files.iter().map(|f| f.2).sum::<u64>());
    }
    if req.include_resources {
        for (dir, prefix) in [
            (&inst.dirs.resourcepacks, "overrides/resourcepacks"),
            (&inst.dirs.shaderpacks, "overrides/shaderpacks"),
        ] {
            let files = collect_dir(
                Path::new(dir),
                prefix,
                MAX_OVERRIDE_ENTRIES,
                MAX_OVERRIDE_BYTES,
            )?;
            resource_files += files.len() as u32;
            pack_bytes = pack_bytes.saturating_add(files.iter().map(|f| f.2).sum::<u64>());
        }
    }

    let exists = target.exists();
    if exists {
        issues.push(format!("{file_name} 已存在，换个版本号再导出"));
        blocking = true;
    }
    if cls.index_files.is_empty()
        && cls.override_mods.is_empty()
        && config_files == 0
        && resource_files == 0
    {
        issues.push("这个实例没有可导出的内容（没有 mod，也没勾选 config / 资源包）".into());
        blocking = true;
    }
    if !cls.override_mods.is_empty() {
        issues.push(format!(
            "{} 个 mod 在 Modrinth 上查不到，会整包带上（包会比较大）",
            cls.override_mods.len()
        ));
    }
    if req.include_config && config_files == 0 {
        issues.push("实例里没有 config 文件".into());
    }

    let free = crate::world_backup::available_space(&target);
    if let Some(free) = free {
        // 打包要占的空间 ≈ 内容大小；留 64 MiB 余量给临时文件
        if free < pack_bytes.saturating_add(64 * 1024 * 1024) {
            issues.push(format!(
                "磁盘剩余空间可能不够（需要约 {} MiB，可用 {} MiB）",
                pack_bytes / (1024 * 1024),
                free / (1024 * 1024)
            ));
            blocking = true;
        }
    }

    Ok(ExportPlan {
        instance_id: inst.id.clone(),
        name: req.name.clone(),
        version,
        file_name,
        target_path: target.to_string_lossy().to_string(),
        exists,
        indexed_mods: cls.index_files.len() as u32,
        override_mods: cls.override_mods.len() as u32,
        config_files,
        resource_files,
        pack_files: cls.override_mods.len() as u32 + config_files + resource_files,
        estimated_bytes: pack_bytes,
        free_bytes: free,
        issues,
        blocking,
    })
}

fn file_len(p: &Path) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
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
    let (version, _file_name, pack_path) = target_of(game, req);
    if pack_path.exists() {
        return Err("FILE_EXISTS|同名整合包已存在，请换个版本号".into());
    }

    let cls = classify_mods(Path::new(&inst.dirs.mods));

    // 先把要放进 overrides 的东西都列出来，顺带算总大小，好在写盘前做空间检查
    let mut overrides: Vec<(String, std::path::PathBuf)> = cls
        .override_mods
        .iter()
        .map(|(name, p)| (format!("overrides/mods/{name}"), p.clone()))
        .collect();
    let mut config_files = 0u32;
    let mut resource_files = 0u32;
    if req.include_config {
        let files = collect_dir(
            Path::new(&inst.dirs.config),
            "overrides/config",
            MAX_OVERRIDE_ENTRIES,
            MAX_OVERRIDE_BYTES,
        )?;
        config_files = files.len() as u32;
        overrides.extend(files.into_iter().map(|(n, p, _)| (n, p)));
    }
    if req.include_resources {
        for (dir, prefix) in [
            (&inst.dirs.resourcepacks, "overrides/resourcepacks"),
            (&inst.dirs.shaderpacks, "overrides/shaderpacks"),
        ] {
            let files = collect_dir(
                Path::new(dir),
                prefix,
                MAX_OVERRIDE_ENTRIES,
                MAX_OVERRIDE_BYTES,
            )?;
            resource_files += files.len() as u32;
            overrides.extend(files.into_iter().map(|(n, p, _)| (n, p)));
        }
    }
    let need: u64 = overrides.iter().map(|(_, p)| file_len(p)).sum();
    if let Some(free) = crate::world_backup::available_space(&pack_path) {
        if free < need.saturating_add(64 * 1024 * 1024) {
            return Err(format!(
                "DISK_FULL|磁盘剩余空间不足（需要约 {} MiB，可用 {} MiB）",
                need / (1024 * 1024),
                free / (1024 * 1024)
            ));
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
        "files": cls.index_files,
        "dependencies": deps,
    });

    // 先写到 .part，全部写完再原子改名 —— 中途失败不会留下半个包骗人
    let part = pack_path.with_extension("mrpack.part");
    let _ = std::fs::remove_file(&part);
    let write_result = (|| -> Result<(), String> {
        let file =
            std::fs::File::create(&part).map_err(|e| format!("PERMISSION|创建整合包失败|{e}"))?;
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        let index_text = serde_json::to_string_pretty(&index)
            .map_err(|e| format!("INTERNAL|序列化 index 失败|{e}"))?;
        zip.start_file("modrinth.index.json", opts)
            .map_err(|e| format!("INTERNAL|写入 index 失败|{e}"))?;
        zip.write_all(index_text.as_bytes())
            .map_err(|e| format!("INTERNAL|写入 index 失败|{e}"))?;

        for (name, p) in &overrides {
            let bytes = std::fs::read(p).map_err(|e| format!("PERMISSION|读取失败|{e}"))?;
            zip.start_file(name.as_str(), opts)
                .map_err(|e| format!("INTERNAL|写入 overrides 失败|{e}"))?;
            zip.write_all(&bytes)
                .map_err(|e| format!("INTERNAL|写入 overrides 失败|{e}"))?;
        }
        zip.finish().map_err(|e| format!("INTERNAL|收尾失败|{e}"))?;
        Ok(())
    })();
    if let Err(e) = write_result {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    std::fs::rename(&part, &pack_path).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        format!("PERMISSION|保存整合包失败|{e}")
    })?;

    let bytes = std::fs::metadata(&pack_path).map(|m| m.len()).unwrap_or(0);
    Ok(ExportResult {
        path: pack_path.to_string_lossy().to_string(),
        indexed_files: cls.index_files.len() as u32,
        override_files: overrides.len() as u32,
        config_files,
        resource_files,
        bytes,
    })
}

/// 导出后回读校验：把 .mrpack 重新解析一遍，确认 index 与 overrides 都能读
///
/// 只读操作，不修改任何文件。返回结构与前端展示一一对应。
pub fn verify_roundtrip(game: &Path, path: &str) -> Result<serde_json::Value, String> {
    let p = Path::new(path);
    if !p.is_file() {
        return Err("NOT_FOUND|整合包不存在".into());
    }
    // 只允许校验自己导出目录下的包，避免被当成任意文件读取器
    let out_dir = game.join("exports");
    let canon_out = out_dir
        .canonicalize()
        .map_err(|e| format!("PERMISSION|导出目录不可用|{e}"))?;
    let canon_p = p
        .canonicalize()
        .map_err(|e| format!("PERMISSION|整合包不可读|{e}"))?;
    if !canon_p.starts_with(&canon_out) {
        return Err("INVALID_PATH|只能校验 exports 目录下的整合包".into());
    }
    let index = crate::modpack::read_index(&canon_p)?;
    let mut issues: Vec<String> = Vec::new();
    if index.files.is_empty() {
        issues.push("index 里没有任何文件".into());
    }
    if index.mc_version.trim().is_empty() {
        issues.push("index 缺少 minecraft 版本".into());
    }
    for f in &index.files {
        if crate::net::check_url(&f.url).is_err() {
            issues.push(format!("下载地址不在白名单：{}", f.path));
        }
        if f.sha1.is_none() && f.sha512.is_none() {
            issues.push(format!("文件没有哈希，无法校验：{}", f.path));
        }
    }
    Ok(serde_json::json!({
        "ok": issues.is_empty(),
        "path": canon_p.to_string_lossy(),
        "name": index.name,
        "mcVersion": index.mc_version,
        "loaderKind": index.loader_kind,
        "loaderVersion": index.loader_version,
        "files": index.files.len(),
        "overrideEntries": index.override_entries,
        "totalBytes": index.total_bytes,
        "issues": issues,
    }))
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

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "astral-export-{tag}-{:x}",
            crate::time::now_nanos()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn collects_nested_files_in_stable_order() {
        let d = tmp("nested");
        std::fs::create_dir_all(d.join("jei/sub")).unwrap();
        std::fs::write(d.join("a.toml"), b"1").unwrap();
        std::fs::write(d.join("jei/b.toml"), b"22").unwrap();
        std::fs::write(d.join("jei/sub/c.toml"), b"333").unwrap();
        let files = collect_dir(&d, "overrides/config", 100, 1 << 20).unwrap();
        let names: Vec<&str> = files.iter().map(|f| f.0.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "overrides/config/a.toml",
                "overrides/config/jei/b.toml",
                "overrides/config/jei/sub/c.toml",
            ]
        );
        assert_eq!(files.iter().map(|f| f.2).sum::<u64>(), 6);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn missing_directory_collects_nothing() {
        let d = tmp("missing");
        let files = collect_dir(&d.join("nope"), "overrides/config", 10, 1 << 20).unwrap();
        assert!(files.is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn caps_entries_and_bytes() {
        let d = tmp("caps");
        for i in 0..5 {
            std::fs::write(d.join(format!("f{i}.toml")), b"0123456789").unwrap();
        }
        let too_many = collect_dir(&d, "overrides/config", 3, 1 << 20);
        assert!(too_many.is_err());
        let too_big = collect_dir(&d, "overrides/config", 100, 20);
        assert!(too_big.is_err());
        let ok = collect_dir(&d, "overrides/config", 100, 1000).unwrap();
        assert_eq!(ok.len(), 5);
        let _ = std::fs::remove_dir_all(&d);
    }
}
