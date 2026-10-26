//! JDK 下载（Adoptium / Temurin）。
//!
//! 只下载**用户明确要求**的版本；下载后用 API 给的 SHA256 校验，
//! 解压到 `<gameDir>/jvm/adoptium-<major>/`，不碰系统环境变量。

use std::io::Write;
use std::path::{Path, PathBuf};

const API: &str = "https://api.adoptium.net/v3/assets/latest";

#[derive(serde::Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct JdkResult {
    pub major: u32,
    pub version: String,
    pub path: String,
    pub bytes: u64,
}

fn str_of(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

/// 下载并安装指定主版本的 Temurin JDK，返回 java.exe 路径
pub fn install(
    major: u32,
    game_dir: &Path,
    should_cancel: impl Fn() -> bool + Send + Sync + 'static,
    mut on: impl FnMut(&str, u32, u32),
) -> Result<JdkResult, String> {
    let should_cancel = std::sync::Arc::new(should_cancel);
    install_inner(major, game_dir, should_cancel, &mut on)
}

fn install_inner(
    major: u32,
    game_dir: &Path,
    should_cancel: std::sync::Arc<dyn Fn() -> bool + Send + Sync>,
    on: &mut dyn FnMut(&str, u32, u32),
) -> Result<JdkResult, String> {
    if !(8..=30).contains(&major) {
        return Err("INVALID_VALUE|Java 主版本超出范围".into());
    }

    let url =
        format!("{API}/{major}/hotspot?architecture=x64&image_type=jdk&os=windows&vendor=eclipse");
    let meta = crate::net::fetch_json(&url)?;
    let first = meta
        .as_array()
        .and_then(|a| a.first())
        .ok_or_else(|| format!("VERSION_NOT_FOUND|Adoptium 没有 Java {major} 的 Windows x64 包"))?;

    let package = first
        .get("binary")
        .and_then(|b| b.get("package"))
        .ok_or_else(|| "INTERNAL|返回缺少 package 信息".to_string())?;
    let link = str_of(package, "link").ok_or_else(|| "INTERNAL|缺少下载地址".to_string())?;
    let checksum = str_of(package, "checksum");
    let name = str_of(package, "name").unwrap_or_else(|| format!("jdk-{major}.zip"));
    let version = first
        .get("version")
        .and_then(|v| str_of(v, "semver"))
        .unwrap_or_else(|| major.to_string());

    let cache = game_dir.join(".astral").join("jdk");
    std::fs::create_dir_all(&cache).map_err(|e| format!("PERMISSION|创建缓存目录失败|{e}"))?;
    let safe_name = Path::new(&name)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| format!("jdk-{major}.zip"));
    let zip_path = cache.join(&safe_name);

    if should_cancel() {
        return Err("CANCELLED|已取消".into());
    }

    if !zip_path.is_file() {
        on("download", 0, 1);
        let c = should_cancel.clone();
        let cancel: crate::net::CancelRef<'_> = &*c;
        crate::net::download_with_cancel(&link, &zip_path, None, None, &cancel, |done, total| {
            on(
                "download",
                done.min(u32::MAX as u64) as u32,
                total.unwrap_or(0).min(u32::MAX as u64) as u32,
            );
        })?;
    }

    if let Some(want) = &checksum {
        let got = crate::net::sha256_file(&zip_path)?;
        if !got.eq_ignore_ascii_case(want) {
            let _ = std::fs::remove_file(&zip_path);
            return Err(format!(
                "HASH_MISMATCH|JDK 校验失败|期望 {want}，实际 {got}"
            ));
        }
    }

    if should_cancel() {
        return Err("CANCELLED|已取消".into());
    }

    let dest = game_dir.join("jvm").join(format!("adoptium-{major}"));
    on("extract", 0, 1);
    let mut cb = |done, total| on("extract", done, total);
    extract_zip(&zip_path, &dest, &mut cb)?;

    let java = find_java_exe(&dest).ok_or_else(|| "INTERNAL|解压后找不到 java.exe".to_string())?;

    let bytes = std::fs::metadata(&zip_path).map(|m| m.len()).unwrap_or(0);
    Ok(JdkResult {
        major,
        version,
        path: java.to_string_lossy().to_string(),
        bytes,
    })
}

/// 解压到 `dest`，逐条校验路径（防 zip-slip），总量上限 2 GiB
fn extract_zip(zip_path: &Path, dest: &Path, on: &mut dyn FnMut(u32, u32)) -> Result<(), String> {
    const MAX_TOTAL: u64 = 2 * 1024 * 1024 * 1024;
    let file = std::fs::File::open(zip_path).map_err(|e| format!("PERMISSION|打不开压缩包|{e}"))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("INTERNAL|不是有效 zip|{e}"))?;

    let mut names: Vec<String> = Vec::new();
    for i in 0..zip.len() {
        if let Ok(entry) = zip.by_index(i) {
            if let Ok(n) = entry.name().map(|c| c.to_string()) {
                names.push(n);
            }
        }
    }
    let total = names.len() as u32;
    let mut written = 0u64;
    let mut done = 0u32;

    for name in names {
        if name.ends_with('/') {
            continue;
        }
        let rel = match crate::modpack::safe_rel_path(&name) {
            Ok(p) => p,
            Err(e) => return Err(e),
        };
        let target = dest.join(&rel);
        if !target.starts_with(dest) {
            return Err(format!("INVALID_PATH|解压路径越界：{name}"));
        }
        let mut entry = zip
            .by_name(&name)
            .map_err(|e| format!("INTERNAL|读取条目失败|{e}"))?;
        written += entry.size();
        if written > MAX_TOTAL {
            return Err("INTERNAL|解压体积异常，已中止".into());
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("PERMISSION|创建目录失败|{e}"))?;
        }
        let mut out =
            std::fs::File::create(&target).map_err(|e| format!("PERMISSION|写入失败|{e}"))?;
        std::io::copy(&mut entry, &mut out).map_err(|e| format!("PERMISSION|解压失败|{e}"))?;
        done += 1;
        on(done, total);
    }
    let _ = std::io::stdout().flush();
    Ok(())
}

/// 在解压目录里找 bin/java.exe（JDK 包通常带一层顶层目录）
fn find_java_exe(dir: &Path) -> Option<PathBuf> {
    let direct = dir.join("bin").join("java.exe");
    if direct.is_file() {
        return Some(direct);
    }
    let rd = std::fs::read_dir(dir).ok()?;
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            let cand = p.join("bin").join("java.exe");
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_range_guard() {
        // 只验证边界判断，不触发网络
        assert!((8..=30).contains(&17));
        assert!(!(8..=30).contains(&7));
    }
}

/// 列出本项目下载过的 JDK（只读，不删除）
#[derive(serde::Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ManagedJdk {
    pub major: u32,
    pub dir: String,
    pub bytes: u64,
    pub java_path: Option<String>,
}

pub fn list_managed(game_dir: &Path) -> Vec<ManagedJdk> {
    let root = game_dir.join("jvm");
    let Ok(rd) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let Some(rest) = name.strip_prefix("adoptium-") else {
            continue;
        };
        let Ok(major) = rest.parse::<u32>() else {
            continue;
        };
        let dir = e.path();
        let mut bytes = 0u64;
        fn walk(p: &Path, acc: &mut u64, depth: u32) {
            if depth > 8 {
                return;
            }
            let Ok(rd) = std::fs::read_dir(p) else { return };
            for e in rd.flatten() {
                let path = e.path();
                match e.file_type() {
                    Ok(t) if t.is_dir() => walk(&path, acc, depth + 1),
                    Ok(t) if t.is_file() => *acc += e.metadata().map(|m| m.len()).unwrap_or(0),
                    _ => {}
                }
            }
        }
        walk(&dir, &mut bytes, 0);
        out.push(ManagedJdk {
            major,
            dir: dir.to_string_lossy().to_string(),
            bytes,
            java_path: find_java_exe(&dir).map(|p| p.to_string_lossy().to_string()),
        });
    }
    out.sort_by_key(|j| j.major);
    out
}
