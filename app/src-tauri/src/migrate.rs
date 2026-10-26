//! 版本复制、导入与迁移（C3）。
//!
//! 三类操作严格分开，绝不混为一谈：
//! - **复制实例**：在 `instances/` 里再建一个独立实例，可勾选带哪些内容；
//! - **导入本地版本**：把 `versions/<ver>/` 里已装好的版本变成托管实例（只读扫描原目录，不移动不删除）；
//! - **迁移**：把兼容实例（共享 `.minecraft` 根目录）的配置复制进新实例。
//!
//! 铁律：
//! - 不复制整个共享 `.minecraft`（只按白名单复制子目录）；
//! - 导入默认**不移动、不删除**原文件；
//! - 目标 id/目录冲突时拒绝或换新 id，绝不覆盖；
//! - 先算计划（只读），玩家确认后才执行。

use serde::{Deserialize, Serialize};
use std::path::Path;

/// 可复制的内容类别（互不包含）
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct CopyOptions {
    /// mods + config（默认开）
    #[serde(default)]
    pub settings: bool,
    /// saves（存档，默认关：体积大且容易撞名）
    #[serde(default)]
    pub worlds: bool,
    /// resourcepacks + shaderpacks（默认关）
    #[serde(default)]
    pub resources: bool,
    /// screenshots（默认关）
    #[serde(default)]
    pub screenshots: bool,
    /// logs（默认关）
    #[serde(default)]
    pub logs: bool,
    /// 覆盖同名文件（默认关；关掉时同名文件跳过并记入 skipped）
    #[serde(default)]
    pub overwrite: bool,
}

impl CopyOptions {
    pub fn settings_only() -> Self {
        Self {
            settings: true,
            ..Default::default()
        }
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CopyPlanItem {
    pub category: String,
    pub from: String,
    pub to: String,
    pub files: u64,
    pub bytes: u64,
    /// 目标里已有同名文件的数量（overwrite=false 时会被跳过）
    pub conflicts: u64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CopyPlan {
    pub plan_id: String,
    pub kind: String,
    pub source_instance: Option<String>,
    pub source_version: Option<String>,
    pub target_name: String,
    pub items: Vec<CopyPlanItem>,
    pub total_files: u64,
    pub total_bytes: u64,
    /// 目标盘可用空间（拿不到时为空）
    pub available_bytes: Option<u64>,
    pub issues: Vec<String>,
    pub created_at: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CopyResult {
    pub instance_id: String,
    pub instance_name: String,
    pub files: u64,
    pub bytes: u64,
    pub skipped: Vec<String>,
    pub warnings: Vec<String>,
}

/// 复制计划缓存（与 plan.rs 同款两段式：先预览，再执行）
static PLANS: std::sync::Mutex<Option<std::collections::HashMap<String, StoredCopyPlan>>> =
    std::sync::Mutex::new(None);

pub struct StoredCopyPlan {
    pub plan: CopyPlan,
    pub options: CopyOptions,
    pub meta: crate::instance::InstanceMeta,
    pub created_ms: i64,
}

const TTL_MS: i64 = 30 * 60 * 1000;

fn store() -> std::sync::MutexGuard<'static, Option<std::collections::HashMap<String, StoredCopyPlan>>> {
    PLANS.lock().unwrap()
}

fn new_plan_id() -> String {
    format!("cplan_{:x}", crate::time::now_unix_ms())
}

/// 类别 → 相对路径列表（白名单，永远不含整个 .minecraft）
fn category_dirs(category: &str) -> &'static [&'static str] {
    match category {
        "settings" => &["mods", "config"],
        "worlds" => &["saves"],
        "resources" => &["resourcepacks", "shaderpacks"],
        "screenshots" => &["screenshots"],
        "logs" => &["logs"],
        _ => &[],
    }
}

fn selected_categories(opts: &CopyOptions) -> Vec<&'static str> {
    let mut out = Vec::new();
    if opts.settings {
        out.push("settings");
    }
    if opts.worlds {
        out.push("worlds");
    }
    if opts.resources {
        out.push("resources");
    }
    if opts.screenshots {
        out.push("screenshots");
    }
    if opts.logs {
        out.push("logs");
    }
    out
}

fn count_tree(dir: &Path) -> (u64, u64) {
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
    (files, bytes)
}


/// 计算复制计划（只读，不创建任何目录）
pub fn plan_copy(
    game: &Path,
    instance_id: &str,
    target_name: &str,
    opts: &CopyOptions,
    stats: &dyn Fn(&str) -> crate::instance::PlayStats,
) -> Result<CopyPlan, String> {
    let src = crate::instance::find(game, instance_id, stats)
        .ok_or_else(|| "INSTANCE_NOT_FOUND|实例不存在".to_string())?;
    if !src.managed {
        return Err("UNSUPPORTED|兼容实例来自共享游戏目录，不能整体复制；请改用导入版本".into());
    }
    let name = target_name.trim();
    crate::instance::validate_name(name)?;
    let src_dir = crate::instance::instance_dir(game, &instance_id);

    let mut items = Vec::new();
    let mut total_files = 0u64;
    let mut total_bytes = 0u64;
    for cat in selected_categories(opts) {
        for rel in category_dirs(cat) {
            let from = src_dir.join(rel);
            if !from.is_dir() {
                continue;
            }
            let (files, bytes) = count_tree(&from);
            if files == 0 {
                continue;
            }
            // 目标实例还不存在，冲突只可能是同名实例目录；这里统计为 0，
            // 真正冲突在 execute 里逐文件判断。
            items.push(CopyPlanItem {
                category: cat.to_string(),
                from: from.to_string_lossy().to_string(),
                to: rel.to_string(),
                files,
                bytes,
                conflicts: 0,
            });
            total_files += files;
            total_bytes += bytes;
        }
    }
    if items.is_empty() {
        return Err("INVALID_VALUE|没有可复制的内容，请至少勾选一项".into());
    }
    let mut issues = Vec::new();
    if !opts.settings {
        issues.push("没有勾选 mods/config，新实例会是干净的".to_string());
    }
    if opts.worlds {
        issues.push("会复制存档；如果新实例版本不同，旧存档可能无法加载".to_string());
    }
    let avail = crate::world_backup::available_space(&src_dir);
    if let Some(a) = avail {
        if a < total_bytes.saturating_add(16 * 1024 * 1024) {
            issues.push(format!(
                "磁盘空间可能不足：需要约 {} MB，可用 {} MB",
                total_bytes / 1024 / 1024,
                a / 1024 / 1024
            ));
        }
    }

    let meta = crate::instance::InstanceMeta {
        id: crate::instance::new_id(),
        name: name.to_string(),
        game_version: src.game_version.clone(),
        loader_kind: src.loader.as_ref().map(|l| l.kind.clone()),
        loader_version: src.loader.as_ref().map(|l| l.version.clone()),
        launch_version: src.launch_version.clone(),
        memory_mb: src.memory_mb,
        jvm_args: src.jvm_args.clone(),
        java_path: src.java_path.clone(),
        cover: src.cover.clone(),
        created_at: crate::time::now_iso(),
    };
    let plan = CopyPlan {
        plan_id: new_plan_id(),
        kind: "copy".to_string(),
        source_instance: Some(instance_id.to_string()),
        source_version: Some(src.game_version.clone()),
        target_name: name.to_string(),
        items,
        total_files,
        total_bytes,
        available_bytes: avail,
        issues,
        created_at: crate::time::now_iso(),
    };
    let mut s = store();
    let map = s.get_or_insert_with(std::collections::HashMap::new);
    let now = crate::time::now_unix_ms();
    map.retain(|_, p| now - p.created_ms < TTL_MS);
    map.insert(
        plan.plan_id.clone(),
        StoredCopyPlan {
            plan: plan.clone(),
            options: opts.clone(),
            meta,
            created_ms: now,
        },
    );
    Ok(plan)
}

/// 扫描 `versions/` 下可以导入的版本（只读）
pub fn scan_importable(game: &Path) -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    let root = game.join("versions");
    let Ok(rd) = std::fs::read_dir(&root) else {
        return out;
    };
    for e in rd.flatten() {
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        let json = p.join(format!("{name}.json"));
        if !json.is_file() {
            continue;
        }
        let has_jar = p.join(format!("{name}.jar")).is_file();
        let (files, bytes) = count_tree(&p);
        let mut loader = None;
        let mut inherits = None;
        if let Ok(text) = std::fs::read_to_string(&json) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                inherits = v
                    .get("inheritsFrom")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("");
                let low = id.to_lowercase();
                for k in ["fabric", "quilt", "forge", "neoforge"] {
                    if low.contains(k) {
                        loader = Some(k.to_string());
                        break;
                    }
                }
            }
        }
        out.push(serde_json::json!({
            "version": name,
            "hasJar": has_jar,
            "inheritsFrom": inherits,
            "loaderKind": loader,
            "files": files,
            "bytes": bytes,
            "alreadyImported": crate::instance::discover(game, &|_| crate::instance::PlayStats::default())
                .iter()
                .any(|i| !i.managed && i.game_version == name),
        }));
    }
    out.sort_by(|a, b| {
        a["version"]
            .as_str()
            .unwrap_or("")
            .cmp(b["version"].as_str().unwrap_or(""))
    });
    out
}

/// 把 `versions/<ver>/` 变成托管实例（只读扫描，不移动原文件）
pub fn plan_import(game: &Path, version: &str, name: Option<&str>) -> Result<CopyPlan, String> {
    let dir = game.join("versions").join(version);
    if !dir.is_dir() || !dir.join(format!("{version}.json")).is_file() {
        return Err(format!("VERSION_NOT_FOUND|本地没有版本 {version}"));
    }
    let display = name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| version.to_string());
    crate::instance::validate_name(&display)?;
    let (files, bytes) = count_tree(&dir);
    let plan = CopyPlan {
        plan_id: new_plan_id(),
        kind: "import".to_string(),
        source_instance: None,
        source_version: Some(version.to_string()),
        target_name: display.clone(),
        items: vec![CopyPlanItem {
            category: "version".to_string(),
            from: dir.to_string_lossy().to_string(),
            to: format!("versions/{version}"),
            files,
            bytes,
            conflicts: 0,
        }],
        total_files: files,
        total_bytes: bytes,
        available_bytes: crate::world_backup::available_space(game),
        issues: vec![
            "导入只创建实例记录并指向本地版本，不会移动或删除原文件".to_string(),
            "版本文件仍由共享游戏目录提供，删掉 versions 里的文件会影响这个实例".to_string(),
        ],
        created_at: crate::time::now_iso(),
    };
    let mut loader_kind = None;
    let mut loader_version = None;
    let mut launch_version = None;
    if let Ok(text) = std::fs::read_to_string(dir.join(format!("{version}.json"))) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            let id = v.get("id").and_then(|x| x.as_str()).unwrap_or(version);
            let low = id.to_lowercase();
            for k in ["fabric", "quilt", "forge", "neoforge"] {
                if low.contains(k) {
                    loader_kind = Some(k.to_string());
                    loader_version = Some(version.to_string());
                    launch_version = Some(version.to_string());
                    break;
                }
            }
        }
    }
    let meta = crate::instance::InstanceMeta {
        id: crate::instance::new_id(),
        name: display,
        game_version: version.to_string(),
        loader_kind,
        loader_version,
        launch_version,
        memory_mb: None,
        jvm_args: vec![],
        java_path: None,
        cover: None,
        created_at: crate::time::now_iso(),
    };
    let mut s = store();
    let map = s.get_or_insert_with(std::collections::HashMap::new);
    let now = crate::time::now_unix_ms();
    map.retain(|_, p| now - p.created_ms < TTL_MS);
    map.insert(
        plan.plan_id.clone(),
        StoredCopyPlan {
            plan: plan.clone(),
            options: CopyOptions::default(),
            meta,
            created_ms: now,
        },
    );
    Ok(plan)
}

/// 执行复制计划：创建实例 → 按类别复制文件
pub fn execute(
    game: &Path,
    plan_id: &str,
    cancel: crate::net::CancelRef<'_>,
    mut on: impl FnMut(u32, u32),
) -> Result<CopyResult, String> {
    let stored = {
        let mut s = store();
        let map = s
            .as_mut()
            .ok_or_else(|| "PLAN_EXPIRED|没有可用计划".to_string())?;
        let st = map
            .remove(plan_id)
            .ok_or_else(|| "PLAN_EXPIRED|计划不存在或已使用".to_string())?;
        let now = crate::time::now_unix_ms();
        if now - st.created_ms >= TTL_MS {
            return Err("PLAN_EXPIRED|计划已过期".into());
        }
        st
    };
    let plan = &stored.plan;
    if plan.kind == "import" {
        // 导入：只写 instance.json，不复制任何版本文件
        let inst = crate::instance::create(game, &stored.meta)?;
        return Ok(CopyResult {
            instance_id: inst.id,
            instance_name: inst.name,
            files: 0,
            bytes: 0,
            skipped: vec![],
            warnings: vec!["版本文件仍在共享游戏目录，删除它们会导致实例无法启动".to_string()],
        });
    }

    let src_id = plan
        .source_instance
        .clone()
        .ok_or_else(|| "INTERNAL|计划缺少源实例".to_string())?;
    let src_dir = crate::instance::instance_dir(game, &src_id);
    let inst = crate::instance::create(game, &stored.meta)?;
    let dst_dir = crate::instance::instance_dir(game, &inst.id);

    let mut files = 0u64;
    let mut bytes = 0u64;
    let mut skipped: Vec<String> = Vec::new();
    let total = plan.total_files.max(1);

    let result = (|| -> Result<(), String> {
        for cat in selected_categories(&stored.options) {
            for rel in category_dirs(cat) {
                let from = src_dir.join(rel);
                if !from.is_dir() {
                    continue;
                }
                copy_tree(
                    &from,
                    &dst_dir.join(rel),
                    &dst_dir,
                    stored.options.overwrite,
                    cancel,
                    &mut files,
                    &mut bytes,
                    &mut skipped,
                    &mut on,
                    total,
                )?;
            }
        }
        Ok(())
    })();

    match result {
        Ok(()) => Ok(CopyResult {
            instance_id: inst.id,
            instance_name: inst.name,
            files,
            bytes,
            skipped,
            warnings: vec![],
        }),
        Err(e) => {
            // 失败回滚：删掉本次新建的实例目录
            let _ = crate::instance::delete(game, &inst.id, false);
            Err(e)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn copy_tree(
    from: &Path,
    to: &Path,
    instance_root: &Path,
    overwrite: bool,
    cancel: crate::net::CancelRef<'_>,
    files: &mut u64,
    bytes: &mut u64,
    skipped: &mut Vec<String>,
    on: &mut impl FnMut(u32, u32),
    total: u64,
) -> Result<(), String> {
    let mut stack = vec![(from.to_path_buf(), to.to_path_buf())];
    while let Some((s, d)) = stack.pop() {
        if cancel() {
            return Err("CANCELLED|已取消".into());
        }
        let Ok(rd) = std::fs::read_dir(&s) else {
            continue;
        };
        std::fs::create_dir_all(&d).map_err(|e| format!("PERMISSION|创建目录失败|{e}"))?;
        for e in rd.flatten() {
            if cancel() {
                return Err("CANCELLED|已取消".into());
            }
            let sp = e.path();
            let Ok(m) = std::fs::symlink_metadata(&sp) else {
                continue;
            };
            let dp = d.join(e.file_name());
            if m.is_dir() {
                stack.push((sp, dp));
                continue;
            }
            if !m.is_file() {
                // 符号链接等不复制，避免把指向外部的指针带进新实例
                skipped.push(format!("{}（不是普通文件）", sp.display()));
                continue;
            }
            if dp.exists() && !overwrite {
                skipped.push(format!("{}（已存在）", dp.display()));
                continue;
            }
            // 目标路径必须留在实例目录内（防目录名里的 .. 或链接）
            crate::net::ensure_within(instance_root, &dp)?;
            if let Some(parent) = dp.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("PERMISSION|创建目录失败|{e}"))?;
            }
            let tmp = dp.with_extension("astral-part");
            std::fs::copy(&sp, &tmp).map_err(|e| format!("PERMISSION|复制失败|{e}"))?;
            if std::fs::rename(&tmp, &dp).is_err() {
                let _ = std::fs::remove_file(&tmp);
                return Err(format!("PERMISSION|保存文件失败|{}", dp.display()));
            }
            *files += 1;
            *bytes += m.len();
            on((*files).min(u32::MAX as u64) as u32, total.min(u32::MAX as u64) as u32);
        }
    }
    Ok(())
}

#[allow(dead_code)]
/// 删除计划缓存里过期的条目（不影响磁盘）
pub fn prune_plans() {
    let mut s = store();
    if let Some(map) = s.as_mut() {
        let now = crate::time::now_unix_ms();
        map.retain(|_, p| now - p.created_ms < TTL_MS);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_are_whitelisted() {
        assert!(category_dirs("settings").contains(&"mods"));
        assert!(!category_dirs("settings").contains(&"."));
        assert!(category_dirs("unknown").is_empty());
    }

    #[test]
    fn settings_only_selects_settings() {
        let cats = selected_categories(&CopyOptions::settings_only());
        assert_eq!(cats, vec!["settings"]);
        let all = CopyOptions {
            settings: true,
            worlds: true,
            resources: true,
            screenshots: true,
            logs: true,
            overwrite: false,
        };
        assert_eq!(selected_categories(&all).len(), 5);
    }

    #[test]
    fn plan_id_prefix() {
        assert!(new_plan_id().starts_with("cplan_"));
    }
}
