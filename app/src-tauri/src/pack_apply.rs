//! 整合包更新执行：把 `pack_update::plan` 的结果真正落地。
//!
//! 流程是「先备份 → 再改 → 失败整批回滚」：
//! 1. 把计划里会新增 / 改动 / 删除的文件先复制到实例内的备份目录；
//! 2. 下载新增与变更的文件，删掉新版本里已经没有的文件；
//! 3. 解压新版本的 overrides（会覆盖 config），最后写新的来源记录。
//!
//! 安全要点：
//! - 计划必须先由 `pack_update::plan` 生成；`plan_id` 是计划内容的指纹，
//!   对不上说明实例或整合包已经变了，要求玩家重新预览，绝不用旧计划硬套。
//! - 所有落盘路径都过 `modpack::safe_rel_path` 与 `net::ensure_within`。
//! - 只动计划里列出的文件，不碰存档，也不会清空整个 mods 目录。
//! - 备份留在实例里，更新完还能回滚。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 备份目录（在实例目录下）
pub const BACKUP_DIR: &str = ".astral-pack-update";
/// 历史索引文件名
pub const INDEX_FILE: &str = "history.json";
/// 保留几份可回滚的更新
pub const MAX_HISTORY: usize = 5;
/// 一次更新最多备份多少个文件
const MAX_BACKUP_FILES: usize = 20_000;
/// 一次更新最多备份多少字节
const MAX_BACKUP_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// 更新进度回调：阶段、当前文件、已完成文件数、总文件数、已完成字节、总字节
pub type Progress<'a> = dyn FnMut(&str, &str, u32, u32, u64, u64) + 'a;

/// 执行一次整合包更新所需的参数
pub struct ApplyRequest<'a> {
    /// 接口根地址（线上是 `pack_update::API`，自检传本地服务器）
    pub base: &'a str,
    pub instance_id: &'a str,
    pub version_id: &'a str,
    /// `pack_update::plan` 给出的计划指纹
    pub plan_id: &'a str,
    /// 实例有没有在跑游戏；运行中拒绝，避免改到一半的文件
    pub running: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PackUpdateRecord {
    pub batch_id: String,
    pub instance_id: String,
    pub from_version_id: String,
    pub from_version_name: String,
    pub to_version_id: String,
    pub to_version_name: String,
    pub created_at: String,
    /// 备份下来的旧文件（相对实例目录）
    pub backed_up: Vec<String>,
    /// 本次新增的文件（回滚时删掉）
    pub added: Vec<String>,
    /// 本次删除的文件（回滚时从备份恢复）
    pub removed: Vec<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PackUpdateOutcome {
    pub batch_id: String,
    pub instance_id: String,
    pub from_version_id: String,
    pub to_version_id: String,
    pub added: u32,
    pub changed: u32,
    pub removed: u32,
    pub bytes: u64,
    pub backup_dir: String,
}

fn backup_root(instance_dir: &Path) -> PathBuf {
    instance_dir.join(BACKUP_DIR)
}

fn history_path(instance_dir: &Path) -> PathBuf {
    backup_root(instance_dir).join(INDEX_FILE)
}

fn load_history(instance_dir: &Path) -> Vec<PackUpdateRecord> {
    let Ok(text) = std::fs::read_to_string(history_path(instance_dir)) else {
        return Vec::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

fn save_history(instance_dir: &Path, list: &[PackUpdateRecord]) -> Result<(), String> {
    let root = backup_root(instance_dir);
    std::fs::create_dir_all(&root).map_err(|e| format!("PERMISSION|创建备份目录失败|{e}"))?;
    let text = serde_json::to_string_pretty(list)
        .map_err(|e| format!("INTERNAL|序列化更新历史失败|{e}"))?;
    let tmp = root.join(format!("{INDEX_FILE}.part"));
    std::fs::write(&tmp, text).map_err(|e| format!("PERMISSION|写入更新历史失败|{e}"))?;
    std::fs::rename(&tmp, history_path(instance_dir))
        .map_err(|e| format!("PERMISSION|保存更新历史失败|{e}"))?;
    Ok(())
}

/// 可回滚的更新记录（最新的在前）
pub fn history(instance_dir: &Path) -> Vec<PackUpdateRecord> {
    let mut list = load_history(instance_dir);
    list.retain(|r| backup_root(instance_dir).join(&r.batch_id).is_dir());
    list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    list
}

/// 相对路径 → 实例内的绝对路径，越界或指向符号链接一律拒绝
fn safe_join(instance_dir: &Path, rel: &str) -> Result<PathBuf, String> {
    let rel_path = crate::modpack::safe_rel_path(rel)?;
    crate::net::ensure_within(instance_dir, &instance_dir.join(rel_path))
}

/// 把一个文件复制进备份目录（保持相对路径结构）
fn backup_one(instance_dir: &Path, batch_dir: &Path, rel: &str) -> Result<u64, String> {
    let src = safe_join(instance_dir, rel)?;
    if !src.is_file() {
        return Ok(0);
    }
    if crate::net::is_reparse_point(&src) {
        return Err(format!("INVALID_PATH|{rel} 是符号链接，已拒绝备份"));
    }
    let dst = batch_dir
        .join("files")
        .join(crate::modpack::safe_rel_path(rel).map_err(|e| format!("{e}（备份 {rel}）"))?);
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("PERMISSION|创建备份目录失败|{e}"))?;
    }
    let bytes = std::fs::copy(&src, &dst).map_err(|e| format!("PERMISSION|备份 {rel} 失败|{e}"))?;
    Ok(bytes)
}

fn restore_one(instance_dir: &Path, batch_dir: &Path, rel: &str) -> Result<(), String> {
    let src = batch_dir
        .join("files")
        .join(crate::modpack::safe_rel_path(rel).map_err(|e| format!("{e}（还原 {rel}）"))?);
    if !src.is_file() {
        return Ok(());
    }
    let dst = safe_join(instance_dir, rel)?;
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("PERMISSION|创建目录失败|{e}"))?;
    }
    std::fs::copy(&src, &dst).map_err(|e| format!("PERMISSION|还原 {rel} 失败|{e}"))?;
    Ok(())
}

fn remove_file_if_inside(instance_dir: &Path, rel: &str) -> Result<bool, String> {
    let path = safe_join(instance_dir, rel)?;
    if !path.is_file() {
        return Ok(false);
    }
    if crate::net::is_reparse_point(&path) {
        return Err(format!("INVALID_PATH|{rel} 是符号链接，已拒绝删除"));
    }
    std::fs::remove_file(&path).map_err(|e| format!("PERMISSION|删除 {rel} 失败|{e}"))?;
    Ok(true)
}

/// 下载一个文件（线上走白名单；自检时允许本地测试服务器）
fn download_one(
    base: &str,
    url: &str,
    dest: &Path,
    sha1: Option<&str>,
    sha512: Option<&str>,
    cancel: crate::net::CancelRef<'_>,
    on: impl FnMut(u64, Option<u64>),
) -> Result<u64, String> {
    let opts = crate::net::DownloadOptions {
        expected_sha1: sha1.map(|s| s.to_string()),
        expected_sha512: sha512.map(|s| s.to_string()),
        allow_local_http: base != crate::pack_update::API,
        ..Default::default()
    };
    if base == crate::pack_update::API {
        crate::net::validate_url(url)?;
    }
    let report = crate::net::download(url, dest, &opts, cancel, on)?;
    Ok(report.bytes)
}

/// 执行一次整合包更新。
///
/// `req.running` = 实例有没有在跑游戏；运行中拒绝，避免改到一半的文件。
pub fn apply_at(
    req: &ApplyRequest<'_>,
    game: &Path,
    cancel: crate::net::CancelRef<'_>,
    mut on: impl FnMut(&str, &str, u32, u32, u64, u64),
) -> Result<PackUpdateOutcome, String> {
    let base = req.base;
    let instance_id = req.instance_id;
    let version_id = req.version_id;
    let plan_id = req.plan_id;
    if req.running {
        return Err("INSTANCE_BUSY|游戏正在运行，先退出游戏再更新整合包".into());
    }
    let dir = crate::instance::instance_dir(game, instance_id);
    if !dir.is_dir() {
        return Err("INSTANCE_NOT_FOUND|实例不存在".into());
    }

    // 重新算一遍计划：plan_id 是内容指纹，对不上就是世界变了
    let plan = crate::pack_update::plan_at(base, game, instance_id, version_id)?;
    if plan.plan_id != plan_id {
        return Err("PLAN_EXPIRED|更新计划已过期，请重新检查更新".into());
    }
    if plan.blocking {
        let why = plan
            .issues
            .iter()
            .filter(|i| i.blocking)
            .map(|i| i.message.clone())
            .collect::<Vec<_>>()
            .join("；");
        return Err(format!("INVALID_VALUE|这个更新不能直接做：{why}"));
    }

    let new_index = crate::pack_update::fetch_index_at(base, game, version_id)?;
    let pack_path = crate::pack_update::pack_path_for(base, game, version_id)?;
    let by_path: std::collections::BTreeMap<&str, &crate::modpack::IndexFile> = new_index
        .files
        .iter()
        .map(|f| (f.path.as_str(), f))
        .collect();

    let batch_id = format!(
        "pb_{}_{:x}",
        crate::time::now_stamp(),
        crate::time::now_nanos() & 0xffff_ffff
    );
    let batch_dir = backup_root(&dir).join(&batch_id);

    // 1) 备份：新增（万一玩家自己放过同名文件）、变更、删除的文件
    let mut backed_up: Vec<String> = Vec::new();
    let mut added_paths: Vec<String> = Vec::new();
    let mut removed_paths: Vec<String> = Vec::new();
    let mut backup_bytes = 0u64;
    {
        let mut want: Vec<&str> = Vec::new();
        want.extend(plan.added.iter().map(|c| c.path.as_str()));
        want.extend(plan.changed.iter().map(|c| c.path.as_str()));
        want.extend(plan.removed.iter().map(|c| c.path.as_str()));
        want.sort_unstable();
        want.dedup();
        if want.len() > MAX_BACKUP_FILES {
            return Err(format!(
                "INVALID_VALUE|这次更新要动 {} 个文件，超过单次上限 {}",
                want.len(),
                MAX_BACKUP_FILES
            ));
        }
        for rel in want {
            if (cancel)() {
                return Err("CANCELLED|已取消".into());
            }
            let n = backup_one(&dir, &batch_dir, rel)?;
            if n > 0 {
                backup_bytes = backup_bytes.saturating_add(n);
                if backup_bytes > MAX_BACKUP_BYTES {
                    let _ = std::fs::remove_dir_all(&batch_dir);
                    return Err(format!(
                        "INVALID_VALUE|这次更新要备份的文件超过 {} MiB，已中止",
                        MAX_BACKUP_BYTES / 1024 / 1024
                    ));
                }
                backed_up.push(rel.to_string());
            }
        }
        // 来源文件也留一份，回滚时一起还原
        let prov = dir.join(crate::modpack::PROVENANCE_FILE);
        if prov.is_file() {
            std::fs::create_dir_all(&batch_dir)
                .map_err(|e| format!("PERMISSION|创建备份目录失败|{e}"))?;
            let _ = std::fs::copy(&prov, batch_dir.join("_provenance.json"));
        }
    }
    added_paths.extend(plan.added.iter().map(|c| c.path.clone()));
    removed_paths.extend(plan.removed.iter().map(|c| c.path.clone()));

    let old_prov = crate::modpack::read_provenance(&dir);
    let total_download: u64 = plan
        .added
        .iter()
        .chain(plan.changed.iter())
        .map(|c| c.new_size)
        .sum();
    let mut done_bytes = 0u64;
    let mut done_files = 0u32;
    let total_files = (plan.added.len() + plan.changed.len()) as u32;

    // 2) 真正改：下载 → 删多余 → 解压 overrides → 写来源
    let run = |cancel: crate::net::CancelRef<'_>,
               on: &mut Progress<'_>,
               done_bytes: &mut u64,
               done_files: &mut u32|
     -> Result<(), String> {
        for change in plan.added.iter().chain(plan.changed.iter()) {
            if (cancel)() {
                return Err("CANCELLED|已取消".into());
            }
            let Some(f) = by_path.get(change.path.as_str()) else {
                return Err(format!(
                    "INTERNAL|新版本里找不到 {} 的下载信息",
                    change.path
                ));
            };
            let dest = safe_join(&dir, &change.path)?;
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("PERMISSION|创建目录失败|{e}"))?;
            }
            let base_bytes = *done_bytes;
            download_one(
                base,
                &f.url,
                &dest,
                f.sha1.as_deref(),
                f.sha512.as_deref(),
                cancel,
                |cur, _total| {
                    on(
                        "download",
                        &change.path,
                        *done_files,
                        total_files,
                        base_bytes.saturating_add(cur),
                        total_download,
                    );
                },
            )?;
            *done_bytes = done_bytes.saturating_add(f.size);
            *done_files += 1;
            on(
                "download",
                &change.path,
                *done_files,
                total_files,
                *done_bytes,
                total_download,
            );
        }

        for change in &plan.removed {
            if (cancel)() {
                return Err("CANCELLED|已取消".into());
            }
            remove_file_if_inside(&dir, &change.path)?;
        }

        if (cancel)() {
            return Err("CANCELLED|已取消".into());
        }
        on("extract", "解压覆盖文件", 0, 0, *done_bytes, total_download);
        crate::modpack::extract_overrides(&pack_path, &dir, cancel, |d, _| {
            on("extract", "解压覆盖文件", d, 0, *done_bytes, total_download);
        })?;

        let prov = crate::modpack::PackProvenance {
            project_id: old_prov
                .as_ref()
                .map(|p| p.project_id.clone())
                .unwrap_or_default(),
            version_id: plan.to_version_id.clone(),
            version_name: plan.to_version_name.clone(),
            source: old_prov
                .as_ref()
                .map(|p| p.source.clone())
                .unwrap_or_else(|| "modrinth".into()),
            pack_name: plan.pack_name.clone(),
            installed_at: crate::time::now_iso(),
        };
        crate::modpack::write_provenance(&dir, &prov)?;
        Ok(())
    };

    let outcome = run(cancel, &mut on, &mut done_bytes, &mut done_files);
    if let Err(e) = outcome {
        // 整批回滚：新增的删掉，备份的还原回去
        let mut rollback_err: Vec<String> = Vec::new();
        for rel in &added_paths {
            if let Err(re) = remove_file_if_inside(&dir, rel) {
                rollback_err.push(re);
            }
        }
        for rel in &backed_up {
            if let Err(re) = restore_one(&dir, &batch_dir, rel) {
                rollback_err.push(re);
            }
        }
        let prov_backup = batch_dir.join("_provenance.json");
        if prov_backup.is_file() {
            let _ = std::fs::copy(&prov_backup, dir.join(crate::modpack::PROVENANCE_FILE));
        }
        if rollback_err.is_empty() {
            let _ = std::fs::remove_dir_all(&batch_dir);
        }
        return Err(e);
    }

    // 3) 记历史，保留最近 MAX_HISTORY 份
    let record = PackUpdateRecord {
        batch_id: batch_id.clone(),
        instance_id: instance_id.to_string(),
        from_version_id: plan.from_version_id.clone(),
        from_version_name: plan.from_version_name.clone(),
        to_version_id: plan.to_version_id.clone(),
        to_version_name: plan.to_version_name.clone(),
        created_at: crate::time::now_iso(),
        backed_up,
        added: added_paths,
        removed: removed_paths,
    };
    let mut list = load_history(&dir);
    list.push(record);
    list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    while list.len() > MAX_HISTORY {
        if let Some(old) = list.pop() {
            let _ = std::fs::remove_dir_all(backup_root(&dir).join(&old.batch_id));
        }
    }
    let _ = save_history(&dir, &list);

    Ok(PackUpdateOutcome {
        batch_id,
        instance_id: instance_id.to_string(),
        from_version_id: plan.from_version_id,
        to_version_id: plan.to_version_id,
        added: plan.added.len() as u32,
        changed: plan.changed.len() as u32,
        removed: plan.removed.len() as u32,
        bytes: done_bytes,
        backup_dir: batch_dir.to_string_lossy().to_string(),
    })
}

/// 线上入口
pub fn apply(
    game: &Path,
    instance_id: &str,
    version_id: &str,
    plan_id: &str,
    running: bool,
    cancel: crate::net::CancelRef<'_>,
    on: impl FnMut(&str, &str, u32, u32, u64, u64),
) -> Result<PackUpdateOutcome, String> {
    let req = ApplyRequest {
        base: crate::pack_update::API,
        instance_id,
        version_id,
        plan_id,
        running,
    };
    apply_at(&req, game, cancel, on)
}

/// 回滚一次更新：删掉当时新增的，恢复当时备份的。
pub fn rollback(game: &Path, instance_id: &str, batch_id: &str) -> Result<u32, String> {
    let dir = crate::instance::instance_dir(game, instance_id);
    if !dir.is_dir() {
        return Err("INSTANCE_NOT_FOUND|实例不存在".into());
    }
    let list = load_history(&dir);
    let rec = list
        .iter()
        .find(|r| r.batch_id == batch_id)
        .cloned()
        .ok_or_else(|| "NOT_FOUND|找不到这次更新的备份".to_string())?;
    let batch_dir = backup_root(&dir).join(&rec.batch_id);
    if !batch_dir.is_dir() {
        return Err("NOT_FOUND|备份文件不在了（可能被手动删掉了）".into());
    }
    let mut restored = 0u32;
    for rel in &rec.added {
        if remove_file_if_inside(&dir, rel)? {
            restored += 1;
        }
    }
    for rel in &rec.backed_up {
        restore_one(&dir, &batch_dir, rel)?;
        restored += 1;
    }
    let prov_backup = batch_dir.join("_provenance.json");
    if prov_backup.is_file() {
        std::fs::copy(&prov_backup, dir.join(crate::modpack::PROVENANCE_FILE))
            .map_err(|e| format!("PERMISSION|还原来源记录失败|{e}"))?;
    }
    let mut kept: Vec<PackUpdateRecord> = list
        .into_iter()
        .filter(|r| r.batch_id != rec.batch_id)
        .collect();
    kept.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    let _ = save_history(&dir, &kept);
    let _ = std::fs::remove_dir_all(&batch_dir);
    Ok(restored)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("astral-pa-{tag}-{:x}", crate::time::now_nanos()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn rejects_paths_outside_the_instance() {
        let d = tmp("path");
        assert!(safe_join(&d, "../evil.jar").is_err());
        assert!(safe_join(&d, "/abs.jar").is_err());
        assert!(safe_join(&d, "C:/win.jar").is_err());
        assert!(safe_join(&d, "mods/a.jar").is_ok());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn backs_up_and_restores_a_file() {
        let d = tmp("backup");
        std::fs::create_dir_all(d.join("mods")).unwrap();
        std::fs::write(d.join("mods/a.jar"), b"old-bytes").unwrap();
        let batch = d.join(BACKUP_DIR).join("pb_test");
        let n = backup_one(&d, &batch, "mods/a.jar").unwrap();
        assert_eq!(n, 9);
        std::fs::write(d.join("mods/a.jar"), b"new!").unwrap();
        restore_one(&d, &batch, "mods/a.jar").unwrap();
        assert_eq!(std::fs::read(d.join("mods/a.jar")).unwrap(), b"old-bytes");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn missing_file_backs_up_as_zero() {
        let d = tmp("missing");
        let batch = d.join(BACKUP_DIR).join("pb_x");
        assert_eq!(backup_one(&d, &batch, "mods/none.jar").unwrap(), 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn history_is_empty_without_a_record() {
        let d = tmp("hist");
        assert!(history(&d).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}
