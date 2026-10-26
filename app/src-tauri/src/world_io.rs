//! 存档导出与导入：把自己的存档发给别人，或者收下别人的存档。
//!
//! 和 `world_backup` 的区别：
//! - `world_backup` 是**自己给自己**留后路，包存在实例的 `.astral-worlds/` 里，
//!   玩家平时看不到，也不会拿去发给别人；
//! - 这里是**给文件**：导出一个 `.zip` 到玩家选的位置（发群里、发网盘），
//!   或者把别人给的 `.zip` 收进 `saves/`。
//!
//! 安全原则（和备份模块一致，另外多了「不认识的东西一律拒绝」）：
//! - 游戏运行中拒绝导出（存档正在被写，包出来是不一致的）和导入
//!   （游戏退出时会覆盖 `level.dat`）；
//! - 导出先写 `.part` 再改名，中途失败不留半个包；
//! - 导入逐条校验路径，防 Zip Slip；拒绝符号链接条目；
//! - 导入必须能在包根看到 `level.dat`，否则说明这不是一个存档；
//! - 目标存档已存在时默认拒绝，要覆盖必须先留底再换；
//! - 全程不删除玩家的东西。

use serde::Serialize;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

/// 单个文件上限，和备份模块保持一致
const MAX_ENTRY: u64 = 512 * 1024 * 1024;
/// 解压后总量上限
const MAX_TOTAL: u64 = 4 * 1024 * 1024 * 1024;
/// 条目数上限
const MAX_ENTRIES: usize = 50_000;

/// 只看一眼这个 zip 是什么（只读，不写任何文件）
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WorldZipInfo {
    pub path: String,
    pub file_name: String,
    /// zip 文件本身多大
    pub bytes: u64,
    /// 包里最外层只有一个目录时，那个目录名就是存档名（`MyWorld/level.dat`）
    pub root_dir: Option<String>,
    /// 解压出来建议叫什么（`root_dir`，没有就用 zip 文件名）
    pub suggested_name: String,
    /// 包根有没有 `level.dat`——没有就说明这不是存档
    pub has_level_dat: bool,
    pub file_count: u64,
    /// 解压后大概要多少字节
    pub total_bytes: u64,
    /// 不能导入的原因（能导入时为空）
    pub problem: Option<String>,
}

/// 导出结果
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WorldExport {
    pub path: String,
    pub world_id: String,
    pub world_name: String,
    pub bytes: u64,
    pub file_count: u64,
}

/// 导入结果
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WorldImport {
    pub world_id: String,
    pub world_name: String,
    /// 覆盖前留底的目录名（没覆盖时为空）
    pub replaced: Option<String>,
    pub files: u64,
    pub bytes: u64,
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// 路径逐段校验：不许绝对路径、不许 `..`、不许反斜杠与盘符
fn safe_rel(name: &str) -> Result<PathBuf, String> {
    if name.contains('\\') || name.contains(':') {
        return Err(format!("INVALID_PATH|包内路径不合法：{name}"));
    }
    let p = Path::new(name);
    if p.is_absolute() {
        return Err(format!("INVALID_PATH|包内含绝对路径：{name}"));
    }
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            _ => return Err(format!("INVALID_PATH|包内含上跳路径：{name}")),
        }
    }
    if out.as_os_str().is_empty() {
        return Err("INVALID_PATH|包内路径为空".into());
    }
    Ok(out)
}

/// 打开 zip 并返回条目名。顺手把「条目数超限」「符号链接条目」挡掉。
fn entry_names(zip: &mut zip::ZipArchive<std::fs::File>) -> Result<Vec<String>, String> {
    if zip.len() > MAX_ENTRIES {
        return Err(format!(
            "INVALID_VALUE|压缩包条目过多（{} 个），已拒绝",
            zip.len()
        ));
    }
    let mut out = Vec::with_capacity(zip.len());
    for i in 0..zip.len() {
        let entry = zip
            .by_index_raw(i)
            .map_err(|e| format!("INTERNAL|读取压缩包条目失败|{e}"))?;
        // 符号链接条目一律拒绝：解出来可能指到存档目录外面
        if let Some(mode) = entry.unix_mode() {
            if mode & 0o170000 == 0o120000 {
                return Err(format!(
                    "INVALID_PATH|压缩包含符号链接条目，已拒绝：{}",
                    entry
                        .name()
                        .map(|n| n.to_string())
                        .unwrap_or_else(|_| "?".to_string())
                ));
            }
        }
        let name = entry
            .name()
            .map_err(|e| format!("INTERNAL|压缩包条目名读不出来|{e}"))?
            .to_string();
        if !name.is_empty() {
            out.push(name);
        }
    }
    Ok(out)
}

/// 判断「包根只有一个目录」还是「文件直接在包根」
///
/// 玩家从网上下到的存档包两种写法都有：
/// - `MyWorld/level.dat`（整个文件夹打包）
/// - `level.dat` 直接在最外层（只打了文件夹里的内容）
fn common_root(names: &[String]) -> Option<String> {
    let mut root: Option<String> = None;
    for n in names {
        let first = n.split('/').next()?;
        // 顶层就是一个文件（不是目录）→ 没有公共根
        if !n.contains('/') {
            return None;
        }
        match &root {
            None => root = Some(first.to_string()),
            Some(r) if r == first => {}
            Some(_) => return None,
        }
    }
    root
}

/// 只看一眼，不写文件
pub fn inspect_zip(path: &Path) -> Result<WorldZipInfo, String> {
    let bytes = std::fs::metadata(path)
        .map_err(|e| format!("NOT_FOUND|找不到这个文件|{e}"))?
        .len();
    let mut info = WorldZipInfo {
        path: path.to_string_lossy().to_string(),
        file_name: file_name_of(path),
        bytes,
        root_dir: None,
        suggested_name: String::new(),
        has_level_dat: false,
        file_count: 0,
        total_bytes: 0,
        problem: None,
    };

    let stem = Path::new(&info.file_name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "world".into());
    info.suggested_name = stem.clone();

    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => {
            info.problem = Some(format!("打不开这个文件：{e}"));
            return Ok(info);
        }
    };
    let mut zip = match zip::ZipArchive::new(file) {
        Ok(z) => z,
        Err(e) => {
            info.problem = Some(format!("这不是一个有效的 zip 包：{e}"));
            return Ok(info);
        }
    };
    let names = match entry_names(&mut zip) {
        Ok(n) => n,
        Err(e) => {
            info.problem = Some(crate::task::decode(&e).message);
            return Ok(info);
        }
    };

    let root = common_root(&names);
    info.root_dir = root.clone();
    if let Some(r) = &root {
        info.suggested_name = r.clone();
    }

    let prefix = root.as_ref().map(|r| format!("{r}/"));
    let mut total = 0u64;
    let mut count = 0u64;
    for name in &names {
        if name.ends_with('/') {
            continue;
        }
        let rel = match &prefix {
            Some(p) => match name.strip_prefix(p) {
                Some(r) => r,
                None => continue,
            },
            None => name.as_str(),
        };
        if rel.is_empty() {
            continue;
        }
        if rel.eq_ignore_ascii_case("level.dat") {
            info.has_level_dat = true;
        }
        count += 1;
        if let Ok(e) = zip.by_name(name) {
            total = total.saturating_add(e.size());
        }
    }
    info.file_count = count;
    info.total_bytes = total;

    if count == 0 {
        info.problem = Some("这个压缩包是空的".into());
    } else if !info.has_level_dat {
        info.problem = Some("包里没有 level.dat，看起来不是一个存档".into());
    }
    Ok(info)
}

/// 导出存档到指定 zip。
///
/// `dest` 是玩家选的目标文件；已存在时 `overwrite` 为 false 就拒绝。
pub fn export(
    saves_dir: &Path,
    world_id: &str,
    dest: &Path,
    overwrite: bool,
    running: bool,
    cancel: crate::net::CancelRef<'_>,
    mut on: impl FnMut(u32, u32),
) -> Result<WorldExport, String> {
    if running {
        return Err("INSTANCE_BUSY|游戏正在运行，存档是写了一半的状态，先退出游戏再导出".into());
    }
    let world = crate::worlds::resolve(saves_dir, world_id)
        .ok_or_else(|| "WORLD_NOT_FOUND|存档不存在".to_string())?;
    let world_name = file_name_of(&world);
    if dest.as_os_str().is_empty() {
        return Err("INVALID_VALUE|没有指定导出到哪里".into());
    }
    if dest.is_dir() {
        return Err("INVALID_VALUE|目标是一个目录，请指定一个文件名".into());
    }
    if dest.exists() && !overwrite {
        return Err("FILE_EXISTS|这个位置已经有同名文件了".into());
    }
    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() && !parent.is_dir() {
            return Err("INVALID_VALUE|目标文件夹不存在".into());
        }
    }

    let tmp = PathBuf::from(format!("{}.part", dest.to_string_lossy()));
    let _ = std::fs::remove_file(&tmp);

    let mut stack: Vec<PathBuf> = vec![world.clone()];
    let mut files: Vec<(String, PathBuf, u64)> = Vec::new();
    let mut total = 0u64;
    while let Some(dir) = stack.pop() {
        if cancel() {
            return Err("CANCELLED|已取消".into());
        }
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            let Ok(meta) = std::fs::symlink_metadata(&p) else {
                continue;
            };
            if meta.is_dir() {
                stack.push(p);
                continue;
            }
            if !meta.is_file() {
                // 符号链接等一律跳过：导出包里不带指针
                continue;
            }
            if meta.len() > MAX_ENTRY {
                return Err(format!(
                    "INVALID_VALUE|存档内单个文件过大，已中止：{}",
                    file_name_of(&p)
                ));
            }
            total = total.saturating_add(meta.len());
            if total > MAX_TOTAL {
                return Err("INVALID_VALUE|存档体积异常，已中止".into());
            }
            let rel = match p.strip_prefix(&world) {
                Ok(r) => r.to_string_lossy().replace('\\', "/"),
                Err(_) => continue,
            };
            if rel.is_empty() {
                continue;
            }
            files.push((rel, p, meta.len()));
        }
    }
    if files.is_empty() {
        return Err("INVALID_VALUE|这个存档里没有任何文件".into());
    }
    // 排序让同样内容导出两次得到稳定的包（方便玩家比对）
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let outcome = (|| -> Result<u64, String> {
        let file =
            std::fs::File::create(&tmp).map_err(|e| format!("PERMISSION|创建导出文件失败|{e}"))?;
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        let mut buf = vec![0u8; 64 * 1024];
        let mut written = 0u32;
        for (rel, src_path, _len) in &files {
            if cancel() {
                return Err("CANCELLED|已取消".into());
            }
            zip.start_file(rel, opts)
                .map_err(|e| format!("INTERNAL|写入导出文件失败|{e}"))?;
            let mut src = std::fs::File::open(src_path)
                .map_err(|e| format!("PERMISSION|读取存档失败|{e}"))?;
            loop {
                let n = src
                    .read(&mut buf)
                    .map_err(|e| format!("PERMISSION|读取存档失败|{e}"))?;
                if n == 0 {
                    break;
                }
                zip.write_all(&buf[..n])
                    .map_err(|e| format!("PERMISSION|写入导出文件失败|{e}"))?;
            }
            written += 1;
            on(written, files.len() as u32);
        }
        zip.finish()
            .map_err(|e| format!("INTERNAL|收尾导出文件失败|{e}"))?;
        Ok(files.len() as u64)
    })();

    let count = match outcome {
        Ok(c) => c,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
    };

    if let Err(e) = std::fs::rename(&tmp, dest) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("PERMISSION|保存导出文件失败|{e}"));
    }
    let bytes = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
    Ok(WorldExport {
        path: dest.to_string_lossy().to_string(),
        world_id: world_id.to_string(),
        world_name,
        bytes,
        file_count: count,
    })
}

/// 导入一个存档 zip 到 `saves/`。
///
/// `name` 为空时用包里推出来的名字。目标已存在且 `overwrite` 为 false 就拒绝；
/// 覆盖时先把旧的改名留底，失败再改回去。
pub fn import(
    saves_dir: &Path,
    source: &Path,
    name: Option<&str>,
    overwrite: bool,
    running: bool,
    cancel: crate::net::CancelRef<'_>,
    mut on: impl FnMut(u32, u32),
) -> Result<WorldImport, String> {
    if running {
        return Err("INSTANCE_BUSY|游戏正在运行，先退出游戏再导入存档".into());
    }
    let info = inspect_zip(source)?;
    if let Some(p) = &info.problem {
        return Err(format!("INVALID_VALUE|{p}"));
    }
    let wanted = match name.map(str::trim).filter(|s| !s.is_empty()) {
        Some(n) => n.to_string(),
        None => info.suggested_name.clone(),
    };
    let target_name = crate::save_manage::validate_name(&wanted)?;

    std::fs::create_dir_all(saves_dir).map_err(|e| format!("PERMISSION|创建存档目录失败|{e}"))?;
    let target = saves_dir.join(&target_name);
    if target.exists() && !overwrite {
        return Err(format!(
            "FILE_EXISTS|已经有一个叫「{target_name}」的存档了，换个名字或选择覆盖"
        ));
    }

    let prefix = info.root_dir.as_ref().map(|r| format!("{r}/"));
    let staging = saves_dir.join(format!(".import-{:032x}", crate::time::now_nanos()));
    std::fs::create_dir_all(&staging).map_err(|e| format!("PERMISSION|创建临时目录失败|{e}"))?;

    let outcome = (|| -> Result<(u64, u64), String> {
        let file =
            std::fs::File::open(source).map_err(|e| format!("PERMISSION|打不开压缩包|{e}"))?;
        let mut zip =
            zip::ZipArchive::new(file).map_err(|e| format!("INTERNAL|不是有效 zip|{e}"))?;
        let mut total = 0u64;
        let mut files = 0u64;
        for i in 0..zip.len() {
            if cancel() {
                return Err("CANCELLED|已取消".into());
            }
            let mut entry = zip
                .by_index(i)
                .map_err(|e| format!("INTERNAL|读取条目失败|{e}"))?;
            let raw_name = entry.name().map(|n| n.to_string()).unwrap_or_default();
            if raw_name.is_empty() || raw_name.ends_with('/') {
                continue;
            }
            let rel = match &prefix {
                Some(p) => match raw_name.strip_prefix(p) {
                    Some(r) => r,
                    None => continue,
                },
                None => raw_name.as_str(),
            };
            if rel.is_empty() {
                continue;
            }
            let safe = safe_rel(rel)?;
            if entry.size() > MAX_ENTRY {
                return Err(format!("INVALID_VALUE|包内单个文件过大，已拒绝：{rel}"));
            }
            let dest = crate::net::ensure_within(&staging, &staging.join(&safe))?;
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("PERMISSION|创建目录失败|{e}"))?;
            }
            let mut out =
                std::fs::File::create(&dest).map_err(|e| format!("PERMISSION|写入失败|{e}"))?;
            let copied = std::io::copy(&mut entry, &mut out)
                .map_err(|e| format!("PERMISSION|解压失败|{e}"))?;
            drop(out);
            // 按实际写出的字节算总量，不信压缩包自称的大小
            total = total.saturating_add(copied);
            if total > MAX_TOTAL {
                return Err("INVALID_VALUE|解压体积异常，已中止".into());
            }
            files += 1;
            on(files as u32, info.file_count as u32);
        }
        Ok((files, total))
    })();

    let (files, bytes) = match outcome {
        Ok(v) => v,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
    };

    if !staging.join("level.dat").is_file() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err("INVALID_VALUE|解压后没有 level.dat，这个包不是一个存档".into());
    }

    // 目标已存在：先改名留底，不删除
    let mut replaced: Option<String> = None;
    if target.exists() {
        let keep = saves_dir.join(format!(
            "{target_name}-replaced-{:032x}",
            crate::time::now_nanos()
        ));
        if let Err(e) = std::fs::rename(&target, &keep) {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(format!("PERMISSION|覆盖前留底失败|{e}"));
        }
        replaced = keep.file_name().map(|n| n.to_string_lossy().to_string());
    }

    if let Err(e) = std::fs::rename(&staging, &target) {
        let _ = std::fs::remove_dir_all(&staging);
        // 尽力把留底的存档还原回去，避免玩家丢档
        if let (Some(k), Some(name)) = (&replaced, Some(target_name.clone())) {
            let _ = std::fs::rename(saves_dir.join(k), saves_dir.join(&name));
        }
        return Err(format!("PERMISSION|导入存档失败|{e}"));
    }

    Ok(WorldImport {
        world_id: crate::worlds::world_id(&target_name),
        world_name: target_name,
        replaced,
        files,
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("astral-worldio-{}-{tag}", crate::time::now_nanos()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 造一个存档目录：level.dat + region/r.0.0.mca
    fn make_world(saves: &Path, name: &str) {
        let w = saves.join(name);
        std::fs::create_dir_all(w.join("region")).unwrap();
        std::fs::write(
            w.join("level.dat"),
            crate::nbt::sample_level_dat("1.20.1", 0, false),
        )
        .unwrap();
        std::fs::write(w.join("region/r.0.0.mca"), vec![7u8; 32]).unwrap();
    }

    fn zip_of(dir: &Path, entries: &[(&str, &[u8])]) -> PathBuf {
        let p = dir.join(format!("pack-{}.zip", crate::time::now_nanos()));
        let f = std::fs::File::create(&p).unwrap();
        let mut z = zip::ZipWriter::new(f);
        let o = zip::write::SimpleFileOptions::default();
        for (name, data) in entries {
            z.start_file(*name, o).unwrap();
            z.write_all(data).unwrap();
        }
        z.finish().unwrap();
        p
    }

    #[test]
    fn exports_and_reads_back() {
        let d = tmp("export");
        let saves = d.join("saves");
        std::fs::create_dir_all(&saves).unwrap();
        make_world(&saves, "我的世界");
        let id = crate::worlds::world_id("我的世界");
        let out = d.join("share.zip");
        let cancel = crate::net::never_cancel();
        let r = export(&saves, &id, &out, false, false, &*cancel, |_, _| {}).unwrap();
        assert_eq!(r.world_name, "我的世界");
        assert_eq!(r.file_count, 2);
        assert!(out.is_file());

        let info = inspect_zip(&out).unwrap();
        assert!(info.problem.is_none(), "{:?}", info.problem);
        assert!(info.has_level_dat);
        assert_eq!(info.file_count, 2);
        assert_eq!(info.suggested_name, "share");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn export_refuses_while_running_and_existing() {
        let d = tmp("export-bad");
        let saves = d.join("saves");
        std::fs::create_dir_all(&saves).unwrap();
        make_world(&saves, "w1");
        let id = crate::worlds::world_id("w1");
        let out = d.join("a.zip");
        std::fs::write(&out, b"old").unwrap();
        let cancel = crate::net::never_cancel();
        assert!(export(&saves, &id, &out, false, true, &*cancel, |_, _| {}).is_err());
        assert!(export(&saves, &id, &out, false, false, &*cancel, |_, _| {}).is_err());
        // 允许覆盖时能成功
        assert!(export(&saves, &id, &out, true, false, &*cancel, |_, _| {}).is_ok());
        // 存档 id 不存在
        assert!(export(&saves, "w_deadbeef", &out, true, false, &*cancel, |_, _| {}).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn imports_a_wrapped_world() {
        let d = tmp("import");
        let saves = d.join("saves");
        std::fs::create_dir_all(&saves).unwrap();
        let lvl = crate::nbt::sample_level_dat("1.20.1", 0, false);
        let pack = zip_of(
            &d,
            &[
                ("FriendWorld/level.dat", &lvl),
                ("FriendWorld/region/r.0.0.mca", &[1u8; 16]),
            ],
        );
        let info = inspect_zip(&pack).unwrap();
        assert_eq!(info.root_dir.as_deref(), Some("FriendWorld"));
        assert_eq!(info.suggested_name, "FriendWorld");
        assert!(info.problem.is_none());

        let cancel = crate::net::never_cancel();
        let r = import(&saves, &pack, None, false, false, &*cancel, |_, _| {}).unwrap();
        assert_eq!(r.world_name, "FriendWorld");
        assert_eq!(r.files, 2);
        assert!(saves.join("FriendWorld/level.dat").is_file());
        assert!(saves.join("FriendWorld/region/r.0.0.mca").is_file());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn imports_a_flat_world_with_given_name() {
        let d = tmp("import-flat");
        let saves = d.join("saves");
        std::fs::create_dir_all(&saves).unwrap();
        let lvl = crate::nbt::sample_level_dat("1.20.1", 0, false);
        let pack = zip_of(&d, &[("level.dat", &lvl), ("region/r.0.0.mca", &[2u8; 8])]);
        let info = inspect_zip(&pack).unwrap();
        assert!(info.root_dir.is_none());
        assert!(info.has_level_dat);

        let cancel = crate::net::never_cancel();
        let r = import(
            &saves,
            &pack,
            Some("收下的档"),
            false,
            false,
            &*cancel,
            |_, _| {},
        )
        .unwrap();
        assert_eq!(r.world_name, "收下的档");
        assert!(saves.join("收下的档/level.dat").is_file());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn rejects_zips_that_are_not_worlds() {
        let d = tmp("import-bad");
        let saves = d.join("saves");
        std::fs::create_dir_all(&saves).unwrap();
        let cancel = crate::net::never_cancel();

        // 没有 level.dat
        let pack = zip_of(&d, &[("readme.txt", b"hi")]);
        let info = inspect_zip(&pack).unwrap();
        assert!(info.problem.is_some());
        assert!(import(&saves, &pack, None, false, false, &*cancel, |_, _| {}).is_err());

        // 上跳路径
        let evil = zip_of(&d, &[("../evil/level.dat", b"x")]);
        assert!(import(&saves, &evil, Some("x"), false, false, &*cancel, |_, _| {}).is_err());

        // 不是 zip
        let junk = d.join("junk.zip");
        std::fs::write(&junk, b"not a zip at all").unwrap();
        let info = inspect_zip(&junk).unwrap();
        assert!(info.problem.is_some());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn refuses_to_overwrite_without_permission() {
        let d = tmp("import-exists");
        let saves = d.join("saves");
        std::fs::create_dir_all(&saves).unwrap();
        make_world(&saves, "Existing");
        std::fs::write(saves.join("Existing/marker.txt"), b"old").unwrap();

        let lvl = crate::nbt::sample_level_dat("1.20.1", 0, false);
        let pack = zip_of(&d, &[("Existing/level.dat", &lvl)]);
        let cancel = crate::net::never_cancel();
        // 不覆盖 → 拒绝，旧存档一点没动
        assert!(import(&saves, &pack, None, false, false, &*cancel, |_, _| {}).is_err());
        assert!(saves.join("Existing/marker.txt").is_file());

        // 覆盖 → 旧的留底
        let r = import(&saves, &pack, None, true, false, &*cancel, |_, _| {}).unwrap();
        assert_eq!(r.world_name, "Existing");
        let kept = r.replaced.expect("覆盖时应该留底");
        assert!(saves.join(&kept).join("marker.txt").is_file());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn refuses_while_running() {
        let d = tmp("import-running");
        let saves = d.join("saves");
        std::fs::create_dir_all(&saves).unwrap();
        let lvl = crate::nbt::sample_level_dat("1.20.1", 0, false);
        let pack = zip_of(&d, &[("W/level.dat", &lvl)]);
        let cancel = crate::net::never_cancel();
        let e = import(&saves, &pack, None, false, true, &*cancel, |_, _| {}).unwrap_err();
        assert!(e.contains("INSTANCE_BUSY"), "{e}");
        let _ = std::fs::remove_dir_all(&d);
    }
}
