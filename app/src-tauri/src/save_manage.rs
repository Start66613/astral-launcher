//! 存档管理：重命名、复制、删除到回收站、从回收站恢复。
//!
//! 原则：
//! - 删除**只移到回收站**，不永久删除；永久清空是独立的危险操作，要 `confirm`；
//! - 重命名 / 复制 / 删除都要求游戏没在运行 —— 跑着的时候存档是写了一半的状态，
//!   这时候复制出来的东西没法自证一致；
//! - 目标同名已存在时一律拒绝，绝不覆盖玩家已有的存档；
//! - 复制不跟随符号链接，有字节与条数上限；
//! - 所有列表操作只读，不修改任何文件。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 回收站目录名（相对实例目录）。挂在存档备份目录下面，
/// 玩家的"存档相关的东西"集中在一处，不会散落到各处。
pub const DELETED_DIR: &str = "deleted";
const INDEX_FILE: &str = "deleted.json";
const MAX_NAME: usize = 64;
const MAX_COPY_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const MAX_COPY_ENTRIES: u64 = 200_000;

/// 回收站里的一个存档
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DeletedWorld {
    /// 回收站内的目录名（`del_<十六进制>`），恢复与永久删除都用它定位
    pub trash_name: String,
    /// 原来的存档目录名
    pub world_name: String,
    pub deleted_at: String,
    /// 回收站里的实际占用
    pub bytes: u64,
    pub file_count: u64,
}

fn deleted_root(instance_dir: &Path) -> PathBuf {
    instance_dir
        .join(crate::world_backup::BACKUP_DIR)
        .join(DELETED_DIR)
}

fn index_path(instance_dir: &Path) -> PathBuf {
    deleted_root(instance_dir).join(INDEX_FILE)
}

fn load_index(instance_dir: &Path) -> Vec<DeletedWorld> {
    let Ok(text) = std::fs::read_to_string(index_path(instance_dir)) else {
        return Vec::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

fn save_index(instance_dir: &Path, list: &[DeletedWorld]) -> Result<(), String> {
    let root = deleted_root(instance_dir);
    std::fs::create_dir_all(&root).map_err(|e| format!("PERMISSION|创建存档回收站失败|{e}"))?;
    let text = serde_json::to_string_pretty(list)
        .map_err(|e| format!("INTERNAL|序列化存档回收站索引失败|{e}"))?;
    let tmp = root.join(format!("{INDEX_FILE}.part"));
    std::fs::write(&tmp, text).map_err(|e| format!("PERMISSION|写入存档回收站索引失败|{e}"))?;
    std::fs::rename(&tmp, index_path(instance_dir))
        .map_err(|e| format!("PERMISSION|保存存档回收站索引失败|{e}"))?;
    Ok(())
}

/// 存档目录名合法性（Windows 上比 Linux 严格得多）
pub fn validate_name(name: &str) -> Result<String, String> {
    let t = name.trim();
    if t.is_empty() {
        return Err("INVALID_NAME|名字不能为空".into());
    }
    if t.chars().count() > MAX_NAME {
        return Err(format!("INVALID_NAME|名字太长（最多 {MAX_NAME} 个字符）"));
    }
    if t == "." || t == ".." {
        return Err("INVALID_NAME|这个名字不能用".into());
    }
    if t.chars().any(|c| c.is_control()) {
        return Err("INVALID_NAME|名字里不能有控制字符".into());
    }
    const BAD: [char; 9] = ['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
    if let Some(c) = t.chars().find(|c| BAD.contains(c)) {
        return Err(format!("INVALID_NAME|名字里不能有 {c}"));
    }
    if t.ends_with('.') || t.ends_with(' ') {
        return Err("INVALID_NAME|名字不能以点或空格结尾（Windows 会悄悄去掉）".into());
    }
    let upper = t.to_ascii_uppercase();
    let stem = upper.split('.').next().unwrap_or("").to_string();
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED.contains(&stem.as_str()) {
        return Err(format!("INVALID_NAME|{t} 是 Windows 的保留名，换一个"));
    }
    Ok(t.to_string())
}

fn new_trash_name() -> String {
    format!("del_{:032x}", crate::time::now_nanos())
}

/// 递归复制一个目录；不跟随符号链接 / junction，有字节与条数上限。
fn copy_tree(src: &Path, dst: &Path) -> Result<(u64, u64), String> {
    let mut bytes = 0u64;
    let mut files = 0u64;
    let mut stack = vec![(src.to_path_buf(), dst.to_path_buf())];
    while let Some((from, to)) = stack.pop() {
        std::fs::create_dir_all(&to).map_err(|e| format!("PERMISSION|创建目录失败|{e}"))?;
        let rd = std::fs::read_dir(&from).map_err(|e| format!("PERMISSION|读取目录失败|{e}"))?;
        for e in rd.flatten() {
            let p = e.path();
            let q = to.join(e.file_name());
            let meta = match std::fs::symlink_metadata(&p) {
                Ok(m) => m,
                Err(_) => continue,
            };
            // 链接一律跳过：一个指到 C:\ 的链接会把整块盘复制进来
            if meta.file_type().is_symlink() || crate::net::is_reparse_point(&p) {
                continue;
            }
            if meta.is_dir() {
                stack.push((p, q));
            } else if meta.is_file() {
                files += 1;
                if files > MAX_COPY_ENTRIES {
                    return Err(format!(
                        "INVALID_VALUE|文件太多（超过 {MAX_COPY_ENTRIES} 个），已中止"
                    ));
                }
                bytes = bytes.saturating_add(meta.len());
                if bytes > MAX_COPY_BYTES {
                    return Err(format!(
                        "INVALID_VALUE|存档太大（超过 {} GB），已中止",
                        MAX_COPY_BYTES / 1024 / 1024 / 1024
                    ));
                }
                std::fs::copy(&p, &q).map_err(|e| format!("PERMISSION|复制文件失败|{e}"))?;
            }
        }
    }
    Ok((bytes, files))
}

/// 重命名一个存档目录。返回新的 `worldId` 与目录名。
pub fn rename(
    saves_dir: &Path,
    world_id: &str,
    new_name: &str,
    running: bool,
) -> Result<(String, String), String> {
    if running {
        return Err("INSTANCE_BUSY|游戏正在运行，先退出游戏再改存档名字".into());
    }
    let name = validate_name(new_name)?;
    let src = crate::worlds::resolve(saves_dir, world_id)
        .ok_or_else(|| "WORLD_NOT_FOUND|存档不存在".to_string())?;
    let dst = saves_dir.join(&name);
    if dst.exists() {
        return Err(format!("FILE_EXISTS|已经有一个叫「{name}」的存档了"));
    }
    std::fs::rename(&src, &dst).map_err(|e| format!("PERMISSION|重命名存档失败|{e}"))?;
    Ok((crate::worlds::world_id(&name), name))
}

/// 复制一个存档。复制出来的目录必须换个名字，不覆盖已有存档。
pub fn duplicate(
    saves_dir: &Path,
    world_id: &str,
    new_name: &str,
    running: bool,
) -> Result<(String, String, u64), String> {
    if running {
        return Err("INSTANCE_BUSY|游戏正在运行，存档是写了一半的状态，没法安全复制".into());
    }
    let name = validate_name(new_name)?;
    let src = crate::worlds::resolve(saves_dir, world_id)
        .ok_or_else(|| "WORLD_NOT_FOUND|存档不存在".to_string())?;
    let dst = saves_dir.join(&name);
    if dst.exists() {
        return Err(format!("FILE_EXISTS|已经有一个叫「{name}」的存档了"));
    }
    // 先复制到临时目录，成功后再改名，失败不留半个存档
    let tmp = saves_dir.join(format!(".copy-{:032x}", crate::time::now_nanos()));
    let result = copy_tree(&src, &tmp);
    let (bytes, _files) = match result {
        Ok(v) => v,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&tmp);
            return Err(e);
        }
    };
    if let Err(e) = std::fs::rename(&tmp, &dst) {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(format!("PERMISSION|复制存档失败|{e}"));
    }
    Ok((crate::worlds::world_id(&name), name, bytes))
}

/// 删除存档：只移到回收站，不永久删除。
pub fn delete(
    instance_dir: &Path,
    saves_dir: &Path,
    world_id: &str,
    running: bool,
) -> Result<DeletedWorld, String> {
    if running {
        return Err("INSTANCE_BUSY|游戏正在运行，先退出游戏再删存档".into());
    }
    let src = crate::worlds::resolve(saves_dir, world_id)
        .ok_or_else(|| "WORLD_NOT_FOUND|存档不存在".to_string())?;
    let world_name = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| "INVALID_NAME|存档目录名无效".to_string())?;
    let (bytes, file_count) = crate::world_backup::dir_stats(&src);

    let root = deleted_root(instance_dir);
    std::fs::create_dir_all(&root).map_err(|e| format!("PERMISSION|创建存档回收站失败|{e}"))?;
    let mut trash_name = new_trash_name();
    let mut guard = 0;
    while root.join(&trash_name).exists() {
        guard += 1;
        if guard > 8 {
            return Err("INTERNAL|回收站命名冲突，请稍后再试".into());
        }
        trash_name = format!("{}-{guard}", new_trash_name());
    }
    let dst = root.join(&trash_name);
    std::fs::rename(&src, &dst).map_err(|e| format!("PERMISSION|移动存档到回收站失败|{e}"))?;

    let entry = DeletedWorld {
        trash_name: trash_name.clone(),
        world_name,
        deleted_at: crate::time::now_iso(),
        bytes,
        file_count,
    };
    let mut list = load_index(instance_dir);
    list.push(entry.clone());
    // 索引写不进去时把目录搬回去，不能出现"文件在回收站、索引里没有"的孤儿
    if let Err(e) = save_index(instance_dir, &list) {
        let _ = std::fs::rename(&dst, &src);
        return Err(e);
    }
    Ok(entry)
}

/// 列出回收站里的存档（只读，最新的在前）
pub fn list_deleted(instance_dir: &Path) -> Vec<DeletedWorld> {
    let root = deleted_root(instance_dir);
    let mut list = load_index(instance_dir);
    // 目录不在的条目不算数，避免"列表里有、恢复时找不到"
    list.retain(|d| root.join(&d.trash_name).is_dir());
    list.sort_by(|a, b| b.deleted_at.cmp(&a.deleted_at));
    list
}

/// 从回收站恢复一个存档。
///
/// 目标同名已存在时：`overwrite = false` 直接拒绝；`overwrite = true`
/// 先把现有存档改名留底（不删除），再把回收站里的搬回去。
pub fn restore(
    instance_dir: &Path,
    saves_dir: &Path,
    trash_name: &str,
    overwrite: bool,
    running: bool,
) -> Result<(String, String, Option<String>), String> {
    if running {
        return Err("INSTANCE_BUSY|游戏正在运行，先退出游戏再恢复存档".into());
    }
    let root = deleted_root(instance_dir);
    let src = root.join(trash_name);
    if trash_name.is_empty()
        || trash_name.contains('/')
        || trash_name.contains('\\')
        || trash_name.contains(':')
        || trash_name == "."
        || trash_name == ".."
    {
        return Err("INVALID_NAME|回收站条目名无效".into());
    }
    if !src.is_dir() {
        return Err("NOT_FOUND|回收站里没有这个存档".into());
    }
    let mut list = load_index(instance_dir);
    let idx = list
        .iter()
        .position(|d| d.trash_name == trash_name)
        .ok_or_else(|| "NOT_FOUND|回收站索引里没有这个存档".to_string())?;
    let world_name = validate_name(&list[idx].world_name)?;

    let dst = saves_dir.join(&world_name);
    let mut replaced: Option<String> = None;
    if dst.exists() {
        if !overwrite {
            return Err(format!(
                "FILE_EXISTS|已经有一个叫「{world_name}」的存档了，先改名或选择覆盖"
            ));
        }
        let backup_name = format!("{world_name}-replaced-{:032x}", crate::time::now_nanos());
        let backup = saves_dir.join(&backup_name);
        std::fs::rename(&dst, &backup).map_err(|e| format!("PERMISSION|无法给现有存档留底|{e}"))?;
        replaced = Some(backup_name);
    }
    if let Err(e) = std::fs::rename(&src, &dst) {
        // 恢复失败时把留底的目录改回去
        if let Some(name) = &replaced {
            let _ = std::fs::rename(saves_dir.join(name), &dst);
        }
        return Err(format!("PERMISSION|恢复存档失败|{e}"));
    }
    list.remove(idx);
    let _ = save_index(instance_dir, &list);
    Ok((crate::worlds::world_id(&world_name), world_name, replaced))
}

/// 永久删除回收站里的一个存档（不可恢复，必须 confirm）
pub fn purge(instance_dir: &Path, trash_name: &str, confirm: bool) -> Result<u64, String> {
    if !confirm {
        return Err("UNSUPPORTED|永久删除存档需要明确确认".into());
    }
    if trash_name.is_empty()
        || trash_name.contains('/')
        || trash_name.contains('\\')
        || trash_name.contains(':')
        || trash_name == "."
        || trash_name == ".."
    {
        return Err("INVALID_NAME|回收站条目名无效".into());
    }
    let root = deleted_root(instance_dir);
    let p = root.join(trash_name);
    if !p.is_dir() {
        return Err("NOT_FOUND|回收站里没有这个存档".into());
    }
    if crate::net::is_reparse_point(&p) {
        return Err("INVALID_PATH|这是一个符号链接，已拒绝删除".into());
    }
    let (bytes, _files) = crate::world_backup::dir_stats(&p);
    std::fs::remove_dir_all(&p).map_err(|e| format!("PERMISSION|删除存档失败|{e}"))?;
    let mut list = load_index(instance_dir);
    list.retain(|d| d.trash_name != trash_name);
    let _ = save_index(instance_dir, &list);
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> (PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!(
            "astral-save-manage-{tag}-{:032x}",
            crate::time::now_nanos()
        ));
        let inst = base.join("inst");
        let saves = inst.join("saves");
        std::fs::create_dir_all(&saves).unwrap();
        (inst, saves)
    }

    fn make_world(saves: &Path, name: &str, files: &[(&str, usize)]) -> String {
        let dir = saves.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("level.dat"), b"nbt").unwrap();
        for (rel, size) in files {
            let p = dir.join(rel);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&p, vec![7u8; *size]).unwrap();
        }
        crate::worlds::world_id(name)
    }

    #[test]
    fn renames_a_world() {
        let (_inst, saves) = tmp("rename");
        let id = make_world(&saves, "旧世界", &[]);
        let (new_id, name) = rename(&saves, &id, "新世界", false).unwrap();
        assert_eq!(name, "新世界");
        assert_eq!(new_id, crate::worlds::world_id("新世界"));
        assert!(!saves.join("旧世界").exists());
        assert!(saves.join("新世界").join("level.dat").is_file());
    }

    #[test]
    fn rename_refuses_existing_target() {
        let (_inst, saves) = tmp("rename-dup");
        let id = make_world(&saves, "a", &[]);
        make_world(&saves, "b", &[]);
        let err = rename(&saves, &id, "b", false).unwrap_err();
        assert!(err.starts_with("FILE_EXISTS|"), "{err}");
        // 两个都还在
        assert!(saves.join("a").is_dir() && saves.join("b").is_dir());
    }

    #[test]
    fn rejects_bad_names() {
        for bad in [
            "", "  ", "..", ".", "a/b", "a\\b", "a:b", "a<b", "CON", "nul.txt", "x.",
        ] {
            assert!(validate_name(bad).is_err(), "{bad} 应该被拒绝");
        }
        assert_eq!(validate_name("  正常名字  ").unwrap(), "正常名字");
    }

    #[test]
    fn duplicates_a_world() {
        let (_inst, saves) = tmp("dup");
        let id = make_world(&saves, "源", &[("region/r.0.0.mca", 100)]);
        let (_new_id, name, bytes) = duplicate(&saves, &id, "副本", false).unwrap();
        assert_eq!(name, "副本");
        assert_eq!(bytes, 3 + 100);
        assert!(saves.join("源").join("region/r.0.0.mca").is_file());
        assert_eq!(
            std::fs::read(saves.join("副本").join("region/r.0.0.mca"))
                .unwrap()
                .len(),
            100
        );
    }

    #[test]
    fn delete_moves_to_trash_then_restore_brings_it_back() {
        let (inst, saves) = tmp("trash");
        let id = make_world(&saves, "要删的", &[("data/x.dat", 12)]);
        let entry = delete(&inst, &saves, &id, false).unwrap();
        assert_eq!(entry.world_name, "要删的");
        assert_eq!(entry.bytes, 3 + 12);
        assert!(!saves.join("要删的").exists());

        let listed = list_deleted(&inst);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].trash_name, entry.trash_name);

        let (new_id, name, replaced) =
            restore(&inst, &saves, &entry.trash_name, false, false).unwrap();
        assert_eq!(name, "要删的");
        assert_eq!(new_id, id);
        assert!(replaced.is_none());
        assert!(saves.join("要删的").join("data/x.dat").is_file());
        assert!(list_deleted(&inst).is_empty());
    }

    #[test]
    fn restore_refuses_existing_without_overwrite() {
        let (inst, saves) = tmp("restore-conflict");
        let id = make_world(&saves, "冲突", &[]);
        let entry = delete(&inst, &saves, &id, false).unwrap();
        make_world(&saves, "冲突", &[("keep.txt", 5)]);

        let err = restore(&inst, &saves, &entry.trash_name, false, false).unwrap_err();
        assert!(err.starts_with("FILE_EXISTS|"), "{err}");
        // 新存档没被覆盖
        assert!(saves.join("冲突").join("keep.txt").is_file());
    }

    #[test]
    fn restore_with_overwrite_keeps_the_old_copy() {
        let (inst, saves) = tmp("restore-over");
        let id = make_world(&saves, "覆盖", &[]);
        let entry = delete(&inst, &saves, &id, false).unwrap();
        make_world(&saves, "覆盖", &[("keep.txt", 5)]);

        let (_id, _name, replaced) =
            restore(&inst, &saves, &entry.trash_name, true, false).unwrap();
        let kept = replaced.expect("应该留底");
        assert!(saves.join(&kept).join("keep.txt").is_file());
        // 恢复回来的是回收站里那份（没有 keep.txt，只有 level.dat）
        assert!(saves.join("覆盖").join("level.dat").is_file());
        assert!(!saves.join("覆盖").join("keep.txt").exists());
    }

    #[test]
    fn purge_requires_confirm() {
        let (inst, saves) = tmp("purge");
        let id = make_world(&saves, "永久删", &[]);
        let entry = delete(&inst, &saves, &id, false).unwrap();
        let err = purge(&inst, &entry.trash_name, false).unwrap_err();
        assert!(err.starts_with("UNSUPPORTED|"), "{err}");
        assert_eq!(list_deleted(&inst).len(), 1);
        let bytes = purge(&inst, &entry.trash_name, true).unwrap();
        assert!(bytes > 0);
        assert!(list_deleted(&inst).is_empty());
    }

    #[test]
    fn refuses_everything_while_running() {
        let (inst, saves) = tmp("running");
        let id = make_world(&saves, "跑着", &[]);
        assert!(rename(&saves, &id, "改名", true).is_err());
        assert!(duplicate(&saves, &id, "复制", true).is_err());
        assert!(delete(&inst, &saves, &id, true).is_err());
        // 存档原封不动
        assert!(saves.join("跑着").join("level.dat").is_file());
    }
}
