//! 实例整体快照：把一个实例的 mods / config / saves 打包成一个 zip，
//! 换电脑、手滑删了东西、想回到昨天的状态时能整个还原回来。
//!
//! 和相邻模块的分工：
//! - `world_backup` 只备份**单个存档**，粒度细、可以每天备份；
//! - `export` 导出的是**能发给别人的整合包**（Mod 走 Modrinth 链接，不含存档）；
//! - 这里是**自用备份**：把实例目录整个存下来，还原时原样放回去。
//!
//! 安全原则：
//! - **游戏运行中、或者实例有进行中的任务时一律拒绝**（不做事后无法自证一致的快照）；
//! - 还原**不删除**玩家现在的实例：先把整个目录改名搬到 `snapshots/replaced/`，再放快照；
//! - 解压前校验 zip 的 sha256，逐条校验路径防 Zip Slip，拒绝符号链接条目；
//! - 删除快照必须显式确认；
//! - `logs/` 与 `screenshots/` 不进快照（体积大、丢了不可惜），但会在记录里写明跳过了什么。

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

/// 快照存在游戏根目录下，**不能放在实例目录里** —— 否则还原时整个目录被换掉，快照自己也没了。
pub const SNAPSHOT_DIR: &str = "snapshots";
const INDEX_FILE: &str = "index.json";
const MANIFEST_FILE: &str = "snapshot.json";
const REPLACED_DIR: &str = "replaced";

/// 进快照的子目录
pub const INCLUDE_DIRS: [&str; 5] = ["mods", "config", "saves", "resourcepacks", "shaderpacks"];
/// 进快照的单个文件（不存在就跳过）
pub const INCLUDE_FILES: [&str; 4] = [
    "instance.json",
    "options.txt",
    "servers.dat",
    "optionsof.txt",
];
/// 明确不进快照的（写进记录，让玩家知道）
pub const SKIPPED_DIRS: [&str; 2] = ["logs", "screenshots"];

const MAX_ENTRY: u64 = 512 * 1024 * 1024;
const MAX_TOTAL: u64 = 8 * 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 120_000;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub snapshot_id: String,
    pub instance_id: String,
    pub instance_name: String,
    pub game_version: String,
    #[serde(default)]
    pub loader_kind: Option<String>,
    #[serde(default)]
    pub loader_version: Option<String>,
    pub created_at: String,
    /// zip 文件大小
    pub bytes: u64,
    /// 打包进去的文件数
    pub file_count: u64,
    /// 打包前这些文件的原始大小
    pub source_bytes: u64,
    /// zip 的 sha256（还原前校验）
    pub sha256: String,
    #[serde(default)]
    pub note: String,
    /// 实际收进去的目录与文件，给界面展示用
    pub parts: Vec<String>,
    /// 明确跳过的，让玩家知道快照里没有这些
    pub skipped: Vec<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotPreview {
    pub snapshot: Snapshot,
    /// 还原的目标实例目录
    pub target_dir: String,
    pub target_exists: bool,
    pub target_files: u64,
    pub target_bytes: u64,
    /// 解压后需要多少字节
    pub need_bytes: u64,
    pub available_bytes: Option<u64>,
    /// 现有实例会被搬到这里（不是删除）
    pub replaced_dir: Option<String>,
    /// 不能还原的原因（能还原时为空）
    pub blockers: Vec<String>,
    /// 能还原但玩家该知道的
    pub warnings: Vec<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RestoreOutcome {
    pub instance_id: String,
    pub restored_files: u64,
    pub bytes: u64,
    /// 旧实例搬到哪了（原来不存在时为空）
    pub replaced_dir: Option<String>,
}

fn snapshot_root(game: &Path) -> PathBuf {
    game.join(SNAPSHOT_DIR)
}

fn index_path(game: &Path) -> PathBuf {
    snapshot_root(game).join(INDEX_FILE)
}

fn zip_path(game: &Path, snapshot_id: &str) -> PathBuf {
    snapshot_root(game).join(format!("{snapshot_id}.zip"))
}

fn load_index(game: &Path) -> Vec<Snapshot> {
    let Ok(text) = std::fs::read_to_string(index_path(game)) else {
        return Vec::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

fn save_index(game: &Path, list: &[Snapshot]) -> Result<(), String> {
    let root = snapshot_root(game);
    std::fs::create_dir_all(&root).map_err(|e| format!("PERMISSION|创建快照目录失败|{e}"))?;
    let text = serde_json::to_string_pretty(list)
        .map_err(|e| format!("INTERNAL|序列化快照索引失败|{e}"))?;
    let tmp = root.join(format!("{INDEX_FILE}.part"));
    std::fs::write(&tmp, text).map_err(|e| format!("PERMISSION|写入快照索引失败|{e}"))?;
    std::fs::rename(&tmp, index_path(game))
        .map_err(|e| format!("PERMISSION|保存快照索引失败|{e}"))?;
    Ok(())
}

fn new_snapshot_id() -> String {
    format!("snap_{:x}", crate::time::now_nanos())
}

/// 校验快照 id：只允许我们自己生成的形状，防止拿它拼路径
fn check_id(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > 64 {
        return Err("INVALID_VALUE|快照编号不合法".into());
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err("INVALID_VALUE|快照编号不合法".into());
    }
    Ok(())
}

fn safe_rel(name: &str) -> Result<PathBuf, String> {
    if name.contains('\\') || name.contains(':') {
        return Err(format!("INVALID_PATH|快照内路径不合法：{name}"));
    }
    let p = Path::new(name);
    if p.is_absolute() {
        return Err(format!("INVALID_PATH|快照内含绝对路径：{name}"));
    }
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            _ => return Err(format!("INVALID_PATH|快照内含上跳路径：{name}")),
        }
    }
    if out.as_os_str().is_empty() {
        return Err("INVALID_PATH|快照内路径为空".into());
    }
    Ok(out)
}

fn dir_stats(dir: &Path) -> (u64, u64) {
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

/// 列出全部快照（最新的在前）。只保留 zip 还在的条目。
pub fn list(game: &Path) -> Vec<Snapshot> {
    let mut list = load_index(game);
    list.retain(|s| zip_path(game, &s.snapshot_id).is_file());
    list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    list
}

fn find<'a>(list: &'a [Snapshot], snapshot_id: &str) -> Option<&'a Snapshot> {
    list.iter().find(|s| s.snapshot_id == snapshot_id)
}

/// 给一个实例打快照。
///
/// `running` = 实例有没有在跑游戏；`has_task` = 有没有进行中的安装/下载任务。
/// 两者任一为 true 都拒绝：这时候文件正在被写，快照拿到的会是半成品。
pub fn create(
    game: &Path,
    instance_id: &str,
    note: &str,
    running: bool,
    has_task: bool,
    cancel: crate::net::CancelRef<'_>,
    mut on: impl FnMut(u32, u32),
) -> Result<Snapshot, String> {
    if running {
        return Err("INSTANCE_BUSY|游戏正在运行，这时候的文件是写了一半的状态，先退出游戏".into());
    }
    if has_task {
        return Err("INSTANCE_BUSY|这个实例还有进行中的任务，等它结束再备份".into());
    }
    let meta = crate::instance::read_meta(game, instance_id)
        .ok_or_else(|| "INSTANCE_NOT_FOUND|实例不存在或不是托管实例".to_string())?;
    let dir = crate::instance::instance_dir(game, instance_id);
    if !dir.is_dir() {
        return Err("INSTANCE_NOT_FOUND|实例目录不存在".into());
    }

    // 先量一遍要打多少，做磁盘预检
    let mut planned: Vec<(String, PathBuf)> = Vec::new();
    let mut source_bytes = 0u64;
    let mut file_count = 0u64;
    for name in INCLUDE_FILES {
        let p = dir.join(name);
        if p.is_file() {
            planned.push((name.to_string(), p.clone()));
            source_bytes =
                source_bytes.saturating_add(std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0));
            file_count += 1;
        }
    }
    let mut parts: Vec<String> = Vec::new();
    for name in INCLUDE_DIRS {
        let d = dir.join(name);
        if !d.is_dir() {
            continue;
        }
        parts.push(name.to_string());
        let (b, f) = dir_stats(&d);
        source_bytes = source_bytes.saturating_add(b);
        file_count += f;
    }
    if file_count == 0 {
        return Err("INVALID_VALUE|这个实例是空的，没有东西可以备份".into());
    }
    if let Some(avail) = crate::world_backup::available_space(game) {
        if avail < source_bytes.saturating_add(64 * 1024 * 1024) {
            return Err(format!(
                "DISK_FULL|磁盘空间不足|实例约 {} MB，可用 {} MB",
                source_bytes / 1024 / 1024,
                avail / 1024 / 1024
            ));
        }
    }

    let root = snapshot_root(game);
    std::fs::create_dir_all(&root).map_err(|e| format!("PERMISSION|创建快照目录失败|{e}"))?;
    let snapshot_id = new_snapshot_id();
    let final_zip = zip_path(game, &snapshot_id);
    let tmp_zip = root.join(format!("{snapshot_id}.zip.part"));

    let mut record = Snapshot {
        snapshot_id: snapshot_id.clone(),
        instance_id: instance_id.to_string(),
        instance_name: meta.name.clone(),
        game_version: meta.game_version.clone(),
        loader_kind: meta.loader_kind.clone(),
        loader_version: meta.loader_version.clone(),
        created_at: crate::time::now_iso(),
        bytes: 0,
        file_count: 0,
        source_bytes,
        sha256: String::new(),
        note: note.to_string(),
        parts: {
            let mut v: Vec<String> = INCLUDE_FILES
                .iter()
                .filter(|n| dir.join(n).is_file())
                .map(|n| n.to_string())
                .collect();
            v.extend(parts.iter().cloned());
            v
        },
        skipped: SKIPPED_DIRS
            .iter()
            .filter(|n| dir.join(n).is_dir())
            .map(|n| n.to_string())
            .collect(),
    };

    let file =
        std::fs::File::create(&tmp_zip).map_err(|e| format!("PERMISSION|创建快照文件失败|{e}"))?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    // 快照自带一份清单，这样就算索引丢了，zip 本身也说得清自己是什么
    let manifest = serde_json::to_vec_pretty(&record).unwrap_or_default();
    let manifest_result = zip
        .start_file(MANIFEST_FILE, opts)
        .map_err(|e| e.to_string())
        .and_then(|_| zip.write_all(&manifest).map_err(|e| e.to_string()));
    if let Err(e) = manifest_result {
        let _ = std::fs::remove_file(&tmp_zip);
        return Err(format!("INTERNAL|写入快照清单失败|{e}"));
    }

    let mut written = 0u64;
    let mut total = 0u64;
    let mut buf = vec![0u8; 64 * 1024];

    // 顶层文件
    for (name, path) in &planned {
        if cancel() {
            let _ = std::fs::remove_file(&tmp_zip);
            return Err("CANCELLED|已取消".into());
        }
        if let Err(e) = write_one(&mut zip, &mut buf, path, name, &mut total) {
            let _ = std::fs::remove_file(&tmp_zip);
            return Err(e);
        }
        written += 1;
        on(written as u32, file_count as u32);
    }

    // 子目录
    for name in &parts {
        let base = dir.join(name);
        let mut stack: Vec<PathBuf> = vec![base.clone()];
        while let Some(d) = stack.pop() {
            if cancel() {
                let _ = std::fs::remove_file(&tmp_zip);
                return Err("CANCELLED|已取消".into());
            }
            let Ok(rd) = std::fs::read_dir(&d) else {
                continue;
            };
            for e in rd.flatten() {
                if cancel() {
                    let _ = std::fs::remove_file(&tmp_zip);
                    return Err("CANCELLED|已取消".into());
                }
                let p = e.path();
                let Ok(m) = std::fs::symlink_metadata(&p) else {
                    continue;
                };
                if m.is_dir() {
                    stack.push(p);
                    continue;
                }
                if !m.is_file() {
                    // 符号链接等一律跳过：快照里不带指针
                    continue;
                }
                let rel = match p.strip_prefix(&dir) {
                    Ok(r) => r.to_string_lossy().replace('\\', "/"),
                    Err(_) => continue,
                };
                if let Err(e) = write_one(&mut zip, &mut buf, &p, &rel, &mut total) {
                    let _ = std::fs::remove_file(&tmp_zip);
                    return Err(e);
                }
                written += 1;
                on(written as u32, file_count as u32);
            }
        }
    }

    if let Err(e) = zip.finish() {
        let _ = std::fs::remove_file(&tmp_zip);
        return Err(format!("INTERNAL|收尾快照失败|{e}"));
    }
    std::fs::rename(&tmp_zip, &final_zip).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_zip);
        format!("PERMISSION|保存快照失败|{e}")
    })?;

    record.file_count = written;
    record.bytes = std::fs::metadata(&final_zip).map(|m| m.len()).unwrap_or(0);
    record.sha256 = crate::net::sha256_file(&final_zip).unwrap_or_default();

    let mut idx = load_index(game);
    idx.retain(|s| s.snapshot_id != record.snapshot_id);
    idx.push(record.clone());
    if let Err(e) = save_index(game, &idx) {
        // 索引写不进去就别留下一个孤儿 zip
        let _ = std::fs::remove_file(&final_zip);
        return Err(e);
    }
    Ok(record)
}

fn write_one<W: Write + std::io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    buf: &mut [u8],
    path: &Path,
    rel: &str,
    total: &mut u64,
) -> Result<(), String> {
    let Ok(meta) = std::fs::metadata(path) else {
        return Ok(());
    };
    if meta.len() > MAX_ENTRY {
        return Err(format!(
            "INVALID_VALUE|单个文件过大，已中止：{}",
            path.display()
        ));
    }
    *total = total.saturating_add(meta.len());
    if *total > MAX_TOTAL {
        return Err("INVALID_VALUE|实例体积异常，已中止".into());
    }
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    zip.start_file(rel, opts)
        .map_err(|e| format!("INTERNAL|写入快照失败|{e}"))?;
    let mut src = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return Ok(()),
    };
    loop {
        let n = src
            .read(buf)
            .map_err(|e| format!("PERMISSION|读取文件失败|{e}"))?;
        if n == 0 {
            break;
        }
        zip.write_all(&buf[..n])
            .map_err(|e| format!("PERMISSION|写入快照失败|{e}"))?;
    }
    Ok(())
}

/// 还原预览：只读。看清楚会动什么再决定。
pub fn preview_restore(game: &Path, snapshot_id: &str) -> Result<SnapshotPreview, String> {
    check_id(snapshot_id)?;
    let list = load_index(game);
    let snap = find(&list, snapshot_id)
        .cloned()
        .ok_or_else(|| "NOT_FOUND|找不到这个快照".to_string())?;
    let path = zip_path(game, snapshot_id);
    if !path.is_file() {
        return Err("NOT_FOUND|快照文件不见了（可能被手动删掉了）".into());
    }
    let target = crate::instance::instance_dir(game, &snap.instance_id);
    let target_exists = target.is_dir();
    let (target_bytes, target_files) = if target_exists {
        dir_stats(&target)
    } else {
        (0, 0)
    };

    let mut blockers: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // 校验 zip 本身
    if !snap.sha256.is_empty() {
        match crate::net::sha256_file(&path) {
            Ok(actual) if actual != snap.sha256 => {
                blockers.push("快照文件的校验值和记录对不上，文件可能已损坏".into());
            }
            Err(e) => blockers.push(format!("读不出快照文件：{e}")),
            _ => {}
        }
    }
    match std::fs::File::open(&path) {
        Ok(f) => match zip::ZipArchive::new(f) {
            Ok(a) if a.len() > MAX_ENTRIES => {
                blockers.push("快照条目过多，已拒绝".into());
            }
            Ok(_) => {}
            Err(e) => blockers.push(format!("快照不是有效的 zip：{e}")),
        },
        Err(e) => blockers.push(format!("打不开快照文件：{e}")),
    }

    let available_bytes = crate::world_backup::available_space(game);
    if let Some(avail) = available_bytes {
        // 解压要空间，同时旧实例还要留在 replaced 里，所以按两份算
        let need = snap.source_bytes.saturating_mul(2);
        if avail < need {
            blockers.push(format!(
                "磁盘空间不足：还原需要约 {} MB（含保留旧实例），可用 {} MB",
                need / 1024 / 1024,
                avail / 1024 / 1024
            ));
        }
    }

    if target_exists {
        warnings.push(format!(
            "现有实例（{} 个文件，约 {} MB）会先搬到 snapshots/replaced/，不会被删除",
            target_files,
            target_bytes / 1024 / 1024
        ));
    } else {
        warnings.push("这个实例现在不在，还原会把它重新建出来".into());
    }
    if snap.skipped.iter().any(|s| s == "logs") {
        warnings.push("快照里没有 logs/，还原后历史日志不会回来".into());
    }
    if snap.skipped.iter().any(|s| s == "screenshots") {
        warnings.push("快照里没有 screenshots/，还原后截图不会回来".into());
    }

    let replaced_dir = if target_exists {
        Some(
            snapshot_root(game)
                .join(REPLACED_DIR)
                .join(format!("{}-{}", crate::time::now_stamp(), snap.instance_id))
                .to_string_lossy()
                .to_string(),
        )
    } else {
        None
    };

    Ok(SnapshotPreview {
        need_bytes: snap.source_bytes,
        snapshot: snap,
        target_dir: target.to_string_lossy().to_string(),
        target_exists,
        target_files,
        target_bytes,
        available_bytes,
        replaced_dir,
        blockers,
        warnings,
    })
}

/// 还原：现有实例整个搬到 `snapshots/replaced/`（不删除），再把快照放回去。
pub fn restore(
    game: &Path,
    snapshot_id: &str,
    running: bool,
    has_task: bool,
    cancel: crate::net::CancelRef<'_>,
    mut on: impl FnMut(u32, u32),
) -> Result<RestoreOutcome, String> {
    if running {
        return Err("INSTANCE_BUSY|游戏正在运行，先退出游戏再还原".into());
    }
    if has_task {
        return Err("INSTANCE_BUSY|这个实例还有进行中的任务，等它结束再还原".into());
    }
    let preview = preview_restore(game, snapshot_id)?;
    if !preview.blockers.is_empty() {
        return Err(format!(
            "VERIFY_FAILED|无法还原这个快照|{}",
            preview.blockers.join("；")
        ));
    }
    let snap = &preview.snapshot;
    let path = zip_path(game, snapshot_id);
    let target = crate::instance::instance_dir(game, &snap.instance_id);

    // 先解到暂存目录：中途失败绝不能把现有实例弄坏
    let staging = snapshot_root(game).join(format!("staging-{}", crate::time::now_nanos()));
    std::fs::create_dir_all(&staging).map_err(|e| format!("PERMISSION|创建临时目录失败|{e}"))?;

    let extracted = (|| -> Result<(u64, u64), String> {
        let file = std::fs::File::open(&path).map_err(|e| format!("PERMISSION|打不开快照|{e}"))?;
        let mut zip =
            zip::ZipArchive::new(file).map_err(|e| format!("INTERNAL|快照不是有效的 zip|{e}"))?;
        if zip.len() > MAX_ENTRIES {
            return Err("INVALID_VALUE|快照条目过多，已拒绝".into());
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
                    return Err("INVALID_PATH|快照内含符号链接条目，已拒绝".into());
                }
            }
            if entry.size() > MAX_ENTRY {
                return Err("INVALID_VALUE|快照内单个文件过大，已拒绝".into());
            }
            let name = entry.name().map(|c| c.to_string()).unwrap_or_default();
            if name.is_empty() || name.ends_with('/') {
                continue;
            }
            // 清单文件不进实例目录
            if name == MANIFEST_FILE {
                continue;
            }
            let rel = safe_rel(&name)?;
            let dest = staging.join(&rel);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("PERMISSION|创建目录失败|{e}"))?;
            }
            let mut out =
                std::fs::File::create(&dest).map_err(|e| format!("PERMISSION|写入失败|{e}"))?;
            let copied = std::io::copy(&mut entry, &mut out)
                .map_err(|e| format!("PERMISSION|解压失败|{e}"))?;
            drop(out);
            total = total.saturating_add(copied);
            if total > MAX_TOTAL {
                return Err("INVALID_VALUE|快照解压体积异常，已中止".into());
            }
            written += 1;
            on(written as u32, snap.file_count as u32);
        }
        Ok((written, total))
    })();

    let (written, bytes) = match extracted {
        Ok(v) => v,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
    };

    // 暂存里必须有 instance.json，否则装回去也不是一个实例
    if !staging.join("instance.json").is_file() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err("INVALID_VALUE|快照里没有实例信息，不能还原".into());
    }

    // 现有实例搬走（不是删除）
    let mut replaced_dir: Option<String> = None;
    if target.is_dir() {
        let replaced_root = snapshot_root(game).join(REPLACED_DIR);
        std::fs::create_dir_all(&replaced_root)
            .map_err(|e| format!("PERMISSION|创建留底目录失败|{e}"))?;
        let kept = replaced_root.join(format!("{}-{}", crate::time::now_stamp(), snap.instance_id));
        std::fs::rename(&target, &kept).map_err(|e| format!("PERMISSION|留底现有实例失败|{e}"))?;
        replaced_dir = Some(kept.to_string_lossy().to_string());
    }

    if let Err(e) = std::fs::rename(&staging, &target) {
        let _ = std::fs::remove_dir_all(&staging);
        // 尽力把旧实例还原回去，不能让玩家两头空
        if let Some(k) = &replaced_dir {
            let _ = std::fs::rename(k, &target);
        }
        return Err(format!("PERMISSION|还原失败|{e}"));
    }

    Ok(RestoreOutcome {
        instance_id: snap.instance_id.clone(),
        restored_files: written,
        bytes,
        replaced_dir,
    })
}

/// 删除一个快照（必须显式确认；只删 zip 与索引条目）
pub fn delete(game: &Path, snapshot_id: &str, confirm: bool) -> Result<u64, String> {
    if !confirm {
        return Err("UNSUPPORTED|删除快照需要明确确认".into());
    }
    check_id(snapshot_id)?;
    let mut list = load_index(game);
    let before = list.len();
    let Some(rec) = find(&list, snapshot_id).cloned() else {
        return Err("NOT_FOUND|找不到这个快照".into());
    };
    let path = zip_path(game, snapshot_id);
    let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    if path.is_file() {
        std::fs::remove_file(&path).map_err(|e| format!("PERMISSION|删除快照失败|{e}"))?;
    }
    list.retain(|s| s.snapshot_id != rec.snapshot_id);
    if list.len() == before {
        return Err("INTERNAL|快照索引没有变化".into());
    }
    save_index(game, &list)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_game(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "astral-snap-{}-{}-{}",
            tag,
            std::process::id(),
            crate::time::now_nanos()
        ));
        let _ = std::fs::create_dir_all(&d);
        d
    }

    fn make_instance(game: &Path, id: &str) {
        let meta = crate::instance::InstanceMeta {
            id: id.to_string(),
            name: "测试实例".into(),
            game_version: "1.20.1".into(),
            loader_kind: Some("fabric".into()),
            loader_version: Some("0.16.9".into()),
            launch_version: None,
            memory_mb: None,
            jvm_args: Vec::new(),
            java_path: None,
            cover: None,
            created_at: crate::time::now_iso(),
        };
        let _ = crate::instance::create(game, &meta);
        let dir = crate::instance::instance_dir(game, id);
        let _ = std::fs::write(dir.join("mods").join("a.jar"), b"mod-a");
        let _ = std::fs::write(dir.join("mods").join("b.jar"), b"mod-bb");
        let _ = std::fs::write(dir.join("config").join("x.toml"), b"k=1");
        let _ = std::fs::create_dir_all(dir.join("saves").join("world1"));
        let _ = std::fs::write(dir.join("saves").join("world1").join("level.dat"), b"nbt");
        let _ = std::fs::write(dir.join("options.txt"), b"fov:70\n");
    }

    #[test]
    fn rejects_missing_instance() {
        let g = tmp_game("missing");
        let e = create(&g, "inst_nope", "", false, false, &|| false, |_, _| {}).unwrap_err();
        assert!(e.starts_with("INSTANCE_NOT_FOUND|"), "{e}");
        let _ = std::fs::remove_dir_all(&g);
    }

    #[test]
    fn refuses_while_running() {
        let g = tmp_game("running");
        make_instance(&g, "inst_a");
        let e = create(&g, "inst_a", "", true, false, &|| false, |_, _| {}).unwrap_err();
        assert!(e.starts_with("INSTANCE_BUSY|"), "{e}");
        let _ = std::fs::remove_dir_all(&g);
    }

    #[test]
    fn creates_and_lists_a_snapshot() {
        let g = tmp_game("create");
        make_instance(&g, "inst_a");
        let snap = create(
            &g,
            "inst_a",
            "第一次备份",
            false,
            false,
            &|| false,
            |_, _| {},
        )
        .unwrap();
        assert_eq!(snap.instance_id, "inst_a");
        assert_eq!(snap.note, "第一次备份");
        assert!(snap.file_count >= 5, "file_count = {}", snap.file_count);
        assert!(!snap.sha256.is_empty());
        assert!(snap.bytes > 0);
        assert!(zip_path(&g, &snap.snapshot_id).is_file());
        // instance.json 与 options.txt 都该在
        assert!(snap.parts.iter().any(|p| p == "instance.json"));
        assert!(snap.parts.iter().any(|p| p == "options.txt"));
        assert!(snap.parts.iter().any(|p| p == "saves"));

        let list = list(&g);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].snapshot_id, snap.snapshot_id);
        let _ = std::fs::remove_dir_all(&g);
    }

    #[test]
    fn skips_logs_and_screenshots() {
        let g = tmp_game("skip");
        make_instance(&g, "inst_a");
        let dir = crate::instance::instance_dir(&g, "inst_a");
        let _ = std::fs::write(dir.join("logs").join("latest.log"), b"hello");
        let _ = std::fs::write(dir.join("screenshots").join("a.png"), b"png");
        let snap = create(&g, "inst_a", "", false, false, &|| false, |_, _| {}).unwrap();
        assert!(snap.skipped.iter().any(|s| s == "logs"));
        assert!(snap.skipped.iter().any(|s| s == "screenshots"));
        assert!(!snap.parts.iter().any(|p| p == "logs"));
        assert!(!snap.parts.iter().any(|p| p == "screenshots"));
        let _ = std::fs::remove_dir_all(&g);
    }

    #[test]
    fn restore_round_trip_brings_files_back() {
        let g = tmp_game("roundtrip");
        make_instance(&g, "inst_a");
        let snap = create(&g, "inst_a", "", false, false, &|| false, |_, _| {}).unwrap();
        let dir = crate::instance::instance_dir(&g, "inst_a");
        // 弄坏：删掉一个 mod、改掉 options
        let _ = std::fs::remove_file(dir.join("mods").join("a.jar"));
        let _ = std::fs::write(dir.join("options.txt"), b"fov:30\n");

        let out = restore(&g, &snap.snapshot_id, false, false, &|| false, |_, _| {}).unwrap();
        assert!(out.restored_files >= 5);
        assert!(dir.join("mods").join("a.jar").is_file());
        assert_eq!(std::fs::read(dir.join("options.txt")).unwrap(), b"fov:70\n");
        let _ = std::fs::remove_dir_all(&g);
    }

    #[test]
    fn restore_keeps_the_old_instance() {
        let g = tmp_game("keepold");
        make_instance(&g, "inst_a");
        let snap = create(&g, "inst_a", "", false, false, &|| false, |_, _| {}).unwrap();
        let dir = crate::instance::instance_dir(&g, "inst_a");
        let _ = std::fs::write(dir.join("mods").join("only-here.jar"), b"x");

        let out = restore(&g, &snap.snapshot_id, false, false, &|| false, |_, _| {}).unwrap();
        let kept = out.replaced_dir.expect("应该留底");
        assert!(Path::new(&kept)
            .join("mods")
            .join("only-here.jar")
            .is_file());
        // 还原后的实例里不该有那个文件
        assert!(!dir.join("mods").join("only-here.jar").exists());
        let _ = std::fs::remove_dir_all(&g);
    }

    #[test]
    fn restore_recreates_a_deleted_instance() {
        let g = tmp_game("recreate");
        make_instance(&g, "inst_a");
        let snap = create(&g, "inst_a", "", false, false, &|| false, |_, _| {}).unwrap();
        let dir = crate::instance::instance_dir(&g, "inst_a");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!dir.exists());

        let preview = preview_restore(&g, &snap.snapshot_id).unwrap();
        assert!(!preview.target_exists);
        assert!(preview.blockers.is_empty(), "{:?}", preview.blockers);

        let out = restore(&g, &snap.snapshot_id, false, false, &|| false, |_, _| {}).unwrap();
        assert!(out.replaced_dir.is_none());
        assert!(dir.join("instance.json").is_file());
        assert!(dir.join("mods").join("a.jar").is_file());
        let _ = std::fs::remove_dir_all(&g);
    }

    #[test]
    fn delete_needs_confirm() {
        let g = tmp_game("del");
        make_instance(&g, "inst_a");
        let snap = create(&g, "inst_a", "", false, false, &|| false, |_, _| {}).unwrap();
        let e = delete(&g, &snap.snapshot_id, false).unwrap_err();
        assert!(e.starts_with("UNSUPPORTED|"), "{e}");
        assert!(zip_path(&g, &snap.snapshot_id).is_file());
        delete(&g, &snap.snapshot_id, true).unwrap();
        assert!(!zip_path(&g, &snap.snapshot_id).exists());
        assert!(list(&g).is_empty());
        let _ = std::fs::remove_dir_all(&g);
    }

    #[test]
    fn rejects_bad_snapshot_ids() {
        assert!(check_id("").is_err());
        assert!(check_id("../evil").is_err());
        assert!(check_id("a/b").is_err());
        assert!(check_id("a:b").is_err());
        assert!(check_id("snap_1234abcd").is_ok());
    }

    #[test]
    fn safe_rel_blocks_zip_slip() {
        assert!(safe_rel("mods/a.jar").is_ok());
        assert!(safe_rel("../evil").is_err());
        assert!(safe_rel("a/../../b").is_err());
        assert!(safe_rel("/abs").is_err());
        assert!(safe_rel("C:/win").is_err());
        assert!(safe_rel("a\\b").is_err());
    }

    #[test]
    fn corrupt_zip_is_blocked() {
        let g = tmp_game("corrupt");
        make_instance(&g, "inst_a");
        let snap = create(&g, "inst_a", "", false, false, &|| false, |_, _| {}).unwrap();
        // 把 zip 改坏
        let p = zip_path(&g, &snap.snapshot_id);
        let _ = std::fs::write(&p, b"not a zip at all");
        let preview = preview_restore(&g, &snap.snapshot_id).unwrap();
        assert!(!preview.blockers.is_empty(), "坏包应该被拦住");
        let e = restore(&g, &snap.snapshot_id, false, false, &|| false, |_, _| {}).unwrap_err();
        assert!(e.starts_with("VERIFY_FAILED|"), "{e}");
        let _ = std::fs::remove_dir_all(&g);
    }
}
