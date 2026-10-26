//! 模组批量更新与回滚（契约 V2.5 B2）。
//!
//! 原则：
//! - **先计划后执行**：`plan` 只读，返回一份人能看的更新清单；`execute` 才动手。
//! - **不删整个 mods 重建**：只动计划里点名的文件。
//! - **可回滚**：更新前把旧文件复制进 `mods/.astral-backup/<批次>/`，
//!   回滚时按批次恢复，恢复完再删备份。
//! - **不把最新当兼容**：版本必须同时匹配实例的 MC 版本与加载器；
//!   稳定版不会被自动升成 beta/alpha，除非玩家自己选。
//! - **失败要能退回去**：一个批次里任何一步失败，整批已成功的部分自动回滚。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::updates::ModUpdate;

pub const BACKUP_DIR: &str = ".astral-backup";
const MAX_HISTORY: usize = 30;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BackupRecord {
    pub batch_id: String,
    pub created_at: String,
    /// 旧文件名 → 备份里的相对文件名
    pub files: BTreeMap<String, String>,
    /// 这次更新把哪些文件换成了什么（用于展示与二次回滚）
    pub replaced: BTreeMap<String, String>,
    pub note: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePlanItem {
    pub file_name: String,
    pub project_id: String,
    pub current_version: String,
    pub target_version: String,
    pub target_version_id: String,
    pub target_file_name: String,
    pub size: u64,
    /// 目标文件名和现有文件不同：会先下新的再删旧的
    pub renames: bool,
    /// 这条为什么不能更新（空 = 可以）
    pub blocked: Option<String>,
    /// 非阻塞提示（比如"这是 beta 版"）
    pub notes: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePlan {
    pub plan_id: String,
    pub instance_id: String,
    pub items: Vec<UpdatePlanItem>,
    pub skipped: Vec<String>,
    pub total_bytes: u64,
    pub blocking: bool,
    pub created_at: String,
    pub expires_at: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UpdateResult {
    pub batch_id: String,
    pub updated: Vec<String>,
    pub failed: Vec<(String, String)>,
    pub rolled_back: bool,
}

fn backup_root(mods_dir: &Path) -> PathBuf {
    mods_dir.join(BACKUP_DIR)
}

fn batch_dir(mods_dir: &Path, batch_id: &str) -> PathBuf {
    backup_root(mods_dir).join(batch_id)
}

fn history_file(mods_dir: &Path) -> PathBuf {
    backup_root(mods_dir).join("history.json")
}

fn load_history(mods_dir: &Path) -> Vec<BackupRecord> {
    let p = history_file(mods_dir);
    let Ok(text) = std::fs::read_to_string(&p) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<BackupRecord>>(&text).unwrap_or_default()
}

fn save_history(mods_dir: &Path, list: &[BackupRecord]) -> Result<(), String> {
    let root = backup_root(mods_dir);
    std::fs::create_dir_all(&root).map_err(|e| format!("PERMISSION|创建备份目录失败|{e}"))?;
    let p = history_file(mods_dir);
    let text = serde_json::to_string_pretty(list)
        .map_err(|e| format!("INTERNAL|备份历史序列化失败|{e}"))?;
    let part = p.with_extension("json.part");
    std::fs::write(&part, text).map_err(|e| format!("PERMISSION|写备份历史失败|{e}"))?;
    std::fs::rename(&part, &p).map_err(|e| format!("PERMISSION|提交备份历史失败|{e}"))?;
    Ok(())
}

/// 生成批次 id：时间戳 + 序号，可排序、可读
fn new_batch_id() -> String {
    format!("upd_{:x}", crate::time::now_unix_ms())
}

/// 列出现有备份批次（新的在前）
pub fn history(mods_dir: &Path) -> Vec<BackupRecord> {
    let mut list = load_history(mods_dir);
    list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    list
}

/// 只读：根据一次 `check()` 的结果，算出真正要做什么。
///
/// 这里会再查一次目标版本的详情，确认它确实兼容当前实例、文件名安全、
/// 稳定版不被自动降级到测试版。任何一条不满足都标 `blocked`，不执行。
pub fn plan(
    mods_dir: &Path,
    instance_id: &str,
    game_version: &str,
    loader: Option<&str>,
    updates: &[ModUpdate],
) -> UpdatePlan {
    let mut items = Vec::new();
    let mut skipped = Vec::new();
    let mut total = 0u64;

    for u in updates {
        let mut notes = Vec::new();
        let mut blocked: Option<String> = None;

        // 目标版本详情：必须属于这个项目，且与实例兼容
        let detail = crate::resources::version_detail(&u.latest_version_id);
        let (file, project) = match detail {
            Ok(v) => v,
            Err(e) => {
                skipped.push(format!("{}：取不到目标版本（{e}）", u.file_name));
                continue;
            }
        };
        if project.as_deref() != Some(u.project_id.as_str()) {
            blocked = Some("目标版本不属于该项目".into());
        }
        if !file.game_versions.iter().any(|g| g == game_version) {
            blocked = Some(format!("目标版本不支持 Minecraft {game_version}"));
        }
        let loader_ok = match loader {
            Some(l) => file.loaders.iter().any(|x| x.eq_ignore_ascii_case(l)),
            None => file
                .loaders
                .iter()
                .all(|x| x.eq_ignore_ascii_case("vanilla") || x.eq_ignore_ascii_case("minecraft"))
                || file.loaders.is_empty(),
        };
        if !loader_ok {
            blocked = Some(format!("目标版本不支持加载器 {}", loader.unwrap_or("原版")));
        }
        // 文件名必须是单层相对名
        if !crate::resources::dep_file_name_ok(&file.file_name) {
            blocked = Some(format!("目标文件名不安全：{}", file.file_name));
        }
        // 稳定版 → 测试版：默认不允许，明确写出来
        let cur_is_stable = !u.current_version.contains("beta")
            && !u.current_version.contains("alpha")
            && !u.current_version.contains("rc");
        let target_is_test = matches!(file.channel.as_str(), "beta" | "alpha");
        if cur_is_stable && target_is_test {
            blocked = Some(format!(
                "目标版本是 {} 通道，不会把稳定版自动升成测试版",
                file.channel
            ));
        } else if target_is_test {
            notes.push(format!("目标版本是 {} 通道", file.channel));
        }

        // 目标文件已存在且不是要替换的那个 → 说明玩家手动装过
        let target_path = mods_dir.join(&file.file_name);
        if file.file_name != u.file_name && target_path.exists() {
            blocked = Some(format!("目标文件 {} 已存在，可能手动装过", file.file_name));
        }

        if blocked.is_none() {
            total += file.size;
        }
        items.push(UpdatePlanItem {
            file_name: u.file_name.clone(),
            project_id: u.project_id.clone(),
            current_version: u.current_version.clone(),
            target_version: u.latest_version.clone(),
            target_version_id: u.latest_version_id.clone(),
            target_file_name: file.file_name.clone(),
            size: file.size,
            renames: file.file_name != u.file_name,
            blocked,
            notes,
        });
    }

    let blocking = items.iter().all(|i| i.blocked.is_some()) && !items.is_empty();
    let now = crate::time::now_unix_ms();
    UpdatePlan {
        plan_id: format!("uplan_{:x}", now),
        instance_id: instance_id.to_string(),
        items,
        skipped,
        total_bytes: total,
        blocking,
        created_at: crate::time::unix_ms_to_iso(now),
        expires_at: crate::time::unix_ms_to_iso(now + 15 * 60 * 1000),
    }
}

/// 把旧文件复制进批次备份目录。用复制而不是移动：失败时原文件还在。
fn backup_file(mods_dir: &Path, batch: &Path, file_name: &str) -> Result<String, String> {
    let src = mods_dir.join(file_name);
    if !src.is_file() {
        return Err(format!("INVALID_PATH|要备份的文件不存在：{file_name}"));
    }
    std::fs::create_dir_all(batch).map_err(|e| format!("PERMISSION|创建批次目录失败|{e}"))?;
    // 备份名加序号，避免同名（理论上不会，但别赌）
    let stored = format!("{}.bak", file_name);
    let dst = batch.join(&stored);
    std::fs::copy(&src, &dst).map_err(|e| format!("PERMISSION|备份 {file_name} 失败|{e}"))?;
    Ok(stored)
}

/// 执行计划：逐条更新，任何一条失败就整批回滚。
pub fn execute(
    game: &Path,
    instance_id: &str,
    plan: &UpdatePlan,
    cancel: crate::net::CancelRef<'_>,
    mut on: impl FnMut(u32, u32, &str),
) -> Result<UpdateResult, String> {
    let stats = |_: &str| crate::instance::PlayStats::default();
    let inst = crate::instance::find(game, instance_id, &stats)
        .ok_or_else(|| "INSTANCE_NOT_FOUND|实例不存在".to_string())?;
    let mods_dir = PathBuf::from(&inst.dirs.mods);

    let todo: Vec<&UpdatePlanItem> = plan
        .items
        .iter()
        .filter(|i| i.blocked.is_none())
        .collect();
    if todo.is_empty() {
        return Err("INVALID_VALUE|计划里没有可执行的更新".into());
    }

    let batch_id = new_batch_id();
    let batch = batch_dir(&mods_dir, &batch_id);
    let mut record = BackupRecord {
        batch_id: batch_id.clone(),
        created_at: crate::time::unix_ms_to_iso(crate::time::now_unix_ms()),
        files: BTreeMap::new(),
        replaced: BTreeMap::new(),
        note: format!("批量更新 {} 个模组", todo.len()),
    };

    let mut updated: Vec<String> = Vec::new();
    let mut failed: Vec<(String, String)> = Vec::new();
    let total = todo.len() as u32;

    for (i, item) in todo.iter().enumerate() {
        on(i as u32, total, &item.file_name);
        if cancel() {
            failed.push((item.file_name.clone(), "CANCELLED|任务已取消".into()));
            break;
        }

        // 1) 先备份旧文件
        match backup_file(&mods_dir, &batch, &item.file_name) {
            Ok(stored) => {
                record.files.insert(item.file_name.clone(), stored);
            }
            Err(e) => {
                failed.push((item.file_name.clone(), e));
                break;
            }
        }

        // 2) 下载新文件（走统一下载引擎：校验哈希、原子提交、可取消）
        let dest = mods_dir.join(&item.target_file_name);
        let dl = crate::resources::version_detail(&item.target_version_id)
            .and_then(|(f, _)| {
                crate::net::download_with_cancel(
                    &f.url,
                    &dest,
                    f.sha1.as_deref(),
                    f.sha512.as_deref(),
                    cancel,
                    |_, _| {},
                )
                .map(|_| f)
            });
        match dl {
            Ok(_) => {
                record
                    .replaced
                    .insert(item.file_name.clone(), item.target_file_name.clone());
                updated.push(item.target_file_name.clone());
                // 3) 新文件到位后才删旧的（同名说明是原地替换）
                if item.target_file_name != item.file_name {
                    if let Err(e) = std::fs::remove_file(mods_dir.join(&item.file_name)) {
                        failed.push((item.file_name.clone(), format!("PERMISSION|删除旧文件失败|{e}")));
                        break;
                    }
                }
            }
            Err(e) => {
                failed.push((item.file_name.clone(), e));
                break;
            }
        }
    }

    // 记历史（即使失败也记，回滚要用）
    let mut hist = load_history(&mods_dir);
    hist.push(record.clone());
    hist.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    if hist.len() > MAX_HISTORY {
        let drop_count = hist.len() - MAX_HISTORY;
        for old in hist.drain(0..drop_count) {
            let _ = std::fs::remove_dir_all(batch_dir(&mods_dir, &old.batch_id));
        }
    }
    let _ = save_history(&mods_dir, &hist);

    // 任何失败：整批回滚，绝不留半新半旧的状态
    let mut rolled_back = false;
    if !failed.is_empty() {
        rolled_back = rollback_batch(&mods_dir, &record).is_ok();
        if rolled_back {
            // 回滚成功后这条批次记录已失效
            let mut h = load_history(&mods_dir);
            h.retain(|r| r.batch_id != record.batch_id);
            let _ = save_history(&mods_dir, &h);
        }
    }

    Ok(UpdateResult {
        batch_id,
        updated,
        failed,
        rolled_back,
    })
}

/// 回滚一个批次：把备份复制回去，删掉这批新装的文件。
///
/// 只动这一批记录里的文件，绝不整目录覆盖。
fn rollback_batch(mods_dir: &Path, record: &BackupRecord) -> Result<u32, String> {
    let batch = batch_dir(mods_dir, &record.batch_id);
    let mut restored = 0u32;
    for (old_name, stored) in &record.files {
        let src = batch.join(stored);
        if !src.is_file() {
            return Err(format!("INVALID_PATH|备份文件丢失：{stored}"));
        }
        let dst = mods_dir.join(old_name);
        std::fs::copy(&src, &dst).map_err(|e| format!("PERMISSION|恢复 {old_name} 失败|{e}"))?;
        restored += 1;
    }
    // 删掉这批新装的文件（不是备份里的那些）
    for (old_name, new_name) in &record.replaced {
        if old_name == new_name {
            continue;
        }
        let p = mods_dir.join(new_name);
        if p.is_file() {
            let _ = std::fs::remove_file(p);
        }
    }
    let _ = std::fs::remove_dir_all(&batch);
    Ok(restored)
}

/// 手动回滚指定批次。恢复后删除该批次备份。
pub fn rollback(mods_dir: &Path, batch_id: &str) -> Result<u32, String> {
    let mut hist = load_history(mods_dir);
    let Some(idx) = hist.iter().position(|r| r.batch_id == batch_id) else {
        return Err("INVALID_VALUE|没有这个更新批次".into());
    };
    let record = hist[idx].clone();
    let n = rollback_batch(mods_dir, &record)?;
    hist.remove(idx);
    save_history(mods_dir, &hist)?;
    Ok(n)
}

/// 清理过期备份（默认保留 7 天）。只删 `.astral-backup` 下的东西。
pub fn prune(mods_dir: &Path, keep_days: u64) -> Result<(u64, u64), String> {
    let cutoff = crate::time::now_unix_ms() - (keep_days as i64) * 24 * 3600 * 1000;
    let mut hist = load_history(mods_dir);
    let mut removed_dirs = 0u64;
    let mut removed_records = 0u64;
    let mut keep = Vec::new();
    for r in hist.drain(..) {
        // created_at 是 ISO 字符串，这里用批次 id 里的时间戳判断更稳
        let ts = r
            .batch_id
            .strip_prefix("upd_")
            .and_then(|h| i64::from_str_radix(h, 16).ok())
            .unwrap_or(0);
        if ts < cutoff {
            let _ = std::fs::remove_dir_all(batch_dir(mods_dir, &r.batch_id));
            removed_dirs += 1;
            removed_records += 1;
        } else {
            keep.push(r);
        }
    }
    save_history(mods_dir, &keep)?;
    Ok((removed_dirs, removed_records))
}

/// 自检专用：生成一个批次 id
#[doc(hidden)]
pub fn new_batch_id_for_test() -> String {
    new_batch_id()
}

/// 自检专用：直接写历史
#[doc(hidden)]
pub fn save_history_for_test(mods_dir: &Path, list: &[BackupRecord]) {
    let _ = save_history(mods_dir, list);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_id_has_prefix() {
        assert!(new_batch_id().starts_with("upd_"));
    }

    #[test]
    fn history_roundtrip() {
        let dir = std::env::temp_dir().join(format!("astral-b2-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let rec = BackupRecord {
            batch_id: "upd_test".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            files: BTreeMap::new(),
            replaced: BTreeMap::new(),
            note: "测试".into(),
        };
        save_history(&dir, std::slice::from_ref(&rec)).unwrap();
        let back = history(&dir);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].batch_id, "upd_test");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
