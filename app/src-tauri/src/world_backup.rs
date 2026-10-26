//! 世界存档备份与恢复（C2）。
//!
//! 安全原则：
//! - **游戏运行中拒绝备份**（不做事后无法自证一致的热备份）；
//! - 备份是"把存档目录打包成 zip 存进实例的 `.astral-worlds/`"，不动原存档；
//! - 恢复默认**不覆盖**已有存档：目标存在时要求显式 `overwrite`，
//!   覆盖前先把现有存档改名留底（不删除）；
//! - 解压时逐条校验路径，防 Zip Slip；拒绝符号链接条目；
//! - 备份文件写入 sha256，恢复前校验，坏包直接拒绝；
//! - 只读预览接口不修改任何文件。

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

pub const BACKUP_DIR: &str = ".astral-worlds";
const INDEX_FILE: &str = "backups.json";
const MAX_ENTRY: u64 = 512 * 1024 * 1024;
const MAX_TOTAL: u64 = 4 * 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 50_000;
const MAX_BACKUPS: usize = 20;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WorldBackup {
    pub backup_id: String,
    pub world_id: String,
    pub world_name: String,
    pub created_at: String,
    pub bytes: u64,
    pub file_count: u64,
    /// 打包后的 zip 的 sha256（恢复前校验）
    pub sha256: String,
    /// `manual`（玩家手动）或 `auto`（以后做自动备份时用）
    pub source: String,
    pub note: String,
    /// 打包时存档的原始大小（解压后）
    pub world_bytes: u64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RestorePreview {
    pub backup: WorldBackup,
    /// 目标存档目录（已存在时提示冲突）
    pub target_dir: String,
    pub target_exists: bool,
    /// 目标目录里已有文件数（存在时才有意义）
    pub target_files: u64,
    /// 解压后需要多少字节
    pub need_bytes: u64,
    /// 目标盘可用空间（拿不到时为空）
    pub available_bytes: Option<u64>,
    /// 会覆盖多少文件
    pub overwrite_files: u64,
    /// 无法恢复的原因（能恢复时为空）
    pub blockers: Vec<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResult {
    pub world_id: String,
    pub world_name: String,
    /// 覆盖前留底的目录名（没有覆盖时为空）
    pub replaced_backup: Option<String>,
    pub restored_files: u64,
    pub bytes: u64,
}

fn backup_root(instance_dir: &Path) -> PathBuf {
    instance_dir.join(BACKUP_DIR)
}

fn index_path(instance_dir: &Path) -> PathBuf {
    backup_root(instance_dir).join(INDEX_FILE)
}

fn load_index(instance_dir: &Path) -> Vec<WorldBackup> {
    let Ok(text) = std::fs::read_to_string(index_path(instance_dir)) else {
        return Vec::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

fn save_index(instance_dir: &Path, list: &[WorldBackup]) -> Result<(), String> {
    let root = backup_root(instance_dir);
    std::fs::create_dir_all(&root).map_err(|e| format!("PERMISSION|创建备份目录失败|{e}"))?;
    let text = serde_json::to_string_pretty(list)
        .map_err(|e| format!("INTERNAL|序列化备份索引失败|{e}"))?;
    let tmp = root.join(format!("{INDEX_FILE}.part"));
    std::fs::write(&tmp, text).map_err(|e| format!("PERMISSION|写入备份索引失败|{e}"))?;
    std::fs::rename(&tmp, index_path(instance_dir))
        .map_err(|e| format!("PERMISSION|保存备份索引失败|{e}"))?;
    Ok(())
}

fn new_backup_id() -> String {
    format!("wb_{:x}", crate::time::now_unix_ms())
}

fn sanitize(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    let t = out.trim_matches('_').to_string();
    if t.is_empty() {
        "world".to_string()
    } else {
        t.chars().take(48).collect()
    }
}

pub(crate) fn dir_stats(dir: &Path) -> (u64, u64) {
    let mut bytes = 0u64;
    let mut files = 0u64;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            match std::fs::symlink_metadata(&p) {
                Ok(m) if m.is_dir() => stack.push(p),
                Ok(m) if m.is_file() => {
                    bytes = bytes.saturating_add(m.len());
                    files += 1;
                }
                _ => {}
            }
        }
    }
    (bytes, files)
}

pub(crate) fn available_space(path: &Path) -> Option<u64> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let canon = |p: &Path| -> PathBuf {
        let c = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        let s = c.to_string_lossy().to_string();
        match s.strip_prefix(r"\\?\") {
            Some(rest) => PathBuf::from(rest),
            None => PathBuf::from(s),
        }
    };
    let target = canon(path);
    let mut best: Option<(usize, u64)> = None;
    for d in disks.list() {
        let mp = canon(d.mount_point());
        if target.starts_with(&mp) {
            let depth = mp.components().count();
            if best.map(|(bd, _)| depth >= bd).unwrap_or(true) {
                best = Some((depth, d.available_space()));
            }
        }
    }
    best.map(|(_, a)| a)
}

/// 列出某个实例的全部世界备份（只读，最新的在前）
pub fn list(instance_dir: &Path) -> Vec<WorldBackup> {
    let mut list = load_index(instance_dir);
    // 只保留备份文件还在的条目，避免"列表里有、恢复时找不到"
    list.retain(|b| backup_root(instance_dir).join(format!("{}.zip", b.backup_id)).is_file());
    list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    list
}

/// 备份一个存档：打包成 zip，记录大小与 sha256
///
/// `running` 由调用方传入（实例是否有正在运行的游戏）；为 true 时直接拒绝。
pub fn create(
    instance_dir: &Path,
    saves_dir: &Path,
    world_id: &str,
    note: &str,
    running: bool,
    cancel: crate::net::CancelRef<'_>,
    mut on: impl FnMut(u32, u32),
) -> Result<WorldBackup, String> {
    if running {
        return Err("INSTANCE_BUSY|游戏正在运行，无法保证存档一致，请先退出游戏".into());
    }
    let world = crate::worlds::resolve(saves_dir, world_id)
        .ok_or_else(|| "NOT_FOUND|找不到这个存档".to_string())?;
    let world_name = world
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "world".into());

    let (world_bytes, _file_count) = dir_stats(&world);
    // 空间检查：压缩后一般更小，但保守按原始大小要空间
    if let Some(avail) = available_space(instance_dir) {
        if avail < world_bytes.saturating_add(32 * 1024 * 1024) {
            return Err(format!(
                "DISK_FULL|磁盘空间不足|存档约 {} MB，可用 {} MB",
                world_bytes / 1024 / 1024,
                avail / 1024 / 1024
            ));
        }
    }

    let root = backup_root(instance_dir);
    std::fs::create_dir_all(&root).map_err(|e| format!("PERMISSION|创建备份目录失败|{e}"))?;
    let backup_id = new_backup_id();
    let zip_path = root.join(format!("{backup_id}.zip"));
    let tmp_zip = root.join(format!("{backup_id}.zip.part"));

    let file = std::fs::File::create(&tmp_zip)
        .map_err(|e| format!("PERMISSION|创建备份文件失败|{e}"))?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let mut stack: Vec<PathBuf> = vec![world.clone()];
    let mut total = 0u64;
    let mut written = 0u32;
    let mut buf = vec![0u8; 64 * 1024];
    while let Some(dir) = stack.pop() {
        if cancel() {
            let _ = std::fs::remove_file(&tmp_zip);
            return Err("CANCELLED|已取消".into());
        }
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            if cancel() {
                let _ = std::fs::remove_file(&tmp_zip);
                return Err("CANCELLED|已取消".into());
            }
            let p = e.path();
            let Ok(meta) = std::fs::symlink_metadata(&p) else {
                continue;
            };
            if meta.is_dir() {
                stack.push(p);
                continue;
            }
            if !meta.is_file() {
                // 符号链接等一律跳过：备份里不带指针
                continue;
            }
            if meta.len() > MAX_ENTRY {
                let _ = std::fs::remove_file(&tmp_zip);
                return Err(format!(
                    "INVALID_VALUE|存档内单个文件过大，已中止：{}",
                    p.display()
                ));
            }
            total = total.saturating_add(meta.len());
            if total > MAX_TOTAL {
                let _ = std::fs::remove_file(&tmp_zip);
                return Err("INVALID_VALUE|存档体积异常，已中止".into());
            }
            let rel = match p.strip_prefix(&world) {
                Ok(r) => r.to_string_lossy().replace('\\', "/"),
                Err(_) => continue,
            };
            if rel.is_empty() {
                continue;
            }
            zip.start_file(&rel, opts)
                .map_err(|e| format!("INTERNAL|写入备份失败|{e}"))?;
            let mut src = match std::fs::File::open(&p) {
                Ok(f) => f,
                Err(_) => continue,
            };
            loop {
                let n = src
                    .read(&mut buf)
                    .map_err(|e| format!("PERMISSION|读取存档失败|{e}"))?;
                if n == 0 {
                    break;
                }
                zip.write_all(&buf[..n])
                    .map_err(|e| format!("PERMISSION|写入备份失败|{e}"))?;
            }
            written += 1;
            on(written, 0);
        }
    }
    zip.finish()
        .map_err(|e| format!("INTERNAL|收尾备份失败|{e}"))?;
    std::fs::rename(&tmp_zip, &zip_path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_zip);
        format!("PERMISSION|保存备份失败|{e}")
    })?;

    let sha = crate::net::sha256_file(&zip_path).unwrap_or_default();
    let bytes = std::fs::metadata(&zip_path).map(|m| m.len()).unwrap_or(0);
    let record = WorldBackup {
        backup_id,
        world_id: world_id.to_string(),
        world_name,
        created_at: crate::time::now_iso(),
        bytes,
        file_count: written as u64,
        sha256: sha,
        source: "manual".to_string(),
        note: note.to_string(),
        world_bytes,
    };

    let mut list = load_index(instance_dir);
    list.push(record.clone());
    // 只保留最近 MAX_BACKUPS 条索引，旧的 zip 不自动删（交给玩家决定）
    if list.len() > MAX_BACKUPS {
        let sorted: Vec<usize> = {
            let mut idx: Vec<usize> = (0..list.len()).collect();
            idx.sort_by(|a, b| list[*a].created_at.cmp(&list[*b].created_at));
            idx
        };
        let drop: std::collections::HashSet<usize> =
            sorted.into_iter().take(list.len() - MAX_BACKUPS).collect();
        let mut kept = Vec::new();
        for (i, b) in list.into_iter().enumerate() {
            if !drop.contains(&i) {
                kept.push(b);
            }
        }
        list = kept;
    }
    save_index(instance_dir, &list)?;
    Ok(record)
}

fn find(list: &[WorldBackup], backup_id: &str) -> Option<WorldBackup> {
    list.iter().find(|b| b.backup_id == backup_id).cloned()
}

/// 恢复前预览（只读，不修改任何文件）
pub fn preview_restore(
    instance_dir: &Path,
    saves_dir: &Path,
    backup_id: &str,
) -> Result<RestorePreview, String> {
    let list = load_index(instance_dir);
    let backup = find(&list, backup_id).ok_or_else(|| "NOT_FOUND|找不到这个备份".to_string())?;
    let zip_path = backup_root(instance_dir).join(format!("{backup_id}.zip"));
    let mut blockers = Vec::new();
    if !zip_path.is_file() {
        blockers.push("备份文件已丢失".to_string());
    } else {
        let sha = crate::net::sha256_file(&zip_path).unwrap_or_default();
        if !backup.sha256.is_empty() && sha != backup.sha256 {
            blockers.push("备份文件校验失败，可能已损坏".to_string());
        }
    }
    let target = saves_dir.join(sanitize(&backup.world_name));
    let target_exists = target.is_dir();
    let (target_files, _) = if target_exists {
        dir_stats(&target)
    } else {
        (0, 0)
    };
    let avail = available_space(instance_dir);
    if let Some(a) = avail {
        if a < backup.world_bytes.saturating_add(16 * 1024 * 1024) {
            blockers.push(format!(
                "磁盘空间不足：需要约 {} MB，可用 {} MB",
                backup.world_bytes / 1024 / 1024,
                a / 1024 / 1024
            ));
        }
    }
    Ok(RestorePreview {
        target_dir: target.to_string_lossy().to_string(),
        target_exists,
        target_files,
        need_bytes: backup.world_bytes,
        available_bytes: avail,
        overwrite_files: if target_exists { target_files } else { 0 },
        backup,
        blockers,
    })
}

/// 恢复：目标存在时必须 `overwrite = true`；覆盖前把现有存档改名留底
pub fn restore(
    instance_dir: &Path,
    saves_dir: &Path,
    backup_id: &str,
    overwrite: bool,
    cancel: crate::net::CancelRef<'_>,
    mut on: impl FnMut(u32, u32),
) -> Result<RestoreResult, String> {
    let preview = preview_restore(instance_dir, saves_dir, backup_id)?;
    if !preview.blockers.is_empty() {
        return Err(format!(
            "VERIFY_FAILED|无法恢复这个备份|{}",
            preview.blockers.join("；")
        ));
    }
    if preview.target_exists && !overwrite {
        return Err(format!(
            "FILE_EXISTS|目标存档已存在：{}|确认覆盖后会先留一份副本",
            preview.backup.world_name
        ));
    }

    let root = backup_root(instance_dir);
    let zip_path = root.join(format!("{backup_id}.zip"));
    let target = PathBuf::from(&preview.target_dir);
    std::fs::create_dir_all(saves_dir).map_err(|e| format!("PERMISSION|创建存档目录失败|{e}"))?;

    // 覆盖前留底：改名而不是删除
    let mut replaced: Option<String> = None;
    if target.exists() {
        let stamp = crate::time::now_stamp();
        let kept = root.join(format!("replaced-{stamp}-{}", sanitize(&preview.backup.world_name)));
        std::fs::create_dir_all(&root).map_err(|e| format!("PERMISSION|创建备份目录失败|{e}"))?;
        std::fs::rename(&target, &kept)
            .map_err(|e| format!("PERMISSION|留底现有存档失败|{e}"))?;
        replaced = Some(kept.to_string_lossy().to_string());
    }

    let staging = saves_dir.join(format!(".restore-{}", crate::time::now_nanos()));
    let outcome = (|| -> Result<(u64, u64), String> {
        std::fs::create_dir_all(&staging).map_err(|e| format!("PERMISSION|创建临时目录失败|{e}"))?;
        let file = std::fs::File::open(&zip_path)
            .map_err(|e| format!("PERMISSION|打不开备份|{e}"))?;
        let mut zip = zip::ZipArchive::new(file)
            .map_err(|e| format!("INTERNAL|备份不是有效的 zip|{e}"))?;
        if zip.len() > MAX_ENTRIES {
            return Err("INVALID_VALUE|备份条目过多，已拒绝".into());
        }
        let mut written = 0u64;
        let mut total = 0u64;
        for i in 0..zip.len() {
            if cancel() {
                return Err("CANCELLED|已取消".into());
            }
            let mut entry = match zip.by_index(i) {
                Ok(e) => e,
                Err(_) => continue,
            };
            if let Some(mode) = entry.unix_mode() {
                if mode & 0o170000 == 0o120000 {
                    return Err("INVALID_PATH|备份内含符号链接条目，已拒绝".into());
                }
            }
            if entry.size() > MAX_ENTRY {
                return Err("INVALID_VALUE|备份内单个文件过大，已拒绝".into());
            }
            let name = entry.name().map(|c| c.to_string()).unwrap_or_default();
            if name.is_empty() || name.ends_with('/') {
                continue;
            }
            // 防 Zip Slip：只允许普通相对路径
            let rel = safe_rel(&name)?;
            let dest = staging.join(&rel);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("PERMISSION|创建目录失败|{e}"))?;
            }
            let mut out = std::fs::File::create(&dest)
                .map_err(|e| format!("PERMISSION|写入存档失败|{e}"))?;
            let copied = std::io::copy(&mut entry, &mut out)
                .map_err(|e| format!("PERMISSION|解压失败|{e}"))?;
            drop(out);
            total = total.saturating_add(copied);
            if total > MAX_TOTAL {
                return Err("INVALID_VALUE|备份解压体积异常，已中止".into());
            }
            written += 1;
            on(written as u32, 0);
        }
        Ok((written, total))
    })();

    match outcome {
        Ok((written, total)) => {
            if let Err(e) = std::fs::rename(&staging, &target) {
                let _ = std::fs::remove_dir_all(&staging);
                // 尽力把留底的目录还原回去，避免玩家丢存档
                if let Some(k) = &replaced {
                    let _ = std::fs::rename(k, &target);
                }
                return Err(format!("PERMISSION|恢复存档失败|{e}"));
            }
            Ok(RestoreResult {
                world_id: preview.backup.world_id,
                world_name: preview.backup.world_name,
                replaced_backup: replaced,
                restored_files: written,
                bytes: total,
            })
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            if let Some(k) = &replaced {
                let _ = std::fs::rename(k, &target);
            }
            Err(e)
        }
    }
}

fn safe_rel(name: &str) -> Result<PathBuf, String> {
    if name.contains('\\') || name.contains(':') {
        return Err(format!("INVALID_PATH|备份内路径不合法：{name}"));
    }
    let p = Path::new(name);
    if p.is_absolute() {
        return Err(format!("INVALID_PATH|备份内含绝对路径：{name}"));
    }
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            _ => return Err(format!("INVALID_PATH|备份内含上跳路径：{name}")),
        }
    }
    if out.as_os_str().is_empty() {
        return Err("INVALID_PATH|备份内路径为空".into());
    }
    Ok(out)
}

/// 删除一个备份（必须显式确认；只删 zip 与索引条目）
pub fn delete(instance_dir: &Path, backup_id: &str, confirm: bool) -> Result<u64, String> {
    if !confirm {
        return Err("UNSUPPORTED|删除备份需要明确确认".into());
    }
    let mut list = load_index(instance_dir);
    let before = list.len();
    let Some(rec) = find(&list, backup_id) else {
        return Err("NOT_FOUND|找不到这个备份".into());
    };
    let zip_path = backup_root(instance_dir).join(format!("{backup_id}.zip"));
    let bytes = std::fs::metadata(&zip_path).map(|m| m.len()).unwrap_or(0);
    if zip_path.is_file() {
        std::fs::remove_file(&zip_path).map_err(|e| format!("PERMISSION|删除备份失败|{e}"))?;
    }
    list.retain(|b| b.backup_id != rec.backup_id);
    if list.len() == before {
        return Err("INTERNAL|备份索引没有变化".into());
    }
    save_index(instance_dir, &list)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_keeps_safe_chars() {
        assert_eq!(sanitize("我的世界 World_1"), "我的世界_World_1");
        assert_eq!(sanitize("../../etc"), "_etc");
        assert_eq!(sanitize(""), "world");
    }

    #[test]
    fn safe_rel_blocks_zip_slip() {
        assert!(safe_rel("level.dat").is_ok());
        assert!(safe_rel("region/r.0.0.mca").is_ok());
        assert!(safe_rel("../evil").is_err());
        assert!(safe_rel("a/../../b").is_err());
        assert!(safe_rel("/abs").is_err());
        assert!(safe_rel("C:/win").is_err());
        assert!(safe_rel("a\\b").is_err());
    }

    #[test]
    fn backup_id_prefix() {
        assert!(new_backup_id().starts_with("wb_"));
    }
}
