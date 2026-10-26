//! 下载缓存（契约 V2.5 A2）。
//!
//! 解决的问题：同一个 mod / 库文件在多个实例里各下一遍，既慢又费流量。
//! 这里做一个按内容哈希寻址的缓存目录：
//!
//! - 只缓存**校验通过**的文件（调用方给出 SHA1 或 SHA512 才进缓存）；
//! - 命中时直接复制到目标位置，不联网；
//! - 缓存有总量上限，超了按最久未使用淘汰；
//! - 缓存目录里的文件永远不被就地修改，复制出去的是独立副本。
//!
//! 放在 `{数据目录}/cache/objects/<哈希前两位>/<哈希>`，跨实例共享。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// 默认缓存上限：1 GiB。超过就按 LRU 淘汰。
pub const DEFAULT_BUDGET_BYTES: u64 = 1024 * 1024 * 1024;

/// 数据目录：与 tasks.json / sessions.json 同一套规则，便于隔离测试。
fn data_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("ASTRAL_DATA_DIR") {
        return Some(PathBuf::from(dir));
    }
    let base = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(base).join("Astral"))
}

pub fn cache_root() -> Option<PathBuf> {
    data_dir().map(|d| d.join("cache").join("objects"))
}

/// 缓存键：优先用 SHA512（更长更安全），退而用 SHA1。
fn key_for(sha1: Option<&str>, sha512: Option<&str>) -> Option<String> {
    let raw = sha512
        .filter(|s| s.len() >= 32)
        .or_else(|| sha1.filter(|s| s.len() >= 32))?;
    let k = raw.trim().to_ascii_lowercase();
    if !k.chars().all(|c| c.is_ascii_hexdigit()) || k.len() < 32 {
        return None;
    }
    Some(k)
}

fn object_path(key: &str) -> Option<PathBuf> {
    let root = cache_root()?;
    Some(root.join(&key[..2]).join(key))
}

/// 缓存里有没有这个文件；有就把访问时间刷新一下（LRU 依据）。
#[allow(dead_code)]
pub fn contains(sha1: Option<&str>, sha512: Option<&str>) -> bool {
    let Some(key) = key_for(sha1, sha512) else {
        return false;
    };
    let Some(p) = object_path(&key) else {
        return false;
    };
    p.is_file()
}

/// 从缓存恢复一份到 `dest`。成功返回字节数。
///
/// 复制而不是硬链接：避免玩家就地改文件时把缓存也改坏。
pub fn restore(sha1: Option<&str>, sha512: Option<&str>, dest: &Path) -> Option<u64> {
    let key = key_for(sha1, sha512)?;
    let src = object_path(&key)?;
    if !src.is_file() {
        return None;
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).ok()?;
    }
    let tmp = PathBuf::from(format!("{}.part", dest.to_string_lossy()));
    std::fs::copy(&src, &tmp).ok()?;
    // 顺手把访问时间往后推，淘汰时优先留下常用文件
    touch(&src);
    if std::fs::rename(&tmp, dest).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return None;
    }
    std::fs::metadata(dest).ok().map(|m| m.len())
}

/// 把一个校验通过的文件放进缓存。已存在就不重复写。
pub fn store(sha1: Option<&str>, sha512: Option<&str>, src: &Path) -> bool {
    let Some(key) = key_for(sha1, sha512) else {
        return false;
    };
    let Some(dst) = object_path(&key) else {
        return false;
    };
    if dst.is_file() {
        touch(&dst);
        return true;
    }
    let Some(parent) = dst.parent() else {
        return false;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return false;
    }
    let tmp = PathBuf::from(format!("{}.part", dst.to_string_lossy()));
    if std::fs::copy(src, &tmp).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return false;
    }
    if std::fs::rename(&tmp, &dst).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return false;
    }
    true
}

/// 刷新文件的修改时间，当作"最近使用"。
fn touch(path: &Path) {
    if let Ok(f) = std::fs::OpenOptions::new().append(true).open(path) {
        let _ = f.set_modified(std::time::SystemTime::now());
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheStats {
    pub entries: u64,
    pub bytes: u64,
    pub budget_bytes: u64,
    /// 缓存目录是否可用（拿不到数据目录时为 false）
    pub available: bool,
}

/// 统计缓存占用，不修改任何文件。
pub fn stats(budget: u64) -> CacheStats {
    let Some(root) = cache_root() else {
        return CacheStats {
            entries: 0,
            bytes: 0,
            budget_bytes: budget,
            available: false,
        };
    };
    let mut entries = 0u64;
    let mut bytes = 0u64;
    if let Ok(dirs) = std::fs::read_dir(&root) {
        for d in dirs.flatten() {
            let Ok(files) = std::fs::read_dir(d.path()) else {
                continue;
            };
            for f in files.flatten() {
                if let Ok(m) = f.metadata() {
                    if m.is_file() {
                        entries += 1;
                        bytes = bytes.saturating_add(m.len());
                    }
                }
            }
        }
    }
    CacheStats {
        entries,
        bytes,
        budget_bytes: budget,
        available: true,
    }
}

/// 按最久未使用淘汰，直到占用不超过 `budget`。返回清掉的字节数。
///
/// 不会删任何实例目录里的文件，只动缓存目录本身。
pub fn enforce_budget(budget: u64) -> u64 {
    let Some(root) = cache_root() else {
        return 0;
    };
    let mut items: Vec<(u64, u64, PathBuf)> = Vec::new();
    let mut total = 0u64;
    if let Ok(dirs) = std::fs::read_dir(&root) {
        for d in dirs.flatten() {
            let Ok(files) = std::fs::read_dir(d.path()) else {
                continue;
            };
            for f in files.flatten() {
                let Ok(m) = f.metadata() else { continue };
                if !m.is_file() {
                    continue;
                }
                let used = m
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                total = total.saturating_add(m.len());
                items.push((used, m.len(), f.path()));
            }
        }
    }
    if total <= budget {
        return 0;
    }
    // 最旧的先删
    items.sort_by_key(|(used, _, _)| *used);
    let mut freed = 0u64;
    for (_, len, path) in items {
        if total <= budget {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(len);
            freed = freed.saturating_add(len);
        }
    }
    freed
}

/// 清空整个缓存。返回释放的字节数。
///
/// 这是不可逆操作，调用方（命令层）必须要求用户明确确认。
pub fn clear() -> u64 {
    let Some(root) = cache_root() else {
        return 0;
    };
    let before = stats(DEFAULT_BUDGET_BYTES).bytes;
    let _ = std::fs::remove_dir_all(&root);
    before
}

/// 清理游戏目录里过期的 `.part` 残留（上次崩溃/取消留下的半截文件）。
///
/// 只删 `*.part` 和 `*.part.json`，不碰任何正式文件。
pub fn clean_stale_partials(root: &Path, older_than_days: u64) -> (u64, u64) {
    let cutoff = std::time::SystemTime::now()
        .checked_sub(std::time::Duration::from_secs(older_than_days * 86400))
        .unwrap_or(std::time::UNIX_EPOCH);
    let mut removed = 0u64;
    let mut freed = 0u64;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            let Ok(md) = e.metadata() else { continue };
            if md.is_dir() {
                // 跳过实例的游戏资源，只扫下载残留
                stack.push(p);
                continue;
            }
            let name = p
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            let is_part = name.ends_with(".part") || name.ends_with(".part.json");
            if !is_part {
                continue;
            }
            let old = md.modified().map(|t| t < cutoff).unwrap_or(false);
            if old && std::fs::remove_file(&p).is_ok() {
                removed += 1;
                freed = freed.saturating_add(md.len());
            }
        }
    }
    (removed, freed)
}

/// 各类目录的占用快照，给"存储管理"页面用。
pub fn disk_usage(game_dir: &Path) -> serde_json::Value {
    fn size_of(p: &Path) -> u64 {
        let mut total = 0u64;
        let mut stack = vec![p.to_path_buf()];
        while let Some(d) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&d) else {
                continue;
            };
            for e in entries.flatten() {
                let Ok(md) = e.metadata() else { continue };
                if md.is_dir() {
                    stack.push(e.path());
                } else {
                    total = total.saturating_add(md.len());
                }
            }
        }
        total
    }

    let mut out: HashMap<String, u64> = HashMap::new();
    for (name, rel) in [
        ("assets", "assets"),
        ("libraries", "libraries"),
        ("versions", "versions"),
        ("instances", "instances"),
        ("logs", "logs"),
    ] {
        let p = game_dir.join(rel);
        if p.exists() {
            out.insert(name.to_string(), size_of(&p));
        }
    }
    let cache = stats(DEFAULT_BUDGET_BYTES);
    serde_json::json!({
        "gameDir": game_dir.to_string_lossy(),
        "folders": out,
        "cache": cache,
        "totalBytes": out.values().sum::<u64>() + cache.bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_key_prefers_sha512() {
        let a = key_for(Some("aa"), Some("bb"));
        assert!(a.is_none(), "太短的不算");
        let k = key_for(Some(&"1".repeat(40)), Some(&"2".repeat(128))).unwrap();
        assert_eq!(k.len(), 128);
        let k2 = key_for(Some(&"A".repeat(40)), None).unwrap();
        assert_eq!(k2, "a".repeat(40), "统一小写");
    }

    #[test]
    fn rejects_non_hex_keys() {
        assert!(key_for(Some(&"z".repeat(40)), None).is_none());
    }
}
