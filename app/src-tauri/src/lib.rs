mod account;
mod cache;
mod components;
mod core;
mod deps;
mod error;
mod export;
mod instance;
mod java;
mod java_audit;
mod jdk;
mod ledger;
mod live;
mod loader;
mod logs;
mod migrate;
mod modpack;
mod mods;
mod modscan;
mod nbt;
mod net;
mod packs;
mod plan;
mod recycle;
mod resources;
mod search;
mod server;
mod servers;
mod session;
mod stats;
mod task;
mod testsrv;
mod time;
mod update_plan;
mod updates;
mod verify;
mod world_backup;
mod worlds;

use core::Config;
use instance::Instance;
use serde::{Deserialize, Serialize};
use session::GameSession;
use std::path::PathBuf;
use task::Task;
use tauri::{AppHandle, Emitter};

const API_VERSION: u32 = 3;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    os: String,
    cpu_cores: usize,
    total_mem_gb: f64,
    free_mem_gb: f64,
    demo: bool,
}

fn gb(bytes: u64) -> f64 {
    ((bytes as f64 / 1024.0 / 1024.0 / 1024.0) * 10.0).round() / 10.0
}

fn game_root() -> PathBuf {
    PathBuf::from(core::load_config().game_dir)
}

/// 实例增删改后广播，界面不必轮询（契约 v3 §6.3）
fn emit_instance_changed(instance_id: &str, change: &str) {
    if let Some(app) = APP_HANDLE.get() {
        let _ = app.emit(
            "astral://instance-changed",
            serde_json::json!({ "instanceId": instance_id, "change": change }),
        );
    }
}

fn play_stats(id: &str) -> instance::PlayStats {
    stats::play_stats(id)
}

// ── 基础 ────────────────────────────────────────────────────────

#[tauri::command]
fn get_api_info() -> serde_json::Value {
    serde_json::json!({
        "apiVersion": API_VERSION,
        "appVersion": env!("CARGO_PKG_VERSION"),
        "features": ["instances", "sessions", "tasks", "saves", "stats", "mods", "loaders", "loader-builds", "worlds", "game-install", "resources", "logs", "modpack", "java", "components", "accounts", "updates", "mirror"],
    })
}

#[tauri::command]
fn get_system_info() -> SystemInfo {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    SystemInfo {
        os: "Windows".to_string(),
        cpu_cores: std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(0),
        total_mem_gb: gb(sys.total_memory()),
        free_mem_gb: gb(sys.available_memory()),
        demo: false,
    }
}

#[tauri::command]
fn get_config() -> Config {
    core::load_config()
}

/// 部分更新：只覆盖出现的字段
#[tauri::command]
fn set_config(config: serde_json::Value) -> Result<Config, String> {
    let mut cur = core::load_config();
    let mut merged = serde_json::to_value(&cur).map_err(|e| format!("INTERNAL|序列化失败|{e}"))?;
    if let (Some(base), Some(patch)) = (merged.as_object_mut(), config.as_object()) {
        for (k, v) in patch {
            base.insert(k.clone(), v.clone());
        }
    }
    cur = serde_json::from_value(merged).map_err(|e| format!("INVALID_NAME|配置格式不对|{e}"))?;
    // 用户名会进 JVM 参数，必须是合法 ASCII，否则 portablemc 截断时可能 panic
    account::validate_username(&cur.username)?;
    if let Some(id) = &cur.msa_client_id {
        if !id.trim().is_empty() {
            account::validate_client_id(id)?;
        }
    }
    core::save_config(&cur)?;
    Ok(cur)
}

#[tauri::command]
fn scan_versions() -> Vec<String> {
    core::scan_installed(&game_root())
}

#[tauri::command]
fn open_path(path: String) -> Result<(), String> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return error::fail(error::PERMISSION, format!("路径不存在: {path}"));
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&p)
            .spawn()
            .map_err(|e| format!("PERMISSION|打开目录失败|{e}"))?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        return error::fail(error::UNSUPPORTED, "仅支持 Windows");
    }
    Ok(())
}

#[tauri::command]
async fn list_versions() -> Result<Vec<core::VersionSummary>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let cfg = core::load_config();
        core::list_versions(&cfg.source)
    })
    .await
    .map_err(|e| e.to_string())?
}

// ── 实例 ────────────────────────────────────────────────────────

#[tauri::command]
fn list_instances() -> Vec<Instance> {
    instance::discover(&game_root(), &play_stats)
}

#[tauri::command]
fn get_instance(id: String) -> Option<Instance> {
    instance::find(&game_root(), &id, &play_stats)
}

#[tauri::command]
fn get_active_instance() -> Option<Instance> {
    let id = core::load_config().active_instance_id?;
    instance::find(&game_root(), &id, &play_stats)
}

#[tauri::command]
fn set_active_instance(id: Option<String>) -> Result<Config, String> {
    let mut cfg = core::load_config();
    if let Some(ref want) = id {
        if instance::find(&game_root(), want, &play_stats).is_none() {
            return error::fail(error::INSTANCE_NOT_FOUND, "实例不存在");
        }
    }
    cfg.active_instance_id = id;
    core::save_config(&cfg)?;
    Ok(cfg)
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct InstancePatch {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    memory_mb: Option<Option<u32>>,
    #[serde(default)]
    jvm_args: Option<Vec<String>>,
    #[serde(default)]
    java_path: Option<Option<String>>,
    #[serde(default)]
    cover: Option<Option<String>>,
}

#[tauri::command]
fn update_instance(id: String, patch: InstancePatch) -> Result<Instance, String> {
    let game = game_root();
    // 运行中或有任务时不允许修改（改名会让启动/安装期间的路径漂移）
    if session::active(Some(&id)).is_some() {
        return error::fail(error::INSTANCE_BUSY, "实例正在运行，先退出游戏");
    }
    if task::has_running(&id) {
        return error::fail(error::INSTANCE_BUSY, "实例有进行中的任务");
    }
    let mut meta = instance::read_meta(&game, &id)
        .ok_or_else(|| "INSTANCE_NOT_FOUND|实例不存在或不是托管实例".to_string())?;
    if let Some(name) = patch.name {
        instance::validate_name(&name)?;
        meta.name = name.trim().to_string();
    }
    if let Some(mem) = patch.memory_mb {
        meta.memory_mb = mem;
    }
    if let Some(args) = patch.jvm_args {
        meta.jvm_args = args;
    }
    if let Some(java) = patch.java_path {
        meta.java_path = java;
    }
    if let Some(cover) = patch.cover {
        meta.cover = cover;
    }
    instance::save_meta(&game, &meta)?;
    emit_instance_changed(&id, "updated");
    instance::find(&game, &id, &play_stats).ok_or_else(|| "INTERNAL|保存后读不到实例".to_string())
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct DeleteOpts {
    #[serde(default)]
    trash: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateInstanceRequest {
    name: String,
    game_version: String,
    #[serde(default)]
    loader_kind: Option<String>,
    #[serde(default)]
    loader_version: Option<String>,
    #[serde(default)]
    memory_mb: Option<u32>,
}

/// 创建托管实例：只建目录与元数据；缺版本文件时由 health 暴露，不静默下载
#[tauri::command]
async fn start_create_instance(request: CreateInstanceRequest) -> Result<Task, String> {
    instance::validate_name(&request.name)?;
    if task::has_running_kind("create", None, Some(&request.game_version)) {
        return Err("BUSY|同类任务正在运行，请等待或取消".into());
    }
    let t = task::create(
        "create",
        None,
        Some(request.game_version.clone()),
        format!("创建实例 {}", request.name.trim()),
    );
    let tid = t.task_id.clone();
    std::thread::spawn(move || {
        task::start(&tid);
        let game = game_root();
        let meta = instance::InstanceMeta {
            id: instance::new_id(),
            name: request.name.trim().to_string(),
            game_version: request.game_version,
            loader_kind: request.loader_kind,
            loader_version: request.loader_version,
            launch_version: None,
            memory_mb: request.memory_mb,
            jvm_args: vec![],
            java_path: None,
            cover: None,
            created_at: time::now_iso(),
        };
        match instance::create(&game, &meta) {
            Ok(inst) => {
                emit_instance_changed(&inst.id, "created");
                task::finish(
                    &tid,
                    serde_json::json!({ "kind": "create", "instance": inst }),
                );
            }
            Err(e) => task::fail(&tid, e),
        }
    });
    Ok(t)
}

#[tauri::command]
fn delete_instance(id: String, opts: Option<DeleteOpts>) -> Result<Config, String> {
    let game = game_root();
    if instance::read_meta(&game, &id).is_none() {
        return error::fail(error::INSTANCE_NOT_FOUND, "实例不存在或不是托管实例");
    }
    if session::active(Some(&id)).is_some() {
        return error::fail(error::INSTANCE_BUSY, "实例正在运行，先退出游戏");
    }
    if task::has_running(&id) {
        return error::fail(error::INSTANCE_BUSY, "实例有进行中的任务");
    }
    instance::delete(&game, &id, opts.and_then(|o| o.trash).unwrap_or(true))?;
    emit_instance_changed(&id, "deleted");

    let mut cfg = core::load_config();
    if cfg.active_instance_id.as_deref() == Some(id.as_str()) {
        cfg.active_instance_id = None;
        core::save_config(&cfg)?;
    }
    Ok(cfg)
}

// ── 回收站 ──────────────────────────────────────────────────────

/// 列出回收站内容（只读，不删除任何东西）
#[tauri::command]
fn list_trash() -> Vec<recycle::TrashEntry> {
    recycle::list(&game_root())
}

/// 从回收站恢复一个实例；id 冲突时自动换新 id，不覆盖现有实例
#[tauri::command]
fn restore_from_trash(trash_name: String) -> Result<recycle::RestoreResult, String> {
    let game = game_root();
    let r = recycle::restore(&game, &trash_name)?;
    emit_instance_changed(&r.instance_id, "restored");
    Ok(r)
}

/// 永久删除回收站里的指定项（危险操作，必须显式列出名称并确认）
#[tauri::command]
fn purge_trash(trash_names: Vec<String>, confirm: bool) -> Result<recycle::PurgeResult, String> {
    if !confirm {
        return error::fail(error::UNSUPPORTED, "永久删除需要明确确认");
    }
    if trash_names.is_empty() {
        return error::fail(error::INVALID_VALUE, "没有指定要永久删除的项目");
    }
    recycle::purge(&game_root(), &trash_names)
}

/// 清空整个回收站（独立危险操作，必须二次确认）
#[tauri::command]
fn purge_all_trash(confirm: bool) -> Result<recycle::PurgeResult, String> {
    if !confirm {
        return error::fail(error::UNSUPPORTED, "清空回收站需要明确确认");
    }
    recycle::purge_all(&game_root())
}

// ── 任务 ────────────────────────────────────────────────────────

#[tauri::command]
fn get_tasks(
    state: Option<String>,
    kind: Option<String>,
    instance_id: Option<String>,
) -> Vec<Task> {
    task::snapshot(state.as_deref(), kind.as_deref(), instance_id.as_deref())
}

#[tauri::command]
fn get_task(task_id: String) -> Option<Task> {
    task::get(&task_id)
}

#[tauri::command]
fn cancel_task(task_id: String) -> serde_json::Value {
    let accepted = task::cancel_task(&task_id);
    serde_json::json!({ "accepted": accepted })
}

#[tauri::command]
async fn start_install(app: AppHandle, game_version: String) -> Result<Task, String> {
    if task::has_running_kind("install", None, Some(&game_version)) {
        return Err("BUSY|同类任务正在运行，请等待或取消".into());
    }
    let t = task::create(
        "install",
        None,
        Some(game_version.clone()),
        format!("安装 {game_version}"),
    );
    let tid = t.task_id.clone();
    task::set_retry_spec(
        &t.task_id,
        serde_json::json!({ "kind": "install", "gameVersion": game_version }),
    );
    let gv = game_version.clone();
    let _ = app;
    std::thread::spawn(move || run_install(&tid, gv));
    Ok(t)
}

/// 安装游戏本体的 worker：命令与「重试」共用同一份逻辑
fn run_install(tid: &str, game_version: String) {
    task::start(tid);
    let cfg = core::load_config();
    let dir = PathBuf::from(&cfg.game_dir);
    let t2 = tid.to_string();
    let v = game_version.clone();
    // 取消判断交给批量下载器，取消后不会继续往磁盘写
    let tc = tid.to_string();
    let cancel = net::cancel_from(move || task::is_cancelled(&tc));
    // 后台线程 panic 必须变成任务失败，不能让任务永远 running
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        core::install_with_cancel(&game_version, &dir, &cfg.username, Some(cancel), move |p| {
            task::progress(&t2, &p.stage, p.label, p.done, p.total);
        })
    }))
    .unwrap_or_else(|_| Err("INTERNAL|后台任务异常|安装线程崩溃".to_string()));
    match res {
        // 活儿已经干完才收到取消：如实报告完成，不假装已取消
        Ok(_) if task::is_cancelled(tid) => task::finish(
            tid,
            serde_json::json!({ "kind": "install", "gameVersion": v, "cancelledLate": true }),
        ),
        Ok(_) => task::finish(
            tid,
            serde_json::json!({ "kind": "install", "gameVersion": v }),
        ),
        Err(_) if task::is_cancelled(tid) => task::cancelled(tid),
        Err(e) => {
            task::fail(tid, e);
        }
    }
}

#[tauri::command]
async fn start_launch(app: AppHandle, instance_id: String) -> Result<Task, String> {
    let game = game_root();
    let inst = instance::find(&game, &instance_id, &play_stats)
        .ok_or_else(|| "INSTANCE_NOT_FOUND|实例不存在".to_string())?;
    // 启动前再校验一次用户名（配置可能是旧版本写的）
    let cfg_now = core::load_config();
    account::validate_username(&cfg_now.username)?;
    if session::active(Some(&instance_id)).is_some() {
        return error::fail(error::INSTANCE_BUSY, "该实例正在运行");
    }
    if task::has_running(&instance_id) {
        return error::fail(error::BUSY, "该实例有进行中的任务");
    }

    let t = task::create(
        "launch",
        Some(instance_id.clone()),
        Some(inst.game_version.clone()),
        format!("启动 {}", inst.name),
    );
    let tid = t.task_id.clone();
    let app2 = app.clone();

    std::thread::spawn(move || {
        task::start(&tid);
        let cfg = core::load_config();
        let main_dir = PathBuf::from(&cfg.game_dir);
        let mc_dir = if inst.managed {
            Some(PathBuf::from(&inst.dir))
        } else {
            None
        };
        let t2 = tid.clone();
        let version = inst.game_version.clone();
        // Forge/NeoForge：启动 installer 生成的版本，不再走 fabric 装配
        let is_installer_loader = matches!(
            inst.loader.as_ref().map(|l| l.kind.as_str()),
            Some("forge") | Some("neoforge")
        );
        let version = if is_installer_loader {
            inst.launch_version.clone().unwrap_or(version)
        } else {
            version
        };
        let loader = if is_installer_loader {
            None
        } else {
            inst.loader
                .as_ref()
                .map(|l| (l.kind.clone(), l.version.clone()))
        };
        // 没指定 Java 时按版本要求自动挑一个（加载器版本会沿继承链找 javaVersion）
        let auto_java: Option<std::path::PathBuf> = match inst.java_path.as_deref() {
            Some(p) => Some(std::path::PathBuf::from(p)),
            None => core::auto_java_for(&main_dir, &version),
        };
        let spawned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            task::progress(&tid, "validate", "检查实例与版本文件", 0, 0);
            core::spawn_with(
                &version,
                loader.as_ref().map(|(k, v)| (k.as_str(), v.as_str())),
                &main_dir,
                mc_dir.as_deref(),
                &cfg.username,
                inst.memory_mb,
                &inst.jvm_args,
                auto_java.as_deref(),
                move |p| task::progress(&t2, &p.stage, p.label, p.done, p.total),
            )
        }))
        .unwrap_or_else(|_| Err("INTERNAL|后台任务异常|启动线程崩溃".to_string()));

        match spawned {
            Ok(mut child) => {
                let pid = child.id();
                task::progress(&tid, "spawned", "进程已创建", 1, 1);
                let sess = session::begin(&instance_id, &tid, pid);
                task::finish(
                    &tid,
                    serde_json::json!({
                        "kind": "launch",
                        "sessionId": sess.session_id,
                        "pid": pid,
                        "startedAt": sess.started_at,
                    }),
                );
                let sid = sess.session_id.clone();
                std::thread::spawn(move || {
                    let code = child.wait().ok().and_then(|s| s.code());
                    if let Some(s) = session::mark_exited(&sid, code) {
                        session::emit_exit(&app2, &s);
                    }
                });
            }
            Err(e) => task::fail(&tid, e),
        }
    });
    Ok(t)
}

// ── 会话 ────────────────────────────────────────────────────────

#[tauri::command]
fn get_active_session(instance_id: Option<String>) -> Option<GameSession> {
    session::active(instance_id.as_deref())
}

#[tauri::command]
fn list_sessions(instance_id: Option<String>, limit: Option<usize>) -> Vec<GameSession> {
    session::list(instance_id.as_deref(), limit.unwrap_or(50))
}

// ── 存档 / 统计 ─────────────────────────────────────────────────

#[tauri::command]
fn list_saves(instance_id: Option<String>) -> Result<Vec<stats::SaveEntry>, String> {
    let game = game_root();
    // 指定了实例却找不到 → 明确报错，绝不回落到全局目录（契约 v3 §5.9）
    let inst = match instance_id {
        Some(id) => Some(
            instance::find(&game, &id, &play_stats)
                .ok_or_else(|| "INSTANCE_NOT_FOUND|实例不存在".to_string())?,
        ),
        None => None,
    };
    Ok(stats::list_saves(&game, inst.as_ref()))
}

/// 存档目标：给数据包安装用的稳定 `worldId`（不是目录名）
#[tauri::command]
fn list_target_worlds(instance_id: String) -> Result<Vec<worlds::WorldTarget>, String> {
    let game = game_root();
    let inst = instance::find(&game, &instance_id, &play_stats)
        .ok_or_else(|| "INSTANCE_NOT_FOUND|实例不存在".to_string())?;
    Ok(worlds::list(std::path::Path::new(&inst.dirs.saves)))
}

/// 单个存档的详细信息（只读）。列表只给概要，点开才读这些。
#[tauri::command]
fn get_world_info(instance_id: String, world_id: String) -> Result<worlds::WorldDetail, String> {
    let game = game_root();
    let inst = instance::find(&game, &instance_id, &play_stats)
        .ok_or_else(|| "INSTANCE_NOT_FOUND|实例不存在".to_string())?;
    worlds::detail(std::path::Path::new(&inst.dirs.saves), &world_id)
}

// ── 存档备份与恢复 ──────────────────────────────────────────────

/// 列出某个实例的存档备份（只读）
#[tauri::command]
fn list_world_backups(instance_id: String) -> Result<Vec<world_backup::WorldBackup>, String> {
    let game = game_root();
    let inst = instance::find(&game, &instance_id, &play_stats)
        .ok_or_else(|| "INSTANCE_NOT_FOUND|实例不存在".to_string())?;
    Ok(world_backup::list(&instance::instance_dir(&game, &inst.id)))
}

/// 备份一个存档：游戏运行中直接拒绝，不做事后无法自证一致的热备份
#[tauri::command]
fn backup_world(
    instance_id: String,
    world_id: String,
    note: Option<String>,
) -> Result<world_backup::WorldBackup, String> {
    let game = game_root();
    let inst = instance::find(&game, &instance_id, &play_stats)
        .ok_or_else(|| "INSTANCE_NOT_FOUND|实例不存在".to_string())?;
    let running = session::active(Some(&inst.id)).is_some();
    world_backup::create(
        &instance::instance_dir(&game, &inst.id),
        std::path::Path::new(&inst.dirs.saves),
        &world_id,
        note.as_deref().unwrap_or(""),
        running,
        &*net::never_cancel(),
        |_, _| {},
    )
}

/// 恢复前预览（只读，不修改任何文件）
#[tauri::command]
fn preview_world_restore(
    instance_id: String,
    backup_id: String,
) -> Result<world_backup::RestorePreview, String> {
    let game = game_root();
    let inst = instance::find(&game, &instance_id, &play_stats)
        .ok_or_else(|| "INSTANCE_NOT_FOUND|实例不存在".to_string())?;
    world_backup::preview_restore(
        &instance::instance_dir(&game, &inst.id),
        std::path::Path::new(&inst.dirs.saves),
        &backup_id,
    )
}

/// 恢复存档：目标存在时必须显式 overwrite，覆盖前会先留一份副本
#[tauri::command]
fn restore_world(
    instance_id: String,
    backup_id: String,
    overwrite: Option<bool>,
) -> Result<world_backup::RestoreResult, String> {
    let game = game_root();
    let inst = instance::find(&game, &instance_id, &play_stats)
        .ok_or_else(|| "INSTANCE_NOT_FOUND|实例不存在".to_string())?;
    if session::active(Some(&inst.id)).is_some() {
        return error::fail(error::INSTANCE_BUSY, "游戏正在运行，先退出游戏再恢复存档");
    }
    world_backup::restore(
        &instance::instance_dir(&game, &inst.id),
        std::path::Path::new(&inst.dirs.saves),
        &backup_id,
        overwrite.unwrap_or(false),
        &*net::never_cancel(),
        |_, _| {},
    )
}

/// 删除一个存档备份（危险操作，必须确认）
#[tauri::command]
fn delete_world_backup(
    instance_id: String,
    backup_id: String,
    confirm: bool,
) -> Result<u64, String> {
    let game = game_root();
    let inst = instance::find(&game, &instance_id, &play_stats)
        .ok_or_else(|| "INSTANCE_NOT_FOUND|实例不存在".to_string())?;
    world_backup::delete(
        &instance::instance_dir(&game, &inst.id),
        &backup_id,
        confirm,
    )
}

// ── 版本复制 / 导入 / 迁移 ──────────────────────────────────────

/// 列出 `versions/` 下可以导入的本地版本（只读）
#[tauri::command]
fn scan_local_versions() -> Vec<serde_json::Value> {
    migrate::scan_importable(&game_root())
}

/// 计算复制计划（只读，不创建任何目录）
#[tauri::command]
fn plan_instance_copy(
    instance_id: String,
    name: String,
    options: Option<migrate::CopyOptions>,
) -> Result<migrate::CopyPlan, String> {
    let game = game_root();
    migrate::plan_copy(
        &game,
        &instance_id,
        &name,
        &options.unwrap_or_else(migrate::CopyOptions::settings_only),
        &play_stats,
    )
}

/// 计算导入计划（只读；导入不会移动或删除 versions 里的原文件）
#[tauri::command]
fn plan_version_import(version: String, name: Option<String>) -> Result<migrate::CopyPlan, String> {
    let game = game_root();
    migrate::plan_import(&game, &version, name.as_deref())
}

/// 执行复制/导入计划（两段式：必须先预览拿到 planId）
#[tauri::command]
fn apply_instance_copy(plan_id: String) -> Result<migrate::CopyResult, String> {
    let game = game_root();
    let r = migrate::execute(&game, &plan_id, &*net::never_cancel(), |_, _| {})?;
    emit_instance_changed(&r.instance_id, "created");
    Ok(r)
}

#[tauri::command]
fn get_stats(range: Option<String>) -> stats::Stats {
    let game = game_root();
    let names: std::collections::HashMap<String, String> = instance::discover(&game, &play_stats)
        .into_iter()
        .map(|i| (i.id, i.name))
        .collect();
    stats::stats(range.as_deref().unwrap_or("7d"), &|id| {
        names.get(id).cloned().unwrap_or_else(|| id.to_string())
    })
}

// ── 资源（Modrinth） ────────────────────────────────────────────

#[tauri::command]
async fn get_resource_project(project_id: String) -> Result<resources::ResourceProject, String> {
    tauri::async_runtime::spawn_blocking(move || resources::project(&project_id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn list_resource_files(
    project_id: String,
    game_version: Option<String>,
    loader: Option<String>,
) -> Result<Vec<resources::ResourceFile>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        resources::files(&project_id, game_version.as_deref(), loader.as_deref())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn search_resources(
    query: String,
    kind: Option<String>,
    game_version: Option<String>,
    loader: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
    sort: Option<String>,
) -> Result<search::SearchResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        search::search(
            &query,
            kind.as_deref(),
            game_version.as_deref(),
            loader.as_deref(),
            limit,
            offset,
            sort.as_deref(),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn prepare_resource_install(
    request: resources::ResourceInstallRequest,
) -> Result<resources::ResourcePlan, String> {
    let game = game_root();
    tauri::async_runtime::spawn_blocking(move || resources::prepare(&game, request, &play_stats))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn start_resource_install(plan_id: String, revision: u32) -> Result<Task, String> {
    let stored = resources::take(&plan_id, revision)?;
    if let Some(first) = stored.plan.issues.iter().find(|i| i.blocking) {
        return Err(format!("{}|{}", first.code, first.message));
    }
    let title = format!("安装 {}", stored.plan.file_name);
    let t = task::create(
        "resource-install",
        Some(stored.plan.instance_id.clone()),
        None,
        title,
    );
    let tid = t.task_id.clone();
    std::thread::spawn(move || {
        task::start(&tid);
        let t2 = tid.clone();
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let cancel = net::cancel_from({
                let tid_c = tid.clone();
                move || task::is_cancelled(&tid_c)
            });
            resources::execute(&stored, &*cancel, move |c, total| {
                task::progress(&t2, "download", format!("下载 {c}/{total}"), c, total);
            })
        }))
        .unwrap_or_else(|_| Err("INTERNAL|后台任务异常|资源安装线程崩溃".to_string()));

        match res {
            // 已经装完才收到取消：如实报告完成，不假装已取消
            Ok(path) if task::is_cancelled(&tid) => task::finish(
                &tid,
                serde_json::json!({
                    "kind": "resource-install",
                    "path": path.to_string_lossy(),
                    "file": stored.plan.file_name,
                    "cancelledLate": true,
                }),
            ),
            Ok(path) => task::finish(
                &tid,
                serde_json::json!({
                    "kind": "resource-install",
                    "path": path.to_string_lossy(),
                    "file": stored.plan.file_name,
                }),
            ),
            Err(e) if task::is_cancelled(&tid) || e.starts_with("CANCELLED") => {
                task::cancelled(&tid)
            }
            Err(e) => task::fail(&tid, e),
        }
    });
    Ok(t)
}

// ── 整合包导出 ──────────────────────────────────────────────────

#[tauri::command]
async fn export_modpack(request: export::ExportRequest) -> Result<export::ExportResult, String> {
    let game = game_root();
    tauri::async_runtime::spawn_blocking(move || export::export(&game, &request, &play_stats))
        .await
        .map_err(|e| e.to_string())?
}

/// 同一个整合包在本地装过哪些版本（只读，用于安装前的"已装版本"提示）
#[tauri::command]
async fn modpack_installed_versions(project_id: String) -> Result<Vec<serde_json::Value>, String> {
    let game = game_root();
    tauri::async_runtime::spawn_blocking(move || {
        Ok(modpack::installed_versions(&game, &project_id))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 导出回读校验：把导出的 .mrpack 重新解析一遍，确认 index 与 overrides 都能读
#[tauri::command]
async fn verify_exported_modpack(path: String) -> Result<serde_json::Value, String> {
    let game = game_root();
    tauri::async_runtime::spawn_blocking(move || export::verify_roundtrip(&game, &path))
        .await
        .map_err(|e| e.to_string())?
}

// ── 模组更新检查 ────────────────────────────────────────────────

#[tauri::command]
async fn check_mod_updates(
    instance_id: String,
    limit: Option<u32>,
) -> Result<updates::UpdateReport, String> {
    let game = game_root();
    tauri::async_runtime::spawn_blocking(move || {
        let inst = instance::find(&game, &instance_id, &play_stats)
            .ok_or_else(|| "INSTANCE_NOT_FOUND|实例不存在".to_string())?;
        let loader = inst.loader.as_ref().map(|l| l.kind.clone());
        Ok(updates::check(
            std::path::Path::new(&inst.dirs.mods),
            &inst.game_version,
            loader.as_deref(),
            limit,
        ))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 原子更新：先下新文件并校验，成功后才删旧文件
#[tauri::command]
async fn start_mod_update(
    instance_id: String,
    file_name: String,
    project_id: String,
    version_id: String,
) -> Result<Task, String> {
    if task::has_running_kind("mod-update", Some(&instance_id), None) {
        return Err("BUSY|同类任务正在运行，请等待或取消".into());
    }
    let t = task::create(
        "mod-update",
        Some(instance_id.clone()),
        None,
        format!("更新 {file_name}"),
    );
    let tid = t.task_id.clone();
    task::set_retry_spec(
        &t.task_id,
        serde_json::json!({
            "kind": "mod-update",
            "instanceId": instance_id,
            "fileName": file_name,
            "projectId": project_id,
            "versionId": version_id,
        }),
    );
    std::thread::spawn(move || {
        run_mod_update(&tid, &instance_id, &file_name, &project_id, &version_id)
    });
    Ok(t)
}

/// 模组更新的 worker：命令与「重试」共用
fn run_mod_update(
    tid: &str,
    instance_id: &str,
    file_name: &str,
    project_id: &str,
    version_id: &str,
) {
    task::start(tid);
    let game = game_root();
    let t2 = tid.to_string();
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let cancel = net::cancel_from({
            let tc = tid.to_string();
            move || task::is_cancelled(&tc)
        });
        updates::apply_update(
            &game,
            instance_id,
            file_name,
            project_id,
            version_id,
            &*cancel,
            move |c, t, _s, _ts| {
                task::progress(&t2, "download", format!("下载 {c}/{t}"), c, t);
            },
        )
    }))
    .unwrap_or_else(|_| Err("INTERNAL|后台任务异常|更新线程崩溃".to_string()));

    match res {
        Ok((new_file, removed)) => task::finish(
            tid,
            serde_json::json!({ "kind": "mod-update", "file": new_file, "removed": removed }),
        ),
        Err(_) if task::is_cancelled(tid) => task::cancelled(tid),
        Err(e) => task::fail(tid, e),
    }
}

// ── 任务重试 ────────────────────────────────────────────────────

/// 校验任务能否重试，返回 (kind, 发起参数)
fn retry_spec_of(task_id: &str) -> Result<(String, serde_json::Value), String> {
    let old = task::get(task_id).ok_or_else(|| "INVALID_VALUE|任务不存在".to_string())?;
    if !matches!(old.state.as_str(), "error" | "cancelled") {
        return Err("INVALID_VALUE|只有失败或已取消的任务可以重试".into());
    }
    let spec = old
        .retry_spec
        .ok_or_else(|| "UNSUPPORTED|该任务没有记录发起参数，无法重试".to_string())?;
    let kind = spec
        .get("kind")
        .and_then(|k| k.as_str())
        .unwrap_or("")
        .to_string();
    if kind.is_empty() {
        return Err("UNSUPPORTED|任务缺少类型信息".into());
    }
    Ok((kind, spec))
}

#[tauri::command]
fn retry_task(task_id: String) -> Result<Task, String> {
    let (kind, spec) = retry_spec_of(&task_id)?;
    let get = |k: &str| {
        spec.get(k)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| format!("UNSUPPORTED|缺少参数 {k}"))
    };

    match kind.as_str() {
        "install" => {
            let gv = get("gameVersion")?;
            if task::has_running_kind("install", None, Some(&gv)) {
                return Err("BUSY|该版本已在安装中".into());
            }
            let t = task::create("install", None, Some(gv.clone()), format!("重试安装 {gv}"));
            task::set_retry_spec(&t.task_id, spec.clone());
            task::update(&t.task_id, |x| x.retry_of = Some(task_id.clone()));
            let tid = t.task_id.clone();
            std::thread::spawn(move || run_install(&tid, gv));
            Ok(t)
        }
        "mod-update" => {
            let inst = get("instanceId")?;
            let file = get("fileName")?;
            let pid = get("projectId")?;
            let vid = get("versionId")?;
            if task::has_running_kind("mod-update", Some(&inst), None) {
                return Err("BUSY|该实例已有更新任务在运行".into());
            }
            let t = task::create(
                "mod-update",
                Some(inst.clone()),
                None,
                format!("重试更新 {file}"),
            );
            task::set_retry_spec(&t.task_id, spec.clone());
            task::update(&t.task_id, |x| x.retry_of = Some(task_id.clone()));
            let tid = t.task_id.clone();
            std::thread::spawn(move || run_mod_update(&tid, &inst, &file, &pid, &vid));
            Ok(t)
        }
        _ => Err("UNSUPPORTED|该任务需要重新规划，请从界面重新发起".into()),
    }
}

/// 本项目下载过的 JDK（只读列表，删除需用户单独确认）
#[tauri::command]
fn list_managed_jdks() -> Vec<jdk::ManagedJdk> {
    jdk::list_managed(&game_root())
}

/// 列出某个光影组件的全部兼容发行版本（供玩家自选）
#[tauri::command]
fn list_component_versions(
    id: String,
    game_version: String,
    loader: Option<String>,
) -> Result<Vec<components::ComponentVersion>, String> {
    components::versions(&id, &game_version, loader.as_deref())
}
// ── JDK 下载 ────────────────────────────────────────────────────

#[tauri::command]
async fn install_java(major: u32) -> Result<Task, String> {
    if task::has_running_kind("jdk", None, None) {
        return Err("BUSY|已有 Java 下载任务在运行".into());
    }
    let t = task::create("jdk", None, None, format!("下载 Java {major}"));
    task::set_retry_spec(
        &t.task_id,
        serde_json::json!({ "kind": "install-java", "major": major }),
    );
    let tid = t.task_id.clone();
    std::thread::spawn(move || {
        task::start(&tid);
        let game = game_root();
        let t2 = tid.clone();
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let tc = tid.clone();
            jdk::install(
                major,
                &game,
                move || task::is_cancelled(&tc),
                move |stage, done, total| {
                    task::progress(
                        &t2,
                        stage,
                        format!("Java {major} {done}/{total}"),
                        done,
                        total,
                    );
                },
            )
        }))
        .unwrap_or_else(|_| Err("INTERNAL|后台任务异常|Java 下载线程崩溃".to_string()));

        match res {
            Ok(r) => task::finish(
                &tid,
                serde_json::json!({
                    "kind": "install-java",
                    "major": r.major,
                    "version": r.version,
                    "path": r.path,
                }),
            ),
            Err(_) if task::is_cancelled(&tid) => task::cancelled(&tid),
            Err(e) => task::fail(&tid, e),
        }
    });
    Ok(t)
}

// ── 账号 ────────────────────────────────────────────────────────

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct MsaStatus {
    configured: bool,
    client_id_masked: Option<String>,
    reason: Option<String>,
    steps: Vec<String>,
}

#[tauri::command]
fn msa_status() -> MsaStatus {
    let cfg = core::load_config();
    match cfg
        .msa_client_id
        .as_deref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        Some(id) => MsaStatus {
            configured: true,
            client_id_masked: Some(format!(
                "{}…{}",
                &id[..8.min(id.len())],
                &id[id.len().saturating_sub(4)..]
            )),
            reason: None,
            steps: vec![],
        },
        None => MsaStatus {
            configured: false,
            client_id_masked: None,
            reason: Some("未配置 Azure 应用 ID，微软登录不可用".into()),
            steps: vec![
                "在 Azure Portal 注册一个应用（个人账号即可，选择「移动和桌面应用」）".into(),
                "在「身份验证」里启用「允许公共客户端流」，不需要回调地址".into(),
                "把应用（客户端）ID 填进配置项 msaClientId".into(),
                "本启动器不内置任何第三方客户端 ID，必须用你自己的应用".into(),
            ],
        },
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountView {
    kind: String,
    name: String,
    online: bool,
    note: Option<String>,
}

#[tauri::command]
fn get_account() -> AccountView {
    AccountView {
        kind: "offline".into(),
        name: core::load_config().username,
        online: false,
        note: Some("离线档案：联机时由服务器分配身份".into()),
    }
}

// ── 光影支持组件 ────────────────────────────────────────────────

#[tauri::command]
async fn get_component_catalog(
    game_version: String,
    loader: Option<String>,
) -> Result<components::ComponentCatalog, String> {
    tauri::async_runtime::spawn_blocking(move || {
        components::catalog(&game_version, loader.as_deref())
    })
    .await
    .map_err(|e| e.to_string())
}

// ── Java ────────────────────────────────────────────────────────

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct JavaReport {
    runtimes: Vec<java::JavaRuntime>,
    /// 当前选中实例需要的 Java 主版本（读不到为 null）
    required_major: Option<u32>,
    /// 建议使用的运行时（路径）
    recommended: Option<String>,
}

#[tauri::command]
async fn list_java_runtimes(instance_id: Option<String>) -> Result<JavaReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let game = game_root();
        let runtimes = java::scan(&game);
        let required = instance_id
            .as_deref()
            .and_then(|id| instance::find(&game, id, &play_stats))
            .and_then(|i| java::required_major(&game, &i.game_version));
        let recommended = required
            .and_then(|m| java::pick_for(&runtimes, m))
            .map(|r| r.path);
        JavaReport {
            runtimes,
            required_major: required,
            recommended,
        }
    })
    .await
    .map_err(|e| e.to_string())
}

// ── 元数据缓存（D1）────────────────────────────────────────────

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct VersionListReport {
    versions: Vec<core::VersionSummary>,
    /// 数据来源：network / cache，含抓取时间与是否过期
    cache: serde_json::Value,
}

/// 版本清单（断网时用上次成功的缓存，并说明数据时间）
#[tauri::command]
async fn list_versions_cached() -> Result<VersionListReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let cfg = core::load_config();
        let (versions, cache) = core::list_versions_cached(&cfg.source)?;
        Ok(VersionListReport { versions, cache })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 元数据缓存占用
#[tauri::command]
fn meta_cache_stats() -> serde_json::Value {
    net::meta_stats()
}

/// 清理元数据缓存（超过 keep_days 天，默认 30）
#[tauri::command]
fn prune_meta_cache(keep_days: Option<u64>) -> (u64, u64) {
    net::meta_prune(keep_days.unwrap_or(30))
}

/// 清空元数据缓存
#[tauri::command]
fn clear_meta_cache(confirm: bool) -> Result<u64, String> {
    net::meta_clear(confirm)
}
// ── Java 审计（C4）──────────────────────────────────────────────

/// 每个实例的 Java 情况 + 托管 JDK 占用 + 没人用的 JDK（只读）
#[tauri::command]
async fn java_audit() -> Result<java_audit::JavaAudit, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let game = game_root();
        Ok(java_audit::audit(&game, &play_stats))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 单独检查一个实例的 Java
#[tauri::command]
async fn check_instance_java(instance_id: String) -> Result<java_audit::JavaCheck, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let game = game_root();
        java_audit::check_instance(&game, &instance_id, &play_stats)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 删除启动器自己下载的 JDK（必须显式确认，系统 Java 一律拒绝）
#[tauri::command]
async fn remove_managed_java(major: u32, confirm: bool) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let game = game_root();
        java_audit::remove_managed(&game, major, confirm)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// JDK 下载缓存占用
#[tauri::command]
fn java_cache_stats() -> serde_json::Value {
    java_audit::cache_stats(&game_root())
}

/// 清空 JDK 下载缓存（只删 zip，不动已安装的 JDK）
#[tauri::command]
fn clear_java_cache(confirm: bool) -> Result<serde_json::Value, String> {
    java_audit::clear_cache(&game_root(), confirm)
}

// ── 整合包 ──────────────────────────────────────────────────────

#[tauri::command]
async fn prepare_modpack_install(
    request: modpack::ModpackRequest,
) -> Result<modpack::ModpackPlan, String> {
    let game = game_root();
    tauri::async_runtime::spawn_blocking(move || modpack::prepare(&game, request))
        .await
        .map_err(|e| e.to_string())?
}

/// 导入本地 .mrpack：只读原文件，复制进受管缓存后按普通流程安装
#[tauri::command]
async fn import_local_modpack(
    path: String,
    name: Option<String>,
) -> Result<modpack::ModpackPlan, String> {
    let game = game_root();
    tauri::async_runtime::spawn_blocking(move || modpack::prepare_local(&game, &path, name))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn start_modpack_install(
    plan_id: String,
    revision: u32,
    include_optional: Option<bool>,
) -> Result<Task, String> {
    let include_optional = include_optional.unwrap_or(false);
    let stored = modpack::take(&plan_id, revision)?;
    if let Some(first) = stored.plan.issues.iter().find(|i| i.blocking) {
        return Err(format!("{}|{}", first.code, first.message));
    }
    if task::has_running_kind("modpack", None, Some(&stored.plan.info.mc_version)) {
        return Err("BUSY|同类任务正在运行，请等待或取消".into());
    }
    let t = task::create(
        "modpack",
        None,
        Some(stored.plan.info.mc_version.clone()),
        format!("安装整合包 {}", stored.plan.info.name),
    );
    let tid = t.task_id.clone();
    std::thread::spawn(move || {
        task::start(&tid);
        let game = game_root();
        let cfg = core::load_config();
        let mut created: Option<String> = None;

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || -> Result<Instance, String> {
                let index = modpack::read_index(&stored.pack_path)?;
                let meta = instance::InstanceMeta {
                    id: instance::new_id(),
                    name: stored.plan.info.name.clone(),
                    game_version: index.mc_version.clone(),
                    loader_kind: index.loader_kind.clone(),
                    loader_version: index.loader_version.clone(),
                    launch_version: None,
                    memory_mb: None,
                    jvm_args: vec![],
                    java_path: None,
                    cover: None,
                    created_at: time::now_iso(),
                };
                let inst = instance::create(&game, &meta)?;
                created = Some(inst.id.clone());
                // 整合包安装期间也要保护这个实例
                task::bind_instance(&tid, &inst.id);

                if !core::version_installed(&game, &index.mc_version) {
                    let t2 = tid.clone();
                    let tc = tid.clone();
                    let cancel = net::cancel_from(move || task::is_cancelled(&tc));
                    core::install_with_cancel(
                        &index.mc_version,
                        &game,
                        &cfg.username,
                        Some(cancel),
                        move |p| {
                            task::progress(
                                &t2,
                                &p.stage,
                                format!("游戏本体 {}", p.label),
                                p.done,
                                p.total,
                            );
                        },
                    )?;
                }
                task::check_cancel(&tid)?;

                if let (Some(kind), Some(ver)) = (&index.loader_kind, &index.loader_version) {
                    let t2 = tid.clone();
                    if kind == "forge" || kind == "neoforge" {
                        let generated = core::install_forge_like(
                            kind,
                            &index.mc_version,
                            ver,
                            &game,
                            move |p| task::progress(&t2, &p.stage, p.label, p.done, p.total),
                        )?;
                        let mut m = instance::read_meta(&game, &inst.id)
                            .ok_or_else(|| "INSTANCE_NOT_FOUND|元数据丢失".to_string())?;
                        m.launch_version = Some(generated);
                        instance::save_meta(&game, &m)?;
                    } else {
                        let tc = tid.clone();
                        let cancel = net::cancel_from(move || task::is_cancelled(&tc));
                        core::install_loader_with_cancel(
                            &index.mc_version,
                            kind,
                            ver,
                            &game,
                            &cfg.username,
                            Some(cancel),
                            move |p| task::progress(&t2, &p.stage, p.label, p.done, p.total),
                        )?;
                    }
                }
                task::check_cancel(&tid)?;

                let inst_dir = instance::instance_dir(&game, &inst.id);

                // 磁盘预检：不够就早点失败，别下到一半才发现
                let (need, avail) = modpack::disk_precheck(&index, &inst_dir);

                if let Some(a) = avail {
                    if a < need.saturating_add(64 * 1024 * 1024) {
                        return Err(format!(
                            "DISK_FULL|磁盘空间不足|需要约 {} MB，可用 {} MB",
                            need / 1024 / 1024,
                            a / 1024 / 1024
                        ));
                    }
                }

                let t2 = tid.clone();
                let tc = tid.clone();
                modpack::download_files(
                    &index,
                    &inst_dir,
                    include_optional,
                    &move || task::is_cancelled(&tc),
                    move |d, total| {
                        task::progress(
                            &t2,
                            "download",
                            format!("整合包文件 {d}/{total}"),
                            d,
                            total,
                        );
                    },
                )?;

                let t3 = tid.clone();
                let tc2 = tid.clone();
                modpack::extract_overrides(
                    &stored.pack_path,
                    &inst_dir,
                    &move || task::is_cancelled(&tc2),
                    move |d, _| {
                        task::progress(&t3, "extract", format!("解压覆盖文件 {d}"), d, 0);
                    },
                )?;

                let fresh = instance::find(&game, &inst.id, &play_stats)
                    .ok_or_else(|| "INTERNAL|安装后读不到实例".to_string())?;
                Ok(fresh)
            },
        ));

        match result {
            // 已经装完才收到取消：如实报告完成，不假装已取消
            Ok(Ok(inst)) if task::is_cancelled(&tid) => {
                emit_instance_changed(&inst.id, "created");
                task::finish(
                    &tid,
                    serde_json::json!({ "kind": "modpack", "instance": inst, "cancelledLate": true }),
                );
            }
            Ok(Ok(inst)) => {
                emit_instance_changed(&inst.id, "created");
                task::finish(
                    &tid,
                    serde_json::json!({ "kind": "modpack", "instance": inst }),
                );
            }
            Ok(Err(e)) if task::is_cancelled(&tid) || e.starts_with("CANCELLED") => {
                if let Some(id) = &created {
                    let _ = instance::delete(&game, id, false);
                }
                task::cancelled(&tid);
            }
            Ok(Err(e)) => {
                if let Some(id) = &created {
                    let _ = instance::delete(&game, id, false);
                }
                task::fail(&tid, e);
            }
            Err(_) => {
                if let Some(id) = &created {
                    let _ = instance::delete(&game, id, false);
                }
                task::fail(&tid, "INTERNAL|后台任务异常|整合包线程崩溃".into());
            }
        }
    });
    Ok(t)
}

// ── 日志与诊断 ──────────────────────────────────────────────────

fn instance_dir_of(instance_id: &str) -> Result<PathBuf, String> {
    let game = game_root();
    let inst = instance::find(&game, instance_id, &play_stats)
        .ok_or_else(|| "INSTANCE_NOT_FOUND|实例不存在".to_string())?;
    Ok(PathBuf::from(inst.dir))
}

#[tauri::command]
fn list_logs(instance_id: String) -> Result<Vec<logs::LogFile>, String> {
    let dir = instance_dir_of(&instance_id)?;
    Ok(logs::list(&dir))
}

#[tauri::command]
fn read_log(
    instance_id: String,
    log_id: String,
    limit: Option<u32>,
) -> Result<logs::LogText, String> {
    let dir = instance_dir_of(&instance_id)?;
    logs::read(&dir, &log_id, limit)
}

/// 实时日志跟随：从 `offset` 开始读新增的完整行。
///
/// 前端记住返回的 `nextOffset`，下次带上它再调；`reset: true` 时清屏重来。
/// 没传 `log_id` 时自动跟最新一份游戏日志（启动后直接就能看）。
#[tauri::command]
fn tail_log(
    instance_id: String,
    log_id: Option<String>,
    offset: Option<u64>,
    max_bytes: Option<u64>,
) -> Result<live::LogChunk, String> {
    let dir = instance_dir_of(&instance_id)?;
    let id = match log_id {
        Some(id) => id,
        None => live::active_game_log(&dir)
            .map(|l| l.id)
            .ok_or_else(|| "INVALID_PATH|这个实例还没有日志".to_string())?,
    };
    live::tail(&dir, &id, offset.unwrap_or(0), max_bytes)
}

/// 查询 Minecraft 服务器的在线状态（服务器列表协议）。
///
/// 地址可以写成 `example.com` 或 `example.com:25566`。
/// **联网失败不报错**：返回 `online=false` + `error`，前端直接显示离线即可；
/// 只有地址本身不合法才返回 `Err`。
#[tauri::command]
async fn ping_server(
    host: String,
    port: Option<u16>,
    timeout_ms: Option<u64>,
) -> Result<server::ServerStatus, String> {
    tauri::async_runtime::spawn_blocking(move || server::ping(&host, port, timeout_ms))
        .await
        .map_err(|e| format!("INTERNAL|查询任务异常|{e}"))?
}

/// 收藏的服务器列表。
#[tauri::command]
fn list_servers() -> Vec<servers::ServerEntry> {
    servers::list(&game_root())
}

/// 加一个收藏。`host` 可以是 `example.com:25566`，端口会自己拆出来。
#[tauri::command]
fn add_server(
    name: String,
    host: String,
    port: Option<u16>,
    note: Option<String>,
) -> Result<servers::ServerEntry, String> {
    servers::add(
        &game_root(),
        &name,
        &host,
        port,
        note.as_deref().unwrap_or(""),
    )
}

/// 改收藏。`None` 的字段保持不变。
#[tauri::command]
fn update_server(
    id: String,
    name: Option<String>,
    host: Option<String>,
    port: Option<u16>,
    note: Option<String>,
) -> Result<servers::ServerEntry, String> {
    servers::update(
        &game_root(),
        &id,
        name.as_deref(),
        host.as_deref(),
        port,
        note.as_deref(),
    )
}

/// 从收藏里删掉。**只删记录，不碰任何文件。**
#[tauri::command]
fn remove_server(id: String) -> Result<(), String> {
    servers::remove(&game_root(), &id)
}

/// 按给定顺序重排。
#[tauri::command]
fn reorder_servers(ids: Vec<String>) -> Result<Vec<servers::ServerEntry>, String> {
    servers::reorder(&game_root(), &ids)
}

/// 批量查状态。不传 `ids` 就查全部。
///
/// 单条查不到不会影响其它条，每条都有自己的 `online` 和 `error`。
#[tauri::command]
async fn refresh_servers(
    ids: Option<Vec<String>>,
) -> Result<Vec<(String, server::ServerStatus)>, String> {
    let game = game_root();
    tauri::async_runtime::spawn_blocking(move || servers::refresh(&game, ids))
        .await
        .map_err(|e| format!("INTERNAL|查询任务异常|{e}"))
}

/// 只生成脱敏预览；上传由用户在界面上确认后另行处理
#[tauri::command]
fn prepare_diagnosis(
    instance_id: String,
    log_ids: Vec<String>,
    max_lines: Option<u32>,
) -> Result<logs::DiagnosisPreview, String> {
    let dir = instance_dir_of(&instance_id)?;
    logs::prepare(&dir, &log_ids, max_lines.unwrap_or(300))
}

// ── 安装计划 ────────────────────────────────────────────────────

#[tauri::command]
async fn prepare_game_install(
    request: plan::GameInstallRequest,
) -> Result<plan::GameInstallPlan, String> {
    let game = game_root();
    tauri::async_runtime::spawn_blocking(move || plan::prepare(&game, request))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn start_game_install(plan_id: String, revision: u32) -> Result<Task, String> {
    let p = plan::take(&plan_id, revision)?;
    if let Some(first) = plan::has_blocking(&p) {
        return Err(format!("{}|{}", first.code, first.message));
    }

    if task::has_running_kind("game-install", None, Some(&p.request.game_version)) {
        return Err("BUSY|同类任务正在运行，请等待或取消".into());
    }
    let t = task::create(
        "game-install",
        None,
        Some(p.request.game_version.clone()),
        format!("安装 {}", p.request.name),
    );
    let tid = t.task_id.clone();

    // 后台执行，body 抽成 run_game_install 便于自检复用
    std::thread::spawn(move || {
        task::start(&tid);
        let (result, created) = run_game_install(&tid, &p);
        match result {
            Ok(inst) => {
                emit_instance_changed(&inst.id, "created");
                task::finish(
                    &tid,
                    serde_json::json!({ "kind": "game-install", "instance": inst }),
                );
            }
            Err(_) if task::is_cancelled(&tid) => {
                if let Some(id) = &created {
                    let _ = instance::delete(&game_root(), id, false);
                }
                task::cancelled(&tid);
            }
            Err(e) => {
                // 失败回滚：只删本次新建的实例目录
                if let Some(id) = &created {
                    let _ = instance::delete(&game_root(), id, false);
                }
                task::fail(&tid, e);
            }
        }
    });

    Ok(t)
}

/// 安装计划的执行体（命令与自检共用）
///
/// 返回 (结果, 本次新建的实例 id)，调用方据此决定回滚。
fn run_game_install(
    tid: &str,
    p: &plan::GameInstallPlan,
) -> (Result<Instance, String>, Option<String>) {
    let game = game_root();
    let cfg = core::load_config();
    let v = p.request.game_version.clone();
    let mut created: Option<String> = None;

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
        || -> Result<Instance, String> {
            if !core::version_installed(&game, &v) {
                let t2 = tid.to_string();
                let tc = tid.to_string();
                let cancel = net::cancel_from(move || task::is_cancelled(&tc));
                core::install_with_cancel(&v, &game, &cfg.username, Some(cancel), move |pr| {
                    task::progress(&t2, &pr.stage, pr.label, pr.done, pr.total);
                })?;
            }
            task::check_cancel(tid)?;

            let meta = instance::InstanceMeta {
                id: p.target.proposed_instance_id.clone(),
                name: p.request.name.trim().to_string(),
                game_version: v.clone(),
                loader_kind: p.request.loader.as_ref().map(|l| l.kind.clone()),
                loader_version: p.request.loader.as_ref().map(|l| l.version.clone()),
                launch_version: None,
                memory_mb: p.request.memory_mb,
                jvm_args: vec![],
                java_path: None,
                cover: None,
                created_at: time::now_iso(),
            };
            let inst = instance::create(&game, &meta)?;
            created = Some(inst.id.clone());
            // 实例已存在，立刻把任务绑上去，避免安装期间被删除/改名
            task::bind_instance(tid, &inst.id);
            task::check_cancel(tid)?;

            if let Some(l) = &p.request.loader {
                let t2 = tid.to_string();
                if l.kind == "forge" || l.kind == "neoforge" {
                    // installer 会生成 {mc}-forge-{ver} 之类的版本，记下来供启动使用
                    let generated =
                        core::install_forge_like(&l.kind, &v, &l.version, &game, move |pr| {
                            task::progress(&t2, &pr.stage, pr.label, pr.done, pr.total);
                        })?;
                    let mut meta = instance::read_meta(&game, &inst.id)
                        .ok_or_else(|| "INSTANCE_NOT_FOUND|实例元数据丢失".to_string())?;
                    meta.launch_version = Some(generated);
                    instance::save_meta(&game, &meta)?;
                } else {
                    let tc = tid.to_string();
                    let cancel = net::cancel_from(move || task::is_cancelled(&tc));
                    core::install_loader_with_cancel(
                        &v,
                        &l.kind,
                        &l.version,
                        &game,
                        &cfg.username,
                        Some(cancel),
                        move |pr| {
                            task::progress(&t2, &pr.stage, pr.label, pr.done, pr.total);
                        },
                    )?;
                }
            }
            task::check_cancel(tid)?;

            // 计划里挑选的光影支持组件：加载器装好之后再按实例真实 id 安装
            for comp in &p.components {
                task::check_cancel(tid)?;
                let req = resources::ResourceInstallRequest {
                    instance_id: inst.id.clone(),
                    kind: "mod".into(),
                    project_id: comp.project_id.clone(),
                    version_id: comp.version_id.clone(),
                    world_id: None,
                    overwrite: false,
                };
                let rp = resources::prepare(&game, req, &play_stats)?;
                if let Some(bad) = rp.issues.iter().find(|i| i.blocking) {
                    return Err(format!(
                        "INCOMPATIBLE|{} 无法安装|{}",
                        comp.label, bad.message
                    ));
                }
                let stored = resources::take(&rp.plan_id, rp.revision)?;
                let label = comp.label.clone();
                let t2 = tid.to_string();
                let t3 = tid.to_string();
                let cancel = net::cancel_from(move || task::is_cancelled(&t3));
                resources::execute(&stored, &*cancel, move |done, total| {
                    task::progress(
                        &t2,
                        "component",
                        format!("安装 {label} {done}/{total}"),
                        done,
                        total,
                    );
                })?;
            }
            task::check_cancel(tid)?;

            let fresh = instance::find(&game, &inst.id, &play_stats)
                .ok_or_else(|| "INTERNAL|安装后读不到实例".to_string())?;
            if !fresh.health.ok {
                return Err(format!(
                    "VERIFY_FAILED|安装完成但文件不完整|{}",
                    fresh.health.reasons.join("; ")
                ));
            }
            Ok(fresh)
        },
    ));

    match result {
        Ok(r) => (r, created),
        Err(_) => (
            Err("INTERNAL|后台任务异常|安装线程崩溃".to_string()),
            created,
        ),
    }
}

/// 修复实例：只补缺失的文件，不动已有内容
#[tauri::command]
async fn repair_instance(instance_id: String) -> Result<Task, String> {
    let inst = managed_instance(&instance_id)?;
    if session::active(Some(&instance_id)).is_some() {
        return Err("INSTANCE_BUSY|实例正在运行，先退出游戏".into());
    }
    if task::has_running_kind("repair", Some(&instance_id), None) {
        return Err("BUSY|该实例已有修复任务在运行".into());
    }
    let t = task::create(
        "repair",
        Some(instance_id.clone()),
        Some(inst.game_version.clone()),
        format!("修复 {}", inst.name),
    );
    let tid = t.task_id.clone();
    std::thread::spawn(move || {
        task::start(&tid);
        let game = game_root();
        let cfg = core::load_config();
        let meta = match instance::read_meta(&game, &instance_id) {
            Some(m) => m,
            None => {
                task::fail(&tid, "INSTANCE_NOT_FOUND|实例不存在".into());
                return;
            }
        };
        let steps = core::repair_steps(&game, &meta);
        let res =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(), String> {
                for step in &steps {
                    task::check_cancel(&tid)?;
                    match *step {
                        "base" => {
                            let t2 = tid.to_string();
                            let tc = tid.to_string();
                            let cancel = net::cancel_from(move || task::is_cancelled(&tc));
                            core::install_with_cancel(
                                &meta.game_version,
                                &game,
                                &cfg.username,
                                Some(cancel),
                                move |pr| {
                                    task::progress(&t2, &pr.stage, pr.label, pr.done, pr.total);
                                },
                            )?;
                        }
                        "loader" => {
                            if let (Some(kind), Some(ver)) =
                                (&meta.loader_kind, &meta.loader_version)
                            {
                                let t2 = tid.to_string();
                                if kind == "forge" || kind == "neoforge" {
                                    let generated = core::install_forge_like(
                                        kind,
                                        &meta.game_version,
                                        ver,
                                        &game,
                                        move |pr| {
                                            task::progress(
                                                &t2, &pr.stage, pr.label, pr.done, pr.total,
                                            );
                                        },
                                    )?;
                                    let mut m = meta.clone();
                                    m.launch_version = Some(generated);
                                    instance::save_meta(&game, &m)?;
                                } else {
                                    let tc = tid.to_string();
                                    let cancel = net::cancel_from(move || task::is_cancelled(&tc));
                                    core::install_loader_with_cancel(
                                        &meta.game_version,
                                        kind,
                                        ver,
                                        &game,
                                        &cfg.username,
                                        Some(cancel),
                                        move |pr| {
                                            task::progress(
                                                &t2, &pr.stage, pr.label, pr.done, pr.total,
                                            );
                                        },
                                    )?;
                                }
                            }
                        }
                        _ => {}
                    }
                }
                Ok(())
            }))
            .unwrap_or_else(|_| Err("INTERNAL|后台任务异常|修复线程崩溃".to_string()));

        match res {
            Ok(()) => {
                let version = instance::read_meta(&game, &instance_id)
                    .and_then(|m| m.launch_version)
                    .unwrap_or_else(|| meta.game_version.clone());
                let health = instance::health_for_test(&game, &version);
                if health.ok {
                    task::finish(
                        &tid,
                        serde_json::json!({
                            "kind": "repair",
                            "instanceId": instance_id,
                            "steps": steps,
                            "ok": true
                        }),
                    );
                } else {
                    task::fail(
                        &tid,
                        format!("VERIFY_FAILED|修复后仍不完整|{}", health.reasons.join("; ")),
                    );
                }
            }
            Err(_) if task::is_cancelled(&tid) => task::cancelled(&tid),
            Err(e) => task::fail(&tid, e),
        }
    });
    Ok(t)
}
/// 结构化日志诊断：返回命中的问题分类与脱敏原文（不上传任何内容）
#[tauri::command]
fn diagnose_instance(
    instance_id: String,
    log_id: Option<String>,
    max_lines: Option<u32>,
) -> Result<logs::DiagnosisReport, String> {
    let inst = managed_instance(&instance_id)?;
    logs::diagnose(
        std::path::Path::new(&inst.dir),
        log_id.as_deref(),
        max_lines.unwrap_or(5000).min(20000),
    )
}
// ── 加载器 ──────────────────────────────────────────────────────

#[tauri::command]
fn list_loaders(game_version: String) -> Result<Vec<loader::LoaderCatalog>, String> {
    loader::list(&game_version)
}

/// 某个游戏版本 + 加载器类型的**全部**可用构建（不只是推荐版本）
#[tauri::command]
fn list_loader_builds(
    game_version: String,
    kind: String,
) -> Result<Vec<loader::LoaderBuild>, String> {
    loader::builds(&game_version, &kind)
}

#[tauri::command]
async fn start_install_loader(
    instance_id: String,
    kind: String,
    version: String,
) -> Result<Task, String> {
    let inst = managed_instance(&instance_id)?;
    if task::has_running_kind("loader", Some(&instance_id), Some(&inst.game_version)) {
        return Err("BUSY|同类任务正在运行，请等待或取消".into());
    }
    // 先确认这个加载器版本确实存在且适配该游戏版本（避免跑到一半 404）
    let builds = loader::builds(&inst.game_version, &kind)?;
    if !builds.iter().any(|b| b.version == version) {
        let sample: Vec<String> = builds.iter().take(3).map(|b| b.version.clone()).collect();
        return Err(format!(
            "INCOMPATIBLE|{kind} {version} 没有适用于 Minecraft {} 的构建|可用示例：{}",
            inst.game_version,
            if sample.is_empty() {
                "无".to_string()
            } else {
                sample.join(", ")
            }
        ));
    }
    let t = task::create(
        "loader",
        Some(instance_id.clone()),
        Some(inst.game_version.clone()),
        format!("安装 {kind} {version}"),
    );
    let tid = t.task_id.clone();
    std::thread::spawn(move || {
        task::start(&tid);
        let cfg = core::load_config();
        let main_dir = PathBuf::from(&cfg.game_dir);
        let t2 = tid.clone();
        let tc = tid.clone();
        let cancel = net::cancel_from(move || task::is_cancelled(&tc));
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || -> Result<Option<String>, String> {
                if kind == "forge" || kind == "neoforge" {
                    // installer 产出的 version id 要记进实例，启动时用它
                    core::install_forge_like(
                        &kind,
                        &inst.game_version,
                        &version,
                        &main_dir,
                        move |p| {
                            task::progress(&t2, &p.stage, p.label, p.done, p.total);
                        },
                    )
                    .map(Some)
                } else {
                    core::install_loader_with_cancel(
                        &inst.game_version,
                        &kind,
                        &version,
                        &main_dir,
                        &cfg.username,
                        Some(cancel),
                        move |p| task::progress(&t2, &p.stage, p.label, p.done, p.total),
                    )
                    .map(|_| None)
                }
            },
        ))
        .unwrap_or_else(|_| Err("INTERNAL|后台任务异常|加载器线程崩溃".to_string()));
        let late = task::is_cancelled(&tid);
        match res {
            Err(_) if late => task::cancelled(&tid),
            Err(e) => task::fail(&tid, e),
            Ok(generated) => {
                let game = game_root();
                // 元数据写入失败必须让任务失败，否则前端以为装好了但实例仍记不到 loader
                let saved = match instance::read_meta(&game, &instance_id) {
                    Some(mut meta) => {
                        meta.loader_kind = Some(kind.clone());
                        meta.loader_version = Some(version.clone());
                        if generated.is_some() {
                            meta.launch_version = generated.clone();
                        }
                        instance::save_meta(&game, &meta)
                    }
                    None => Err("INSTANCE_NOT_FOUND|实例元数据丢失".to_string()),
                };
                match saved {
                    Ok(_) => {
                        emit_instance_changed(&instance_id, "updated");
                        task::finish(
                            &tid,
                            serde_json::json!({
                                "kind": "loader",
                                "loader": { "kind": kind, "version": version },
                                "cancelledLate": late,
                            }),
                        );
                    }
                    Err(e) => task::fail(&tid, e),
                }
            }
        }
    });
    Ok(t)
}

// ── 模组 ────────────────────────────────────────────────────────

/// 只对托管实例开放（兼容实例共享游戏目录，不能写）
fn managed_instance(id: &str) -> Result<Instance, String> {
    let game = game_root();
    let inst = instance::find(&game, id, &play_stats)
        .ok_or_else(|| "INSTANCE_NOT_FOUND|实例不存在".to_string())?;
    if !inst.managed {
        return Err("UNSUPPORTED|兼容实例不支持模组管理".into());
    }
    Ok(inst)
}

/// 共享下载缓存的占用情况（条数 / 字节 / 上限）。
#[tauri::command]
fn cache_stats() -> cache::CacheStats {
    cache::stats(cache::DEFAULT_BUDGET_BYTES)
}

/// 按最久未使用淘汰，把缓存压回上限之内。返回释放的字节数。
#[tauri::command]
fn cache_enforce_budget() -> u64 {
    cache::enforce_budget(cache::DEFAULT_BUDGET_BYTES)
}

/// 清空共享下载缓存。
///
/// 危险操作：必须由用户在前端点"确认清空"后调用，且 `confirm` 必须为 true。
/// 只删缓存目录，不会碰任何实例、存档、mods。
#[tauri::command]
fn cache_clear(confirm: bool) -> Result<u64, String> {
    if !confirm {
        return Err("UNSUPPORTED|清空缓存需要明确确认".into());
    }
    Ok(cache::clear())
}

/// 清理下载残留的 `.part` 文件（崩溃或取消留下的半截文件）。
///
/// 默认只清 7 天以前的；`days = 0` 表示全部清理，同样需要 `confirm`。
#[tauri::command]
fn clean_partials(days: Option<u64>, confirm: Option<bool>) -> Result<serde_json::Value, String> {
    let days = days.unwrap_or(7);
    if days == 0 && !confirm.unwrap_or(false) {
        return Err("UNSUPPORTED|清理全部下载残留需要明确确认".into());
    }
    let cfg = core::load_config();
    let game = PathBuf::from(&cfg.game_dir);
    if !game.exists() {
        return Ok(serde_json::json!({ "removed": 0, "freed": 0 }));
    }
    let (removed, freed) = cache::clean_stale_partials(&game, days);
    Ok(serde_json::json!({ "removed": removed, "freed": freed, "days": days }))
}

/// 各目录占用快照，给"存储管理"页面用。
#[tauri::command]
fn disk_usage() -> serde_json::Value {
    let cfg = core::load_config();
    cache::disk_usage(std::path::Path::new(&cfg.game_dir))
}

/// 下载调度器的实时状态：并发数、每个域名的占用、累计下载量。
///
/// 前端可以轮询它显示"正在同时下载 N 个文件"，不需要自己维护计数。
#[tauri::command]
fn net_scheduler_stats() -> serde_json::Value {
    net::scheduler_stats()
}

// ── B2 批量更新与回滚 ────────────────────────────────────────────

/// 只读：算出这批更新到底要做什么，能更新哪些、哪些被挡住、为什么。
#[tauri::command]
fn plan_mod_updates(
    instance_id: String,
    updates: Vec<updates::ModUpdate>,
) -> Result<update_plan::UpdatePlan, String> {
    let inst = managed_instance(&instance_id)?;
    let dir = PathBuf::from(&inst.dirs.mods);
    Ok(update_plan::plan(
        &dir,
        &instance_id,
        &inst.game_version,
        inst.loader.as_ref().map(|l| l.kind.as_str()),
        &updates,
    ))
}

/// 执行批量更新。失败会自动整批回滚。
#[tauri::command]
fn apply_mod_updates(
    app: AppHandle,
    instance_id: String,
    plan: update_plan::UpdatePlan,
) -> Result<update_plan::UpdateResult, String> {
    let cfg = core::load_config();
    let game = PathBuf::from(&cfg.game_dir);
    if task::has_running_kind("mod-update-batch", Some(&instance_id), None) {
        return Err("BUSY|该实例已有批量更新在进行".into());
    }
    let t = task::create(
        "mod-update-batch",
        Some(instance_id.clone()),
        None,
        format!("批量更新 {} 个模组", plan.items.len()),
    );
    let tid = t.task_id.clone();
    let cancel = net::cancel_from({
        let tc = tid.clone();
        move || task::is_cancelled(&tc)
    });
    std::thread::spawn(move || {
        task::start(&tid);
        let inst = instance_id.clone();
        let r = update_plan::execute(&game, &inst, &plan, &*cancel, |i, total, name| {
            task::progress(
                &tid,
                "download",
                format!("更新 {name}（{}/{total}）", i + 1),
                i,
                total,
            );
        });
        match r {
            Ok(res) if res.failed.is_empty() => {
                task::finish(
                    &tid,
                    serde_json::json!({
                        "kind": "mod-update-batch",
                        "batchId": res.batch_id,
                        "updated": res.updated,
                    }),
                );
            }
            Ok(res) => {
                // 有失败：已经回滚过了，如实报告
                let msg = res
                    .failed
                    .iter()
                    .map(|(n, e)| format!("{n}: {}", task::decode(e).message))
                    .collect::<Vec<_>>()
                    .join("；");
                let _ = app.emit(
                    "astral://mod-update",
                    serde_json::json!({ "instanceId": instance_id, "result": res }),
                );
                task::fail(&tid, format!("NETWORK|批量更新失败，已回滚|{msg}"));
            }
            Err(e) => task::fail(&tid, e),
        }
    });
    Ok(update_plan::UpdateResult {
        batch_id: t.task_id.clone(),
        updated: vec![],
        failed: vec![],
        rolled_back: false,
    })
}

/// 列出可回滚的更新批次
#[tauri::command]
fn mod_update_history(instance_id: String) -> Result<Vec<update_plan::BackupRecord>, String> {
    let inst = managed_instance(&instance_id)?;
    Ok(update_plan::history(std::path::Path::new(&inst.dirs.mods)))
}

/// 回滚指定批次
#[tauri::command]
fn rollback_mod_updates(instance_id: String, batch_id: String) -> Result<u32, String> {
    let inst = managed_instance(&instance_id)?;
    if task::has_running(&instance_id) {
        return Err("INSTANCE_BUSY|实例正在执行任务，不能回滚".into());
    }
    update_plan::rollback(std::path::Path::new(&inst.dirs.mods), &batch_id)
}

/// 清理过期备份（只动 .astral-backup）
#[tauri::command]
fn prune_mod_update_backups(
    instance_id: String,
    keep_days: Option<u64>,
) -> Result<(u64, u64), String> {
    let inst = managed_instance(&instance_id)?;
    update_plan::prune(
        std::path::Path::new(&inst.dirs.mods),
        keep_days.unwrap_or(7).clamp(1, 365),
    )
}

// ── B3 完整游戏文件检查与智能修复 ─────────────────────────────

/// 只读：体检一个版本的完整性（缺失 / 损坏 / Java 兼容）。
///
/// `deep` 为 true 时逐个算 SHA1（慢，但能发现"大小对、内容错"）。
#[tauri::command]
fn check_version_files(
    instance_id: Option<String>,
    version: String,
    deep: Option<bool>,
) -> Result<verify::VersionReport, String> {
    let game = game_root();
    let ver = match &instance_id {
        Some(id) => {
            let inst = instance::find(&game, id, &play_stats)
                .ok_or_else(|| "INSTANCE_NOT_FOUND|实例不存在".to_string())?;
            inst.launch_version.unwrap_or(inst.game_version)
        }
        None => version,
    };
    let opts = verify::CheckOptions {
        deep: deep.unwrap_or(false),
        ..Default::default()
    };
    Ok(verify::check(&game, &ver, &opts))
}

/// 只读：生成修复计划，给玩家看要做什么。
#[tauri::command]
fn plan_version_repair(
    instance_id: Option<String>,
    version: String,
    deep: Option<bool>,
) -> Result<(verify::VersionReport, verify::RepairPlan), String> {
    let game = game_root();
    let (ver, kind, lver, inst_id) = match &instance_id {
        Some(id) => {
            let inst = instance::find(&game, id, &play_stats)
                .ok_or_else(|| "INSTANCE_NOT_FOUND|实例不存在".to_string())?;
            let kind = inst.loader.as_ref().map(|l| l.kind.clone());
            let lver = inst.loader.as_ref().map(|l| l.version.clone());
            (
                inst.launch_version.unwrap_or(inst.game_version),
                kind,
                lver,
                Some(id.clone()),
            )
        }
        None => (version, None, None, None),
    };
    let opts = verify::CheckOptions {
        deep: deep.unwrap_or(false),
        ..Default::default()
    };
    let report = verify::check(&game, &ver, &opts);
    let plan = verify::plan(
        &game,
        inst_id.as_deref(),
        &ver,
        &report,
        kind.as_deref(),
        lver.as_deref(),
    );
    Ok((report, plan))
}

/// 执行修复（后台任务）。只补缺失/损坏，不动 mods / config / saves。
#[tauri::command]
fn repair_version(
    app: AppHandle,
    instance_id: Option<String>,
    version: String,
    plan: verify::RepairPlan,
    allow_replace: Option<bool>,
) -> Result<String, String> {
    let game = game_root();
    let t = task::create(
        "repair",
        instance_id.clone(),
        Some(version.clone()),
        format!("修复 {version}"),
    );
    let tid = t.task_id.clone();
    let out_id = tid.clone();
    let cancel = net::cancel_from({
        let tc = tid.clone();
        move || task::is_cancelled(&tc)
    });
    std::thread::spawn(move || {
        task::start(&tid);
        let r = verify::repair(
            &game,
            &version,
            &plan,
            allow_replace.unwrap_or(false),
            &*cancel,
            |done, total, stage| {
                task::progress(&tid, "download", stage.to_string(), done, total);
            },
        );
        match r {
            Ok(res) => {
                let failed = res.failed.len();
                let payload = serde_json::to_value(&res).unwrap_or(serde_json::Value::Null);
                if failed == 0 {
                    task::finish(&tid, payload.clone());
                } else {
                    task::fail(
                        &tid,
                        format!(
                            "VERIFY_FAILED|修复完成但有 {failed} 项失败|{}",
                            res.failed
                                .iter()
                                .map(|(k, v)| format!("{k}: {}", task::decode(v).message))
                                .collect::<Vec<_>>()
                                .join("；")
                        ),
                    );
                }
                let _ = app.emit("astral://repair", payload);
            }
            Err(e) => task::fail(&tid, e),
        }
    });
    Ok(out_id)
}

/// 列出修复备份批次
#[tauri::command]
fn repair_backups() -> Vec<String> {
    verify::backups(&game_root())
}

/// 只读：解析一个 Mod 的完整依赖树，不下载任何东西。
///
/// 前端拿它做"安装前预览"：能看到每个依赖为什么被装、会替换哪个版本、
/// 有哪些冲突。真正安装仍然走 `prepare_resource_install`。
#[tauri::command]
fn resolve_mod_dependencies(
    instance_id: String,
    project_id: String,
    version_id: String,
) -> Result<deps::Resolved, String> {
    let inst = managed_instance(&instance_id)?;
    if !inst.managed {
        return Err("UNSUPPORTED|兼容实例共享游戏目录，不能解析依赖".into());
    }
    let (installed, installed_unknown) =
        deps::scan_installed(std::path::Path::new(&inst.dirs.mods));
    let req = deps::Request {
        project_id,
        version_id,
        game_version: inst.game_version.clone(),
        loader: inst.loader.as_ref().map(|l| l.kind.clone()),
        installed,
        installed_unknown,
        optional: std::collections::HashSet::new(),
    };
    Ok(deps::resolve(
        &deps::ModrinthSource,
        &req,
        &deps::Limits::default(),
    ))
}

#[tauri::command]
fn list_mods(instance_id: String) -> Result<Vec<mods::ModFile>, String> {
    let inst = managed_instance(&instance_id)?;
    Ok(mods::list(std::path::Path::new(&inst.dirs.mods)))
}

#[tauri::command]
fn set_mod_enabled(
    instance_id: String,
    file_name: String,
    enabled: bool,
) -> Result<mods::ModFile, String> {
    let inst = managed_instance(&instance_id)?;
    mods::set_enabled(std::path::Path::new(&inst.dirs.mods), &file_name, enabled)
}

#[tauri::command]
fn delete_mod(instance_id: String, file_name: String) -> Result<(), String> {
    let inst = managed_instance(&instance_id)?;
    mods::delete(std::path::Path::new(&inst.dirs.mods), &file_name)
}

// ── 资源包 / 光影包 ─────────────────────────────────────────────

fn pack_dir(instance_id: &str, kind: &str) -> Result<PathBuf, String> {
    let inst = managed_instance(instance_id)?;
    let dir = match packs::normalize_kind(kind)? {
        "resourcepack" => inst.dirs.resourcepacks.clone(),
        "shaderpack" => inst.dirs.shaderpacks.clone(),
        other => return Err(format!("INVALID_VALUE|这个类型不支持文件管理：{other}")),
    };
    Ok(PathBuf::from(dir))
}

#[tauri::command]
fn list_packs(instance_id: String, kind: String) -> Result<Vec<packs::PackFile>, String> {
    let dir = pack_dir(&instance_id, &kind)?;
    packs::list(&dir, &kind)
}

#[tauri::command]
fn set_pack_enabled(
    instance_id: String,
    kind: String,
    file_name: String,
    enabled: bool,
) -> Result<packs::PackFile, String> {
    let dir = pack_dir(&instance_id, &kind)?;
    packs::set_enabled(&dir, &kind, &file_name, enabled)
}

#[tauri::command]
fn delete_pack(
    instance_id: String,
    kind: String,
    file_name: String,
    confirm: bool,
) -> Result<(), String> {
    let dir = pack_dir(&instance_id, &kind)?;
    packs::delete(&dir, &kind, &file_name, confirm)
}

// ── 本地 Mod 静态扫描 ────────────────────────────────────────────

/// 离线扫描实例的 mods 目录：重复 mod id、缺失依赖、版本不满足、声明的冲突。
///
/// 只读，不下载也不修改任何文件；和 `resolve_mod_dependencies`（联网递归解析）
/// 互补：那个回答"装之前要下什么"，这个回答"现在这一堆为什么起不来"。
#[tauri::command]
fn scan_mods(instance_id: String) -> Result<modscan::ModScanReport, String> {
    let inst = managed_instance(&instance_id)?;
    Ok(modscan::scan(std::path::Path::new(&inst.dirs.mods)))
}

// ── legacy（阻塞语义，保持不变）──────────────────────────────────

#[tauri::command]
async fn install_version(app: AppHandle, version: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let cfg = core::load_config();
        let dir = PathBuf::from(&cfg.game_dir);
        let v = version.clone();
        core::install(&version, &dir, &cfg.username, move |p| {
            let _ = app.emit("astral://progress", (&v, &p));
        })
        .map(|_| ())
        .map_err(legacy_msg)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn launch_version(app: AppHandle, version: String) -> Result<u32, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let cfg = core::load_config();
        let dir = PathBuf::from(&cfg.game_dir);
        let v = version.clone();
        core::launch(&version, &dir, &cfg.username, move |p| {
            let _ = app.emit("astral://progress", (&v, &p));
        })
        .map_err(legacy_msg)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// legacy 命令的报错保持人话，不带 CODE 前缀（旧界面直接展示）
fn legacy_msg(encoded: String) -> String {
    let e = task::decode(&encoded);
    match e.detail {
        Some(d) => format!("{}（{}）", e.message, d),
        None => e.message,
    }
}

/// 旧版下载命令的目录规则（可单独测试）
///
/// 规则：Mod / 数据包必须指定实例，不允许直接写共享游戏目录；
/// 资源包 / 光影包 / 整合包允许落到全局目录（它们本来就不是实例专属）。
pub fn legacy_sub_dir(kind: &str) -> &'static str {
    match kind {
        "resourcepack" => "resourcepacks",
        "shaderpack" => "shaderpacks",
        "modpack" => "modpacks",
        "datapack" => "datapacks",
        _ => "mods",
    }
}

pub fn legacy_requires_instance(kind: &str) -> bool {
    matches!(kind, "mod" | "datapack")
}

/// ⚠️ 旧接口：新代码请用 prepare_resource_install + start_resource_install。
///
/// 安全加固：
/// - Mod / 数据包必须给 instanceId，禁止写共享目录；
/// - 目标已存在时默认拒绝覆盖（除非显式 overwrite=true）；
/// - 可选传入 sha1 / sha512，下载后校验（校验通过才落正式文件）；
/// - 目标路径同时校验落在实例目录（若给了实例）与游戏目录内；
/// - 重定向每一跳都会重新过白名单，DNS 解析到内网也会被拒绝。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn download_to(
    app: AppHandle,
    url: String,
    kind: String,
    filename: String,
    instance_id: Option<String>,
    sha1: Option<String>,
    sha512: Option<String>,
    overwrite: Option<bool>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let cfg = core::load_config();
        let game = PathBuf::from(&cfg.game_dir);
        let sub = legacy_sub_dir(&kind);

        if instance_id.is_none() && legacy_requires_instance(&kind) {
            return Err(
                "INVALID_VALUE|Mod / 数据包必须指定实例；建议改用 prepare_resource_install".into(),
            );
        }

        let (dir, root) = match instance_id {
            Some(id) => {
                let inst = instance::find(&game, &id, &play_stats)
                    .ok_or_else(|| "INSTANCE_NOT_FOUND|实例不存在".to_string())?;
                let d = PathBuf::from(&inst.dir);
                (d.join(sub), d)
            }
            None => (game.join(sub), game.clone()),
        };

        let safe = PathBuf::from(&filename)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "download.bin".into());

        // 目标必须落在（实例或游戏）目录内，且先落 .part
        let dest = net::ensure_within(&root, &dir.join(&safe))?;
        let _ = net::ensure_within(&game, &dest)?;

        if dest.exists() && overwrite != Some(true) {
            return Err(format!(
                "FILE_EXISTS|{} 已存在；如需覆盖请显式传 overwrite=true",
                safe
            ));
        }

        let name = safe.clone();
        let app2 = app.clone();
        net::download_verified(
            &url,
            &dest,
            sha1.as_deref(),
            sha512.as_deref(),
            move |c: u32, tc: u32, s: u32, ts: u32| {
                let _ = app2.emit("astral://file", (&name, c, tc, s, ts));
            },
        )?;
        Ok(dest.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

// ── 启动 ────────────────────────────────────────────────────────

static APP_HANDLE: std::sync::OnceLock<AppHandle> = std::sync::OnceLock::new();

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle().clone();
            let _ = APP_HANDLE.set(handle.clone());
            task::init(handle);
            task::boot();
            session::load();
            session::reconcile();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_api_info,
            get_system_info,
            get_config,
            set_config,
            scan_versions,
            open_path,
            list_versions,
            list_instances,
            get_instance,
            get_active_instance,
            set_active_instance,
            update_instance,
            delete_instance,
            list_trash,
            restore_from_trash,
            purge_trash,
            purge_all_trash,
            get_tasks,
            get_task,
            cancel_task,
            retry_task,
            start_install,
            start_launch,
            start_create_instance,
            prepare_game_install,
            start_game_install,
            get_active_session,
            list_sessions,
            list_saves,
            get_stats,
            net_scheduler_stats,
            cache_stats,
            resolve_mod_dependencies,
            check_version_files,
            plan_version_repair,
            repair_version,
            repair_backups,
            plan_mod_updates,
            apply_mod_updates,
            mod_update_history,
            rollback_mod_updates,
            prune_mod_update_backups,
            cache_enforce_budget,
            cache_clear,
            clean_partials,
            disk_usage,
            list_mods,
            set_mod_enabled,
            delete_mod,
            list_packs,
            set_pack_enabled,
            delete_pack,
            scan_mods,
            list_loaders,
            start_install_loader,
            list_loader_builds,
            repair_instance,
            list_target_worlds,
            get_world_info,
            scan_local_versions,
            plan_instance_copy,
            plan_version_import,
            apply_instance_copy,
            list_world_backups,
            backup_world,
            preview_world_restore,
            restore_world,
            delete_world_backup,
            get_resource_project,
            list_resource_files,
            search_resources,
            prepare_resource_install,
            start_resource_install,
            export_modpack,
            modpack_installed_versions,
            verify_exported_modpack,
            check_mod_updates,
            start_mod_update,
            msa_status,
            get_account,
            get_component_catalog,
            list_component_versions,
            list_java_runtimes,
            list_versions_cached,
            meta_cache_stats,
            prune_meta_cache,
            clear_meta_cache,
            java_audit,
            check_instance_java,
            remove_managed_java,
            java_cache_stats,
            clear_java_cache,
            install_java,
            list_managed_jdks,
            prepare_modpack_install,
            import_local_modpack,
            start_modpack_install,
            list_logs,
            read_log,
            prepare_diagnosis,
            tail_log,
            ping_server,
            list_servers,
            add_server,
            update_server,
            remove_server,
            reorder_servers,
            refresh_servers,
            diagnose_instance,
            install_version,
            launch_version,
            download_to
        ])
        .run(tauri::generate_context!())
        .expect("error while running astral");
}

/// 无界面自检：`astral.exe --selftest`。
///
/// 本机的 `cargo test` 二进制在加载期就挂（0xc0000139），所以把关键断言
/// 放进 app 二进制里跑，结果写 `%TEMP%\astral-selftest.log`，进程退出码 = 失败数。
pub fn selftest() -> i32 {
    let mut fails = 0i32;
    let mut log = String::new();
    let mut results: Vec<(String, bool)> = Vec::new();
    macro_rules! check {
        ($name:expr, $cond:expr) => {{
            let ok = $cond;
            results.push(($name.to_string(), ok));
            if ok {
                log.push_str(&format!("PASS {}\n", $name));
            } else {
                fails += 1;
                log.push_str(&format!("FAIL {}\n", $name));
            }
        }};
    }

    // 任务持久化用临时目录，避免污染真实配置
    let task_dir = std::env::temp_dir().join(format!("astral-tasks-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&task_dir);
    std::env::set_var("ASTRAL_DATA_DIR", &task_dir);
    task::boot();

    let e = error::ApiError::with_detail(error::NETWORK, "下载失败", "a|b");
    let d = task::decode(&e.encode());
    check!(
        "error roundtrip",
        d.code == "NETWORK" && d.detail.as_deref() == Some("a|b")
    );

    check!(
        "time epoch",
        time::unix_ms_to_iso(0) == "1970-01-01T00:00:00Z"
            && time::iso_to_unix_ms("1970-01-01T00:00:00Z") == Some(0)
    );
    let iso = "2026-10-23T00:00:00Z";
    check!(
        "time roundtrip",
        time::iso_to_unix_ms(iso)
            .map(time::unix_ms_to_iso)
            .as_deref()
            == Some(iso)
    );
    check!(
        "time leap",
        time::iso_to_unix_ms("2024-02-29T12:00:00Z")
            .map(time::unix_ms_to_iso)
            .as_deref()
            == Some("2024-02-29T12:00:00Z")
    );

    let game = std::env::temp_dir().join(format!("astral-selftest-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&game);
    let _ = std::fs::create_dir_all(game.join("versions/1.20.1"));
    let _ = std::fs::write(game.join("versions/1.20.1/1.20.1.json"), "{}");
    let _ = std::fs::write(game.join("versions/1.20.1/1.20.1.jar"), "x");
    let _ = std::fs::create_dir_all(game.join("assets"));

    let meta = instance::InstanceMeta {
        id: instance::new_id(),
        name: "自检实例".into(),
        game_version: "1.20.1".into(),
        loader_kind: None,
        loader_version: None,
        launch_version: None,
        memory_mb: None,
        jvm_args: vec![],
        java_path: None,
        cover: None,
        created_at: time::now_iso(),
    };
    check!("instance create", instance::create(&game, &meta).is_ok());
    let list = instance::discover(&game, &|_| instance::PlayStats::default());
    check!(
        "instance discover managed",
        list.iter().any(|i| i.id == meta.id && i.managed)
    );
    check!(
        "instance discover legacy",
        list.iter().any(|i| i.id == "ver_1.20.1" && !i.managed)
    );
    check!(
        "instance health ok",
        list.iter().any(|i| i.id == meta.id && i.health.ok)
    );

    let mods = instance::instance_dir(&game, &meta.id).join("mods");
    let had_mods = std::fs::write(mods.join("a.jar"), "x").is_ok()
        && std::fs::write(mods.join("b.jar.disabled"), "x").is_ok();
    let after = instance::discover(&game, &|_| instance::PlayStats::default());
    check!(
        "instance mods counted",
        had_mods
            && after
                .iter()
                .any(|i| i.id == meta.id && i.mods_count == 1 && i.mods_disabled == 1)
    );

    // 模组：列表 / 启停 / 删除 / 路径穿越防护
    let mod_dir = std::env::temp_dir().join(format!("astral-mods-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&mod_dir);
    let _ = std::fs::create_dir_all(&mod_dir);
    let _ = std::fs::write(mod_dir.join("a.jar"), b"x");
    let _ = std::fs::write(mod_dir.join("b.jar.disabled"), b"y");
    let listed = mods::list(&mod_dir);
    check!("mods listed", listed.len() == 2);
    check!(
        "mods enabled flag",
        listed.iter().filter(|m| m.enabled).count() == 1
    );
    check!(
        "mod disable",
        mods::set_enabled(&mod_dir, "a.jar", false)
            .map(|m| m.file_name == "a.jar.disabled" && !m.enabled)
            .unwrap_or(false)
    );
    check!(
        "mod enable",
        mods::set_enabled(&mod_dir, "a.jar.disabled", true)
            .map(|m| m.file_name == "a.jar" && m.enabled)
            .unwrap_or(false)
    );
    check!("mod delete", mods::delete(&mod_dir, "a.jar").is_ok());
    check!(
        "mod path guard",
        mods::delete(&mod_dir, "../a.jar").is_err()
    );
    let _ = std::fs::remove_dir_all(&mod_dir);

    // 资源包 / 光影包：文件层管理（pack_format 解析、启停、删除确认）
    let pack_dir = std::env::temp_dir().join(format!("astral-packs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&pack_dir);
    let _ = std::fs::create_dir_all(&pack_dir);
    // 造一个真的带 pack.mcmeta 的资源包
    {
        let f = std::fs::File::create(pack_dir.join("Better Leaves.zip")).unwrap();
        let mut zw = zip::ZipWriter::new(f);
        let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default();
        use std::io::Write as _;
        zw.start_file("pack.mcmeta", opts).unwrap();
        zw.write_all(br#"{"pack":{"pack_format":15,"description":"Leaves"}}"#)
            .unwrap();
        zw.finish().unwrap();
    }
    // 再放一个没有 pack.mcmeta 的 zip，验证"会提示游戏可能不认"
    let _ = std::fs::write(pack_dir.join("broken.zip"), b"not a zip");
    let _ = std::fs::write(pack_dir.join("note.txt"), b"ignore me");
    let packs_listed = packs::list(&pack_dir, "resourcepack").unwrap_or_default();
    check!(
        "packs list reads pack format",
        packs_listed.len() == 2
            && packs_listed
                .iter()
                .any(|p| p.file_name == "Better Leaves.zip"
                    && p.pack_format == Some(15)
                    && p.description.as_deref() == Some("Leaves")
                    && p.compatible_versions.iter().any(|v| v.contains("1.20")))
            && packs_listed
                .iter()
                .any(|p| p.file_name == "broken.zip" && p.pack_format.is_none())
    );
    check!(
        "packs disable renames file",
        packs::set_enabled(&pack_dir, "resourcepack", "Better Leaves.zip", false)
            .map(|p| p.file_name == "Better Leaves.zip.disabled" && !p.enabled)
            .unwrap_or(false)
            && pack_dir.join("Better Leaves.zip.disabled").exists()
    );
    check!(
        "packs enable renames back",
        packs::set_enabled(
            &pack_dir,
            "resourcepack",
            "Better Leaves.zip.disabled",
            true
        )
        .map(|p| p.file_name == "Better Leaves.zip" && p.enabled)
        .unwrap_or(false)
    );
    check!(
        "packs delete needs confirm",
        packs::delete(&pack_dir, "resourcepack", "broken.zip", false).is_err()
            && pack_dir.join("broken.zip").exists()
            && packs::delete(&pack_dir, "resourcepack", "broken.zip", true).is_ok()
            && !pack_dir.join("broken.zip").exists()
    );
    check!(
        "packs reject bad kind and names",
        packs::list(&pack_dir, "taco").is_err()
            && packs::delete(&pack_dir, "resourcepack", "../x.zip", true).is_err()
    );
    let _ = std::fs::remove_dir_all(&pack_dir);

    // 本地 Mod 静态扫描：重复 id / 缺依赖 / 版本不满足 / 干净目录保持安静
    let scan_dir = std::env::temp_dir().join(format!("astral-scan-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scan_dir);
    let _ = std::fs::create_dir_all(&scan_dir);
    {
        use std::io::Write as _;
        let mk = |name: &str, body: &str| {
            let f = std::fs::File::create(scan_dir.join(name)).unwrap();
            let mut zw = zip::ZipWriter::new(f);
            let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default();
            zw.start_file("fabric.mod.json", opts).unwrap();
            zw.write_all(body.as_bytes()).unwrap();
            zw.finish().unwrap();
        };
        // alpha 装了两份（重复 id），并要求 beta >=2.0 与一个根本没装的 mod
        mk(
            "alpha.jar",
            r#"{"id":"alpha","name":"Alpha","version":"1.0.0","depends":{"minecraft":">=1.20","beta":">=2.0","ghostlib":">=1.0"}}"#,
        );
        mk(
            "alpha-copy.jar",
            r#"{"id":"alpha","name":"Alpha","version":"1.0.0"}"#,
        );
        mk(
            "beta.jar",
            r#"{"id":"beta","name":"Beta","version":"1.0.0"}"#,
        );
    }
    let scan_report = modscan::scan(&scan_dir);
    check!(
        "modscan finds duplicate id",
        scan_report.counts.total == 3
            && scan_report.issues.iter().any(|i| i.code == "duplicate-id"
                && i.mod_id.as_deref() == Some("alpha")
                && i.severity == "error")
    );
    check!(
        "modscan finds missing dependency",
        scan_report
            .issues
            .iter()
            .any(|i| i.code == "missing-dependency"
                && i.mod_id.as_deref() == Some("ghostlib")
                && i.severity == "error")
    );
    check!(
        "modscan flags version mismatch",
        scan_report
            .issues
            .iter()
            .any(|i| i.code == "version-mismatch"
                && i.mod_id.as_deref() == Some("beta")
                && i.severity == "warn")
    );
    check!(
        "modscan counts and summary",
        scan_report.counts.enabled == 3
            && scan_report.counts.errors >= 2
            && scan_report.counts.warnings >= 1
            && scan_report.summary.is_some()
    );
    // 干净目录必须一条问题都不报（避免"什么都报警"的假阳性）
    let clean_dir = std::env::temp_dir().join(format!("astral-scan-ok-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&clean_dir);
    let _ = std::fs::create_dir_all(&clean_dir);
    {
        use std::io::Write as _;
        let f = std::fs::File::create(clean_dir.join("solo.jar")).unwrap();
        let mut zw = zip::ZipWriter::new(f);
        let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default();
        zw.start_file("fabric.mod.json", opts).unwrap();
        zw.write_all(
            br#"{"id":"solo","name":"Solo","version":"1.0.0","depends":{"minecraft":">=1.20"}}"#,
        )
        .unwrap();
        zw.finish().unwrap();
    }
    let clean_report = modscan::scan(&clean_dir);
    check!(
        "modscan quiet on clean dir",
        clean_report.issues.is_empty()
            && clean_report.summary.is_none()
            && clean_report.counts.errors == 0
    );
    let _ = std::fs::remove_dir_all(&scan_dir);
    let _ = std::fs::remove_dir_all(&clean_dir);

    // 账本：Astral 装过的 Mod 能被依赖解析认出版本，玩家手放的仍算未知
    {
        let led_dir = std::env::temp_dir().join(format!("astral-ledger-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&led_dir);
        let _ = std::fs::create_dir_all(&led_dir);
        std::fs::write(led_dir.join("sodium-0.5.13.jar"), b"x").unwrap();
        let _ = ledger::record(
            &led_dir,
            "sodium",
            "ver-sodium",
            "0.5.13",
            "sodium-0.5.13.jar",
        );
        // 玩家手动丢进来的（没有账本条目）——必须是真 zip，否则读不出 project id
        {
            use std::io::Write as _;
            let f = std::fs::File::create(led_dir.join("manual.jar")).unwrap();
            let mut zw = zip::ZipWriter::new(f);
            let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default();
            zw.start_file("fabric.mod.json", opts).unwrap();
            zw.write_all(
                br#"{"id":"manualmod","version":"1.0","contact":{"homepage":"https://modrinth.com/mod/manualmod"}}"#,
            )
            .unwrap();
            zw.finish().unwrap();
        }
        let (known, unknown) = deps::scan_installed(&led_dir);
        check!(
            "ledger marks astral-installed as known",
            known.get("sodium").map(String::as_str) == Some("ver-sodium")
        );
        check!(
            "ledger keeps manual jars unknown",
            unknown.contains("manualmod") && !known.contains_key("manualmod")
        );
        // 文件被玩家删掉之后，账本不该再算它已装
        let _ = std::fs::remove_file(led_dir.join("sodium-0.5.13.jar"));
        let (known2, _) = deps::scan_installed(&led_dir);
        check!(
            "ledger forgets missing files",
            !known2.contains_key("sodium")
        );
        let _ = std::fs::remove_dir_all(&led_dir);
    }

    check!(
        "instance delete to trash",
        instance::delete(&game, &meta.id, true).is_ok()
    );
    check!(
        "instance trash exists",
        instance::instances_root(&game)
            .join(".trash")
            .read_dir()
            .map(|d| d.count())
            .unwrap_or(0)
            == 1
    );

    let t = task::create("install", None, Some("1.20.1".into()), "安装 1.20.1");
    let rev0 = t.revision;
    task::progress(&t.task_id, "download", "下载 1/2", 1, 2);
    let rev1 = task::get(&t.task_id).map(|x| x.revision).unwrap_or(0);
    task::finish(
        &t.task_id,
        serde_json::json!({"kind":"install","gameVersion":"1.20.1"}),
    );
    let done = task::get(&t.task_id);
    check!(
        "task lifecycle",
        rev1 == rev0 + 1
            && done
                .as_ref()
                .map(|x| x.state == "done" && x.result.is_some())
                .unwrap_or(false)
    );
    check!(
        "task snapshot",
        !task::snapshot(None, None, None).is_empty()
    );

    // 取消：受理 → worker 确认 → 终态粘滞
    let t3 = task::create("install", None, Some("1.20.1".into()), "取消测试");
    check!(
        "task boot id",
        task::get(&t3.task_id)
            .map(|t| !t.boot_id.is_empty())
            .unwrap_or(false)
    );
    check!(
        "task cancel accepted",
        task::cancel_task(&t3.task_id) && task::is_cancelled(&t3.task_id)
    );
    task::cancelled(&t3.task_id);
    check!(
        "task cancel terminal",
        task::get(&t3.task_id)
            .map(|t| t.state == "cancelled" && !t.cancelling)
            .unwrap_or(false)
    );
    task::progress(&t3.task_id, "download", "迟到的进度", 1, 2);
    check!(
        "task cancel sticky",
        task::get(&t3.task_id)
            .map(|t| t.state == "cancelled")
            .unwrap_or(false)
    );
    check!("task persisted", task_dir.join("tasks.json").exists());

    // 取消令牌不能泄漏：任务进终态后 tokens 里不该还留着它
    check!(
        "task token released",
        task::health()["tokens"].as_u64().unwrap_or(99)
            < task::health()["tasks"].as_u64().unwrap_or(0)
            || task::health()["tokens"].as_u64() == Some(0)
    );

    // 状态单调：取消请求发出后，迟到的进度不能把它改回 running
    let tc2 = task::create("install", None, Some("1.20.1".into()), "取消单调测试");
    task::start(&tc2.task_id);
    let _ = task::cancel_task(&tc2.task_id);
    task::progress(&tc2.task_id, "download", "迟到的进度", 1, 2);
    check!(
        "task cancelling not reverted by progress",
        task::get(&tc2.task_id)
            .map(|t| t.state != "running")
            .unwrap_or(false)
    );
    task::cancelled(&tc2.task_id);
    // ── B1 依赖图：用假 source 验证遍历、环、缺失、上限、已装复用、可选 ──
    {
        struct FakeDeps {
            by_id: std::collections::HashMap<String, (deps::Candidate, Option<String>)>,
            by_project: std::collections::HashMap<String, Vec<deps::Candidate>>,
        }
        impl deps::DepSource for FakeDeps {
            fn versions_for(
                &self,
                pid: &str,
                _g: &str,
                _l: Option<&str>,
            ) -> Result<Vec<deps::Candidate>, String> {
                Ok(self.by_project.get(pid).cloned().unwrap_or_default())
            }
            fn version_by_id(
                &self,
                vid: &str,
            ) -> Result<(deps::Candidate, Option<String>), String> {
                self.by_id
                    .get(vid)
                    .cloned()
                    .ok_or_else(|| "not found".to_string())
            }
        }
        fn mk(pid: &str, vid: &str, deps: Vec<deps::Dep>) -> deps::Candidate {
            deps::Candidate {
                project_id: pid.into(),
                version_id: vid.into(),
                name: pid.into(),
                version_number: vid.into(),
                channel: "release".into(),
                file_name: format!("{pid}.jar"),
                url: format!("https://cdn.modrinth.com/data/{pid}/x.jar"),
                size: 100,
                sha1: None,
                sha512: None,
                game_versions: vec!["1.20.1".into()],
                loaders: vec!["fabric".into()],
                dependencies: deps,
            }
        }
        fn rq(pid: &str, vid: &str) -> deps::Request {
            deps::Request {
                project_id: pid.into(),
                version_id: vid.into(),
                game_version: "1.20.1".into(),
                loader: Some("fabric".into()),
                installed: std::collections::HashMap::new(),
                installed_unknown: std::collections::HashSet::new(),
                optional: std::collections::HashSet::new(),
            }
        }
        fn req_dep(pid: &str) -> deps::Dep {
            deps::Dep {
                project_id: Some(pid.into()),
                version_id: None,
                kind: deps::DepKind::Required,
            }
        }
        fn mk_fake(list: Vec<deps::Candidate>) -> FakeDeps {
            let mut f = FakeDeps {
                by_id: Default::default(),
                by_project: Default::default(),
            };
            for c in list {
                f.by_project
                    .entry(c.project_id.clone())
                    .or_default()
                    .push(c.clone());
                f.by_id.insert(
                    c.version_id.clone(),
                    (c.clone(), Some(c.project_id.clone())),
                );
            }
            f
        }

        let f = mk_fake(vec![
            mk("root", "r1", vec![req_dep("lib")]),
            mk("lib", "l1", vec![req_dep("core")]),
            mk("core", "c1", vec![]),
        ]);
        let r = deps::resolve(&f, &rq("root", "r1"), &deps::Limits::default());
        check!(
            "deps resolve nested order",
            !r.blocking()
                && r.nodes
                    .iter()
                    .map(|n| n.project_id.as_str())
                    .collect::<Vec<_>>()
                    == vec!["core", "lib", "root"]
        );
        check!(
            "deps via chain",
            r.nodes
                .iter()
                .find(|n| n.project_id == "core")
                .map(|n| n.via == vec!["root".to_string(), "lib".to_string()] && n.depth == 2)
                .unwrap_or(false)
        );

        let f2 = mk_fake(vec![
            mk("a", "a1", vec![req_dep("b")]),
            mk("b", "b1", vec![req_dep("a")]),
        ]);
        let r2 = deps::resolve(&f2, &rq("a", "a1"), &deps::Limits::default());
        check!("deps detects cycle", r2.blocking() && !r2.cycles.is_empty());

        let f3 = mk_fake(vec![mk("root", "r1", vec![req_dep("ghost")])]);
        let r3 = deps::resolve(&f3, &rq("root", "r1"), &deps::Limits::default());
        check!(
            "deps reports missing",
            r3.blocking() && r3.conflicts.iter().any(|c| c.project_id == "ghost")
        );

        let mut chain = Vec::new();
        for i in 0..12 {
            let next = if i == 11 {
                vec![]
            } else {
                vec![req_dep(&format!("p{}", i + 1))]
            };
            chain.push(mk(&format!("p{i}"), &format!("v{i}"), next));
        }
        let f4 = mk_fake(chain);
        let lim = deps::Limits {
            max_depth: 4,
            ..Default::default()
        };
        let r4 = deps::resolve(&f4, &rq("p0", "v0"), &lim);
        check!("deps depth limit", r4.truncated && r4.nodes.len() <= 6);

        let f5 = mk_fake(vec![
            mk("root", "r1", vec![req_dep("lib"), req_dep("mystery")]),
            mk("lib", "l1", vec![]),
        ]);
        let mut rq5 = rq("root", "r1");
        rq5.installed.insert("lib".into(), "l1".into());
        rq5.installed_unknown.insert("mystery".into());
        let r5 = deps::resolve(&f5, &rq5, &deps::Limits::default());
        check!(
            "deps skips installed",
            !r5.blocking()
                && r5.nodes.len() == 1
                && r5.skipped.iter().any(|s| s.reason == "installed")
                && r5.skipped.iter().any(|s| s.reason == "installed-unknown")
        );

        let mut root6 = mk("root", "r1", vec![]);
        root6.dependencies.push(deps::Dep {
            project_id: Some("opt".into()),
            version_id: None,
            kind: deps::DepKind::Optional,
        });
        let f6 = mk_fake(vec![root6, mk("opt", "o1", vec![])]);
        let r6 = deps::resolve(&f6, &rq("root", "r1"), &deps::Limits::default());
        check!(
            "deps optional skipped",
            r6.nodes.len() == 1 && r6.skipped.iter().any(|s| s.reason == "optional")
        );
    }
    // ── B2 批量更新：备份、回滚、历史 ──
    {
        let mods_dir = game.join("mods");
        let _ = std::fs::create_dir_all(&mods_dir);

        // 造两个"已装模组"
        let a = mods_dir.join("alpha-1.0.jar");
        let b = mods_dir.join("beta-2.0.jar");
        std::fs::write(&a, b"alpha-old").unwrap();
        std::fs::write(&b, b"beta-old").unwrap();

        // 备份目录与历史
        check!(
            "update history empty at start",
            update_plan::history(&mods_dir).is_empty()
        );

        // 备份一个文件 → 历史里能看到 → 回滚恢复原内容
        let batch = update_plan::new_batch_id_for_test();
        let batch_path = mods_dir.join(update_plan::BACKUP_DIR).join(&batch);
        std::fs::create_dir_all(&batch_path).unwrap();
        std::fs::copy(&a, batch_path.join("alpha-1.0.jar.bak")).unwrap();
        let mut rec = update_plan::BackupRecord {
            batch_id: batch.clone(),
            created_at: crate::time::unix_ms_to_iso(crate::time::now_unix_ms()),
            files: std::collections::BTreeMap::new(),
            replaced: std::collections::BTreeMap::new(),
            previous: std::collections::BTreeMap::new(),
            note: "自检".into(),
        };
        rec.files
            .insert("alpha-1.0.jar".into(), "alpha-1.0.jar.bak".into());
        rec.replaced
            .insert("alpha-1.0.jar".into(), "alpha-2.0.jar".into());
        update_plan::save_history_for_test(&mods_dir, std::slice::from_ref(&rec));
        check!(
            "update history lists batch",
            update_plan::history(&mods_dir)
                .iter()
                .any(|r| r.batch_id == batch)
        );

        // 模拟"更新已完成"：旧文件换成新文件
        std::fs::write(&a, b"alpha-new").unwrap();
        std::fs::write(mods_dir.join("alpha-2.0.jar"), b"alpha-new").unwrap();
        std::fs::remove_file(&a).unwrap();
        let restored = update_plan::rollback(&mods_dir, &batch).unwrap();
        check!(
            "update rollback restores old file",
            restored == 1
                && std::fs::read(&a).unwrap() == b"alpha-old"
                && !mods_dir.join("alpha-2.0.jar").exists()
        );
        check!(
            "update rollback clears history",
            update_plan::history(&mods_dir).is_empty()
        );

        // 只动点名的文件：beta 全程没被碰过
        check!(
            "update rollback leaves other mods",
            std::fs::read(&b).unwrap() == b"beta-old"
        );

        // 计划：稳定版不许被自动升成 beta
        let item_beta = update_plan::UpdatePlanItem {
            file_name: "alpha-1.0.jar".into(),
            project_id: "P_alpha".into(),
            current_version: "1.0".into(),
            target_version: "2.0-beta".into(),
            target_version_id: "v2b".into(),
            target_file_name: "alpha-2.0-beta.jar".into(),
            size: 100,
            renames: true,
            blocked: Some("目标版本是 beta 通道，不会把稳定版自动升成测试版".into()),
            notes: vec![],
        };
        let plan = update_plan::UpdatePlan {
            plan_id: "uplan_selftest".into(),
            instance_id: "inst_x".into(),
            items: vec![item_beta],
            skipped: vec![],
            total_bytes: 0,
            blocking: true,
            created_at: crate::time::unix_ms_to_iso(crate::time::now_unix_ms()),
            expires_at: crate::time::unix_ms_to_iso(crate::time::now_unix_ms() + 60000),
        };
        check!(
            "update plan blocks beta promotion",
            plan.blocking && plan.items[0].blocked.is_some()
        );

        // 清理
        let _ = std::fs::remove_file(&a);
        let _ = std::fs::remove_file(&b);
        // ── B3 完整文件检查与修复计划 ──
        {
            let g = game.join("verify-game");
            let ver = "1.20.1";
            let vdir = g.join("versions").join(ver);
            std::fs::create_dir_all(&vdir).unwrap();
            std::fs::create_dir_all(g.join("libraries")).unwrap();
            std::fs::create_dir_all(g.join("assets").join("indexes")).unwrap();

            // 造一个"半残"的版本：json 有、jar 缺、库缺一个、资源索引缺
            let jar_bytes = b"fake-client-jar-bytes";
            let version_json = serde_json::json!({
                "id": ver,
                "javaVersion": { "majorVersion": 17 },
                "downloads": {
                    "client": {
                        "size": jar_bytes.len(),
                        "sha1": "0000000000000000000000000000000000000000"
                    }
                },
                "assetIndex": { "id": "5" },
                "libraries": [
                    {
                        "name": "org.example:present:1.0",
                        "downloads": {
                            "artifact": {
                                "path": "org/example/present/1.0/present-1.0.jar",
                                "size": 3,
                                "sha1": "1111111111111111111111111111111111111111"
                            }
                        }
                    },
                    {
                        "name": "org.example:missing:1.0",
                        "downloads": {
                            "artifact": {
                                "path": "org/example/missing/1.0/missing-1.0.jar",
                                "size": 4,
                                "sha1": "2222222222222222222222222222222222222222"
                            }
                        }
                    }
                ]
            });
            std::fs::write(
                vdir.join(format!("{ver}.json")),
                serde_json::to_string_pretty(&version_json).unwrap(),
            )
            .unwrap();
            let present = g
                .join("libraries")
                .join("org/example/present/1.0/present-1.0.jar");
            std::fs::create_dir_all(present.parent().unwrap()).unwrap();
            std::fs::write(&present, b"abc").unwrap();

            let report = verify::check(&g, ver, &verify::CheckOptions::default());
            check!(
                "verify finds missing jar",
                report.missing >= 1
                    && report
                        .files
                        .iter()
                        .any(|f| f.kind == "client-jar" && !f.exists)
            );
            check!(
                "verify counts missing library",
                report
                    .files
                    .iter()
                    .any(|f| f.kind == "library" && !f.exists && f.path.contains("missing"))
            );
            check!(
                "verify accepts present library by size",
                report
                    .files
                    .iter()
                    .any(|f| f.path.contains("present") && f.exists && f.complete == Some(true))
            );
            check!(
                "verify reports missing asset index",
                report
                    .files
                    .iter()
                    .any(|f| f.kind == "asset-index" && !f.exists)
            );
            check!(
                "verify report not complete",
                !report.complete && report.needs_repair()
            );

            // 修复计划：只读、不碰用户文件、给出 untouched 清单
            let plan = verify::plan(&g, Some("inst_v"), ver, &report, None, None);
            check!(
                "verify plan targets base reinstall",
                plan.actions.iter().any(|a| a.kind == "reinstall-base")
            );
            check!(
                "verify plan lists untouched user dirs",
                plan.untouched.iter().any(|s| s.contains("saves"))
                    && plan.untouched.iter().any(|s| s.contains("mods"))
            );
            check!(
                "verify plan never overwrites user files",
                plan.actions.iter().all(|a| !a.overwrites_user_file)
            );

            // 深度模式：大小对但内容错 → 必须被 sha1 抓出来
            let libs_json_path = vdir.join(format!("{ver}.json"));
            let mut v2: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&libs_json_path).unwrap()).unwrap();
            // 把 present 的 sha1 改成"正确内容 abc 的 sha1"
            let correct = crate::net::sha1_file(&present).unwrap();
            v2["libraries"][0]["downloads"]["artifact"]["sha1"] =
                serde_json::Value::String(correct.clone());
            std::fs::write(&libs_json_path, serde_json::to_string_pretty(&v2).unwrap()).unwrap();
            let shallow = verify::check(&g, ver, &verify::CheckOptions::default());
            check!(
                "verify shallow trusts size",
                shallow
                    .files
                    .iter()
                    .any(|f| f.path.contains("present") && f.complete == Some(true))
            );
            // 改内容但保持同样长度 → 浅检通过、深检必须失败
            std::fs::write(&present, b"abd").unwrap();
            let deep = verify::check(
                &g,
                ver,
                &verify::CheckOptions {
                    deep: true,
                    ..Default::default()
                },
            );
            check!(
                "verify deep detects corrupt content",
                deep.files
                    .iter()
                    .any(|f| f.path.contains("present") && f.complete == Some(false))
                    && deep.corrupt >= 1
            );

            // 修好一个文件后报告要跟着变（幂等，不缓存旧结论）
            std::fs::write(&present, b"abc").unwrap();
            let fixed = verify::check(
                &g,
                ver,
                &verify::CheckOptions {
                    deep: true,
                    ..Default::default()
                },
            );
            check!(
                "verify recheck reflects fix",
                fixed
                    .files
                    .iter()
                    .any(|f| f.path.contains("present") && f.complete == Some(true))
            );

            // 完整版本：不报任何修复动作
            let ok_dir = g.join("versions").join("2.0");
            std::fs::create_dir_all(&ok_dir).unwrap();
            std::fs::write(ok_dir.join("2.0.jar"), b"x").unwrap();
            std::fs::write(
                ok_dir.join("2.0.json"),
                serde_json::json!({ "id": "2.0" }).to_string(),
            )
            .unwrap();
            let ok_report = verify::check(&g, "2.0", &verify::CheckOptions::default());
            check!(
                "verify clean version has no actions",
                verify::plan(&g, None, "2.0", &ok_report, None, None)
                    .actions
                    .is_empty()
            );

            // 备份目录：覆盖前备份能落盘
            std::fs::write(g.join("probe.txt"), b"keep-me").unwrap();
            let bdir = g.join(verify::BACKUP_DIR).join("rep_test");
            std::fs::create_dir_all(&bdir).unwrap();
            std::fs::copy(g.join("probe.txt"), bdir.join("probe.txt")).unwrap();
            check!(
                "verify backups listed",
                verify::backups(&g).iter().any(|b| b == "rep_test")
            );

            let _ = std::fs::remove_dir_all(&g);
        }

        let _ = std::fs::remove_dir_all(mods_dir.join(update_plan::BACKUP_DIR));
    }

    check!(
        "task health shape",
        task::health()["tasks"].is_number()
            && task::health()["running"].is_number()
            && task::health()["cancelling"].is_number()
            && task::health()["tokens"].is_number()
            && task::health()["bootId"].is_string()
    );

    let sess = session::begin("inst_selftest", &t.task_id, 0);
    check!("session begin", !sess.session_id.is_empty());
    let exited = session::mark_exited(&sess.session_id, Some(0));
    check!(
        "session exit",
        exited
            .as_ref()
            .map(|s| s.state == "exited" && s.duration_ms.is_some())
            .unwrap_or(false)
    );
    check!(
        "session stats",
        session::stats_for("inst_selftest").last_played.is_some()
    );

    // 按 pid 对账：pid 0 永不存活，重启后应被收尾并标记 reconciled
    let _dead = session::begin("inst_reconcile", "task_r", 0);
    session::reconcile();
    let rec = session::list(Some("inst_reconcile"), 1);
    check!(
        "session reconcile",
        rec.first()
            .map(|s| s.reconciled && s.exit_code.is_none() && s.state == "exited")
            .unwrap_or(false)
    );

    // PID 复用：同一个 pid 但创建时间对不上，就不能算"还在运行"
    {
        let mut s = session::begin("inst_pidreuse", "task_pid", 0);
        // 真实 pid 但创建时间故意写错（比真实时间早 1 小时）
        let me = std::process::id();
        s.pid = me;
        s.pid_started_at = Some(1);
        session::save_for_test(vec![s.clone()]);
        session::reconcile();
        let listed = session::list(Some("inst_pidreuse"), 1);
        check!(
            "session detects pid reuse",
            listed
                .first()
                .map(|x| x.state == "exited" && x.reconciled)
                .unwrap_or(false)
        );
        // 创建时间对得上时，活着的进程不能被误判为已退出
        // begin() 自己会记录真实创建时间，这里直接用它
        let s2 = session::begin("inst_pidalive", "task_pid2", me);
        check!(
            "session records pid start time",
            s2.pid_started_at.is_some()
        );
        session::save_for_test(vec![s2]);
        session::reconcile();
        let listed2 = session::list(Some("inst_pidalive"), 1);
        check!(
            "session keeps live process",
            listed2
                .first()
                .map(|x| x.state != "exited")
                .unwrap_or(false)
        );
    }

    let save = game.join("saves").join("World1");
    let _ = std::fs::create_dir_all(&save);
    let _ = std::fs::write(save.join("level.dat"), "x");
    check!("saves listed", stats::list_saves(&game, None).len() == 1);

    // level.dat 是 gzip 压缩的 NBT：版本、难度、上次游玩都从里面读。
    // 读不出来也不能让整个存档列表失败——坏文件只是没有信息，不是没这个存档。
    {
        let entries = stats::list_saves(&game, None);
        check!(
            "saves survive a broken level.dat",
            entries
                .first()
                .map(|e| e.game_version.is_none() && e.info_note.is_some())
                .unwrap_or(false)
        );
        let _ = std::fs::write(
            save.join("level.dat"),
            nbt::sample_level_dat("1.20.1", 1, true),
        );
        let entries = stats::list_saves(&game, None);
        let first = entries.first();
        check!(
            "saves read version from level.dat",
            first
                .map(|e| e.game_version.as_deref() == Some("1.20.1"))
                .unwrap_or(false)
                && first.and_then(|e| e.version_id) == Some(3465)
                && first.and_then(|e| e.game_mode.as_deref()) == Some("创造")
                && first.and_then(|e| e.difficulty.as_deref()) == Some("普通")
                && first.and_then(|e| e.hardcore) == Some(true)
        );
        check!(
            "saves prefer last played from level.dat",
            first.and_then(|e| e.last_played.as_deref()) == Some("2023-11-14T22:13:20Z")
                && first.and_then(|e| e.level_name.as_deref()) == Some("测试世界")
                && first.map(|e| e.info_note.is_none()).unwrap_or(false)
        );
        check!(
            "worlds carry the real game version",
            worlds::list(&game.join("saves"))
                .first()
                .map(|w| w.game_version.as_deref() == Some("1.20.1"))
                .unwrap_or(false)
        );
        let wid = worlds::world_id("World1");
        let detail = worlds::detail(&game.join("saves"), &wid);
        check!(
            "world detail reads level info",
            detail
                .as_ref()
                .map(|d| d.game_version.as_deref() == Some("1.20.1")
                    && d.game_mode.as_deref() == Some("创造")
                    && d.hardcore == Some(true)
                    && d.info_note.is_none())
                .unwrap_or(false)
        );
        check!(
            "world detail rejects unknown id",
            worlds::detail(&game.join("saves"), "w_00000000")
                .err()
                .map(|e| e.starts_with("WORLD_NOT_FOUND|"))
                .unwrap_or(false)
        );
    }

    let st = stats::stats("7d", &|id| id.to_string());
    check!(
        "stats honest nulls",
        st.deaths.is_none() && st.achievements.is_none() && st.range == "7d"
    );

    // 加载器清单解析（纯函数，不依赖网络）
    let sample = r#"[{"loader":{"version":"0.16.9","stable":true}},{"loader":{"version":"0.16.8","stable":false}}]"#;
    let parsed = loader::parse_fabric(sample);
    check!(
        "loader parse",
        parsed.len() == 2 && parsed[0].0 == "0.16.9" && parsed[0].1 && !parsed[1].1
    );
    check!(
        "loader empty safe",
        loader::parse_fabric("[]").is_empty() && loader::parse_fabric("oops").is_empty()
    );

    // 下载安全：来源白名单与路径边界
    check!(
        "url allow",
        net::check_url("https://cdn.modrinth.com/data/x/y.jar").is_ok()
            && net::check_url("https://bmclapi2.bangbang93.com/maven/a.jar").is_ok()
    );
    check!(
        "url deny",
        net::check_url("http://cdn.modrinth.com/a.jar").is_err()
            && net::check_url("https://evil.example.com/a.jar").is_err()
            && net::check_url("https://127.0.0.1/a.jar").is_err()
            && net::check_url("https://user@cdn.modrinth.com/a.jar").is_err()
            && net::check_url("file:///C:/a.jar").is_err()
    );
    let net_root = std::env::temp_dir().join(format!("astral-net-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&net_root);
    let _ = std::fs::create_dir_all(&net_root);
    check!(
        "path within",
        net::ensure_within(&net_root, &net_root.join("mods/a.jar")).is_ok()
    );
    check!(
        "path escape",
        net::ensure_within(&net_root, &net_root.join("..").join("escaped.jar")).is_err()
    );
    let _ = std::fs::remove_dir_all(&net_root);

    // 世界 ID：稳定且能反解
    let saves_dir = std::env::temp_dir().join(format!("astral-worlds-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&saves_dir);
    let _ = std::fs::create_dir_all(saves_dir.join("World1"));
    let _ = std::fs::write(saves_dir.join("World1/level.dat"), b"x");
    let ws = worlds::list(&saves_dir);
    check!(
        "worlds listed",
        ws.len() == 1 && ws[0].display_name == "World1"
    );
    check!(
        "world id stable",
        !ws.is_empty() && worlds::world_id("World1") == ws[0].world_id
    );
    check!(
        "world resolve",
        !ws.is_empty() && worlds::resolve(&saves_dir, &ws[0].world_id).is_some()
    );
    check!(
        "world resolve rejects unknown",
        worlds::resolve(&saves_dir, "w_00000000").is_none()
    );
    let _ = std::fs::remove_dir_all(&saves_dir);

    // 离线身份：用户名校验 + 旧 UUID 兼容检测
    check!(
        "username valid",
        account::validate_username("Steve").is_ok() && account::validate_username("a_b1").is_ok()
    );
    check!(
        "username invalid",
        account::validate_username("ab").is_err()
            && account::validate_username("玩家").is_err()
            && account::validate_username("bad name").is_err()
    );
    let id_dir = std::env::temp_dir().join(format!("astral-identity-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&id_dir);
    check!(
        "no legacy identity on empty",
        !account::has_legacy_identity(&id_dir, "Steve")
    );
    let pd = id_dir.join("World1").join("playerdata");
    let _ = std::fs::create_dir_all(&pd);
    let legacy_file = format!("{}.dat", account::legacy_uuid("Steve").hyphenated());
    let _ = std::fs::write(pd.join(&legacy_file), b"x");
    check!(
        "legacy identity detected",
        account::has_legacy_identity(&id_dir, "Steve")
    );
    let _ = std::fs::remove_dir_all(&id_dir);

    // 安装计划：校验 → 一次性取走 → 改写/过期拒绝
    let plan_game = std::env::temp_dir().join(format!("astral-plan-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&plan_game);
    let _ = std::fs::create_dir_all(plan_game.join("versions/1.20.1"));
    let _ = std::fs::write(plan_game.join("versions/1.20.1/1.20.1.json"), "{}");
    let _ = std::fs::write(plan_game.join("versions/1.20.1/1.20.1.jar"), "x");

    let req = plan::GameInstallRequest {
        name: "计划测试".into(),
        game_version: "1.20.1".into(),
        loader: None,
        memory_mb: Some(2048),
        shader_support: None,
        shader_support_version: None,
    };
    let p = plan::prepare(&plan_game, req.clone()).expect("prepare");
    check!(
        "plan prepared",
        p.plan_id.starts_with("plan_") && p.issues.iter().all(|i| !i.blocking)
    );
    check!("plan reuses installed", p.files.is_empty());
    check!("plan take", plan::take(&p.plan_id, 1).is_ok());
    check!("plan single use", plan::take(&p.plan_id, 1).is_err());

    let mut bad_name = req.clone();
    bad_name.name = "bad/name".into();
    let p_bad = plan::prepare(&plan_game, bad_name).expect("prepare bad");
    check!(
        "plan rejects bad name",
        p_bad
            .issues
            .iter()
            .any(|i| i.blocking && i.code == "INVALID_NAME")
    );

    let mut forge_req = req.clone();
    forge_req.loader = Some(plan::LoaderSelection {
        kind: "forge".into(),
        version: "47.2.0".into(),
    });
    let p_forge = plan::prepare(&plan_game, forge_req).expect("prepare forge");
    check!(
        "plan accepts forge",
        p_forge.issues.iter().all(|i| !i.blocking)
    );

    check!(
        "installer url forge",
        core::installer_url("forge", "1.20.1", "47.2.0")
            .map(|u| u.ends_with("forge-1.20.1-47.2.0-installer.jar"))
            .unwrap_or(false)
    );
    check!(
        "installer url neoforge",
        core::installer_url("neoforge", "1.21.1", "21.1.72")
            .map(|u| u.ends_with("neoforge-21.1.72-installer.jar"))
            .unwrap_or(false)
    );
    check!(
        "installer url neoforge 1.20.1",
        core::installer_url("neoforge", "1.20.1", "47.1.106")
            .map(|u| u.contains("net/neoforged/forge/1.20.1-47.1.106/"))
            .unwrap_or(false)
    );

    let p_rev = plan::prepare(&plan_game, req.clone()).expect("prepare rev");
    check!(
        "plan revision guard",
        plan::take(&p_rev.plan_id, 2).is_err()
    );
    let _ = std::fs::remove_dir_all(&plan_game);

    // 可选的真实安装验证：ASTRAL_LIVE_FORGE="mc|ver|dir" 或 "kind|mc|ver|dir"
    if let Ok(spec) = std::env::var("ASTRAL_LIVE_FORGE") {
        let raw: Vec<&str> = spec.split('|').collect();
        let parts: Vec<&str> = if raw.len() == 4 {
            raw[1..].to_vec()
        } else {
            raw.clone()
        };
        let kind = if raw.len() == 4 { raw[0] } else { "forge" };
        if parts.len() == 3 {
            let dir = std::path::PathBuf::from(parts[2]);
            log.push_str(&format!("LIVE {kind} start: {spec}\n"));
            let mut progress_lines = 0;
            let r = core::install_forge_like(kind, parts[0], parts[1], &dir, |p| {
                progress_lines += 1;
                if progress_lines <= 5 {
                    log.push_str(&format!("  {kind}: {} {}\n", p.stage, p.label));
                }
            });
            match r {
                Ok(id) => log.push_str(&format!("LIVE {kind} ok: {id}\n")),
                Err(e) => {
                    fails += 1;
                    log.push_str(&format!("LIVE {kind} failed: {e}\n"));
                }
            }
        }
    }

    // 可选的真实启动验证：ASTRAL_LIVE_LAUNCH="dir|version"
    if let Ok(spec) = std::env::var("ASTRAL_LIVE_LAUNCH") {
        let parts: Vec<&str> = spec.split('|').collect();
        if parts.len() == 2 || parts.len() == 4 {
            let dir = std::path::PathBuf::from(parts[0]);
            // 2 段：dir|version；4 段：dir|version|loaderKind|loaderVersion
            let loader = if parts.len() == 4 {
                Some((parts[2].to_string(), parts[3].to_string()))
            } else {
                None
            };
            log.push_str(&format!("LIVE launch start: {spec}\n"));
            // 与启动命令走同一个自动挑 Java 的函数
            let picked_java = core::auto_java_for(&dir, parts[1]);
            match &picked_java {
                Some(p) => log.push_str(&format!("LIVE launch java={}\n", p.display())),
                None => log.push_str("LIVE launch java=default\n"),
            }
            let r = core::spawn_with(
                parts[1],
                loader.as_ref().map(|(k, v)| (k.as_str(), v.as_str())),
                &dir,
                Some(&dir),
                "Steve",
                Some(2048),
                &[],
                picked_java.as_deref(),
                |_p| {},
            );
            match r {
                Ok(mut child) => {
                    let pid = child.id();
                    let mut alive_ticks = 0;
                    for _ in 0..20 {
                        std::thread::sleep(std::time::Duration::from_secs(3));
                        match child.try_wait() {
                            Ok(Some(status)) => {
                                log.push_str(&format!(
                                    "LIVE launch exited early: code={:?}\n",
                                    status.code()
                                ));
                                fails += 1;
                                break;
                            }
                            _ => alive_ticks += 1,
                        }
                    }
                    if alive_ticks >= 20 {
                        log.push_str(&format!(
                            "LIVE launch ok: pid {pid} still running after 60s\n"
                        ));
                        let _ = child.kill();
                    }
                }
                Err(e) => {
                    fails += 1;
                    log.push_str(&format!("LIVE launch failed: {e}\n"));
                }
            }
        }
    }

    // 可选的真实资源安装验证：ASTRAL_LIVE_RESOURCE="gameDir"
    if let Ok(dir) = std::env::var("ASTRAL_LIVE_RESOURCE") {
        let mut spec = dir.split('|');
        let game_dir = spec.next().unwrap_or("").to_string();
        let project_id = spec.next().unwrap_or("AANobbMI").to_string();
        let game = std::path::PathBuf::from(&game_dir);
        log.push_str(&format!("LIVE resource start: {dir}\n"));
        let meta = instance::InstanceMeta {
            id: instance::new_id(),
            name: "resource-test".into(),
            game_version: "1.20.1".into(),
            loader_kind: Some("fabric".into()),
            loader_version: Some("0.16.9".into()),
            launch_version: None,
            memory_mb: None,
            jvm_args: vec![],
            java_path: None,
            cover: None,
            created_at: time::now_iso(),
        };
        let run = || -> Result<String, String> {
            let inst = instance::create(&game, &meta)?;
            let files = resources::files(&project_id, Some("1.20.1"), Some("fabric"))?;
            let first = files
                .first()
                .ok_or_else(|| "VERSION_NOT_FOUND|没有兼容文件".to_string())?;
            let req = resources::ResourceInstallRequest {
                instance_id: inst.id.clone(),
                kind: "mod".into(),
                project_id: project_id.clone(),
                version_id: first.version_id.clone(),
                world_id: None,
                overwrite: false,
            };
            let rp = resources::prepare(&game, req, &play_stats)?;
            if let Some(b) = rp.issues.iter().find(|i| i.blocking) {
                return Err(format!("{}|{}", b.code, b.message));
            }
            let stored = resources::take(&rp.plan_id, 1)?;
            let p = resources::execute(&stored, &*net::never_cancel(), |_, _| {})?;
            Ok(p.to_string_lossy().to_string())
        };
        match run() {
            Ok(p) => log.push_str(&format!("LIVE resource ok: {p}\n")),
            Err(e) => {
                fails += 1;
                log.push_str(&format!("LIVE resource failed: {e}\n"));
            }
        }
    }

    // 日志与诊断预览
    let log_dir = std::env::temp_dir().join(format!("astral-logs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&log_dir);
    // D2：结构化诊断（本地规则、可解释、不猜测）
    {
        let d = game.join("diag-game");
        let _ = std::fs::create_dir_all(d.join("logs"));
        let _ = std::fs::write(
            d.join("logs").join("latest.log"),
            "[12:00:00] [main/ERROR]: java.lang.UnsupportedClassVersionError: bad\n\
             [12:00:01] [main/ERROR]: java.lang.OutOfMemoryError: Java heap space\n\
             [12:00:02] [main/WARN]: java.io.IOException: Access is denied\n",
        );
        let r = logs::diagnose(&d, None, 200).expect("diagnose");
        check!(
            "diagnosis report is structured",
            r.worst == "error"
                && r.local_only
                && r.summary
                    .as_deref()
                    .map(|s| s.contains("可疑"))
                    .unwrap_or(false)
                && !r.next_steps.is_empty()
                && r.next_steps.len() <= 4
                && r.basis.contains("本地规则")
                && r.findings
                    .iter()
                    .all(|f| !f.cause.is_empty()
                        && matches!(f.confidence, "high" | "medium" | "low"))
                && r.findings
                    .iter()
                    .any(|f| f.id == "java-version" && f.category == "java")
        );
        let _ = std::fs::write(
            d.join("logs").join("latest.log"),
            "[12:00:00] [main/INFO]: Loading Minecraft 1.20.1\n",
        );
        let clean = logs::diagnose(&d, None, 200).expect("diagnose clean");
        check!(
            "diagnosis stays quiet on clean log",
            clean.findings.is_empty()
                && clean.worst == "none"
                && clean.summary.is_none()
                && clean.next_steps.is_empty()
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    let _ = std::fs::create_dir_all(log_dir.join("logs"));
    let _ = std::fs::create_dir_all(log_dir.join("crash-reports"));
    let _ = std::fs::write(
        log_dir.join("logs/latest.log"),
        "line1\nline2\nAuthorization: Bearer abc123\ncontact steve@example.com\nfrom 192.168.1.20\n",
    );
    let _ = std::fs::write(log_dir.join("crash-reports/crash-1.txt"), "boom\n");
    let logs_list = logs::list(&log_dir);
    check!("logs listed", logs_list.len() == 2);
    let latest = logs_list.iter().find(|l| l.name == "latest.log").cloned();
    check!(
        "log id stable",
        latest
            .as_ref()
            .map(|l| l.id.starts_with("log_") && l.kind == "game")
            .unwrap_or(false)
    );
    let tail = logs::read(&log_dir, &latest.as_ref().unwrap().id, Some(2)).expect("read log");
    check!("log tail limit", tail.lines == 2 && tail.truncated);
    check!(
        "log unknown id rejected",
        logs::read(&log_dir, "log_00000000", None).is_err()
    );
    let preview = logs::prepare(&log_dir, &[latest.unwrap().id], 50).expect("preview");
    check!(
        "diagnosis redacted",
        preview.redactions >= 3
            && preview.text.contains("<email>")
            && preview.text.contains("<ip>")
            && preview.text.contains("<redacted>")
    );
    // 实时跟随：只给完整行、按 offset 续读、日志被重写时自愈
    {
        let live_id = logs::list(&log_dir)
            .into_iter()
            .find(|l| l.name == "latest.log")
            .map(|l| l.id)
            .unwrap();
        let c1 = live::tail(&log_dir, &live_id, 0, None).expect("tail 1");
        check!(
            "live log returns complete lines",
            c1.lines == 5 && c1.eof && !c1.reset && c1.text.contains("line2")
        );
        // 没新内容时不该重复给旧行
        let c2 = live::tail(&log_dir, &live_id, c1.next_offset, None).expect("tail 2");
        check!(
            "live log does not repeat",
            c2.text.is_empty() && c2.next_offset == c1.next_offset
        );
        // 追加一行，只给新的那行
        {
            use std::io::Write as _;
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(log_dir.join("logs/latest.log"))
                .unwrap();
            f.write_all(b"line6\n").unwrap();
        }
        let c3 = live::tail(&log_dir, &live_id, c2.next_offset, None).expect("tail 3");
        check!(
            "live log appends incrementally",
            c3.text == "line6\n" && c3.eof
        );
        // 日志被重写（比 offset 短）→ reset 并从头给
        let _ = std::fs::write(log_dir.join("logs/latest.log"), "fresh\n");
        let c4 = live::tail(&log_dir, &live_id, c3.next_offset, None).expect("tail 4");
        check!(
            "live log resets after rewrite",
            c4.reset && c4.from_offset == 0 && c4.text == "fresh\n"
        );
        // 只有半行时不返回，等补齐
        let _ = std::fs::write(log_dir.join("logs/latest.log"), "whole\npart");
        let c5 = live::tail(&log_dir, &live_id, 0, None).expect("tail 5");
        check!(
            "live log holds back partial line",
            c5.text == "whole\n" && !c5.eof
        );
    }
    let _ = std::fs::remove_dir_all(&log_dir);

    // 可选的真实整合包安装：ASTRAL_LIVE_MODPACK="gameDir|projectId"
    if let Ok(spec) = std::env::var("ASTRAL_LIVE_MODPACK") {
        let parts: Vec<&str> = spec.split('|').collect();
        if parts.len() == 2 {
            let game = std::path::PathBuf::from(parts[0]);
            let pid = parts[1].to_string();
            log.push_str(&format!("LIVE modpack start: {spec}\n"));
            let r = (|| -> Result<String, String> {
                let vers = crate::net::fetch_json(&format!(
                    "https://api.modrinth.com/v2/project/{pid}/version"
                ))?;
                let arr = vers
                    .as_array()
                    .ok_or_else(|| "INTERNAL|版本列表格式异常".to_string())?;
                let pick = arr
                    .iter()
                    .find(|v| {
                        v.get("game_versions")
                            .and_then(|g| g.as_array())
                            .map(|a| a.iter().any(|x| x.as_str() == Some("1.20.1")))
                            .unwrap_or(false)
                    })
                    .or_else(|| arr.first())
                    .ok_or_else(|| "VERSION_NOT_FOUND|没有版本".to_string())?;
                let vid = pick
                    .get("id")
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| "INTERNAL|版本缺少 id".to_string())?
                    .to_string();
                let plan = modpack::prepare(
                    &game,
                    modpack::ModpackRequest {
                        project_id: pid.clone(),
                        version_id: vid,
                        name: None,
                        memory_mb: None,
                    },
                )?;
                if let Some(b) = plan.issues.iter().find(|i| i.blocking) {
                    return Err(format!("{}|{}", b.code, b.message));
                }
                let stored = modpack::take(&plan.plan_id, 1)?;
                modpack::install(&stored, &game, "Steve", || false, |_s, _l, _d, _t| {})
            })();
            match r {
                Ok(id) => log.push_str(&format!("LIVE modpack ok: {id}\n")),
                Err(e) => {
                    fails += 1;
                    log.push_str(&format!("LIVE modpack failed: {e}\n"));
                }
            }
        }
    }

    // Java 探测的纯逻辑
    check!(
        "java major parse",
        java::parse_major("1.8.0_402") == Some(8)
            && java::parse_major("17.0.9") == Some(17)
            && java::parse_major("25.0.1+8-LTS") == Some(25)
            && java::parse_major("garbage").is_none()
    );
    let mk_java = |m: u32| java::JavaRuntime {
        path: format!("j{m}"),
        version: m.to_string(),
        major: m,
        vendor: "t".into(),
        bits: Some(64),
        source: "t".into(),
    };
    let rts = vec![mk_java(25), mk_java(21), mk_java(17)];
    check!(
        "java pick",
        java::pick_for(&rts, 17).map(|r| r.major) == Some(17)
            && java::pick_for(&rts, 18).map(|r| r.major) == Some(21)
            && java::pick_for(&rts, 30).is_none()
    );

    // C4：Java 需求推算与兼容判断（不联网、不依赖本机 Java）
    check!(
        "java expected major table",
        java_audit::expected_major("1.12.2") == Some(8)
            && java_audit::expected_major("1.16.5") == Some(8)
            && java_audit::expected_major("1.17.1") == Some(17)
            && java_audit::expected_major("1.20.4") == Some(17)
            && java_audit::expected_major("1.20.5") == Some(21)
            && java_audit::expected_major("1.21.4") == Some(21)
            && java_audit::expected_major("1.20.1-forge-47.2.0") == Some(17)
            && java_audit::expected_major("garbage").is_none()
    );
    check!(
        "java satisfies rules",
        java_audit::satisfies(8, 8)
            && !java_audit::satisfies(17, 8)
            && java_audit::satisfies(21, 17)
            && !java_audit::satisfies(17, 21)
    );
    {
        // 造一个实例：记录的 Java 路径不存在 → 必须报 problem，且建议自动选择
        let g = game.join("java-audit-game");
        let _ = std::fs::create_dir_all(&g);
        let meta = instance::InstanceMeta {
            id: instance::new_id(),
            name: "Java 审计实例".into(),
            game_version: "1.20.1".into(),
            loader_kind: None,
            loader_version: None,
            launch_version: None,
            memory_mb: Some(2048),
            jvm_args: Vec::new(),
            java_path: Some("D:\\nonexistent\\java.exe".into()),
            cover: None,
            created_at: time::now_iso(),
        };
        let made = instance::create(&g, &meta)
            .map(|i| i.id)
            .unwrap_or_default();
        let stats_fn = |_: &str| instance::PlayStats::default();
        let one = java_audit::check_instance(&g, &made, &stats_fn);
        check!(
            "java audit flags missing path",
            one.as_ref()
                .map(|c| !c.exists && c.problem.is_some() && c.suggestion.is_some())
                .unwrap_or(false)
                && one
                    .as_ref()
                    .map(|c| c.source == "instance")
                    .unwrap_or(false)
        );
        let full = java_audit::audit(&g, &stats_fn);
        check!(
            "java audit report shape",
            full.instances.len() == 1
                && full.broken.len() == 1
                && full.broken[0] == made
                && full.checked_at.len() > 10
        );
        // 删除托管 JDK：没确认必须拒绝；不存在的版本必须拒绝
        check!(
            "java remove requires confirm",
            java_audit::remove_managed(&g, 17, false).is_err()
                && java_audit::remove_managed(&g, 17, true).is_err()
        );
        // 缓存：空目录也要能报出结构
        let cs = java_audit::cache_stats(&g);
        check!(
            "java cache stats shape",
            cs.get("count").and_then(|x| x.as_u64()) == Some(0)
                && cs.get("bytes").and_then(|x| x.as_u64()) == Some(0)
                && cs.get("dir").and_then(|x| x.as_str()).is_some()
        );
        check!(
            "java cache clear requires confirm",
            java_audit::clear_cache(&g, false).is_err()
                && java_audit::clear_cache(&g, true).is_ok()
        );
        // 托管 JDK 目录：造一个假的 adoptium-21，删它必须成功且只删它
        {
            let jvm = g.join("jvm").join("adoptium-21");
            let _ = std::fs::create_dir_all(jvm.join("bin"));
            let _ = std::fs::write(jvm.join("bin").join("java.exe"), b"FAKE");
            let keep = g.join("jvm").join("adoptium-17");
            let _ = std::fs::create_dir_all(keep.join("bin"));
            let _ = std::fs::write(keep.join("bin").join("java.exe"), b"FAKE");
            let removed = java_audit::remove_managed(&g, 21, true);
            check!(
                "java remove managed deletes only target",
                removed.is_ok() && !jvm.exists() && keep.is_dir()
            );
        }
    }

    // 可选的真实 Java 探测：ASTRAL_LIVE_JAVA="gameDir"
    if let Ok(dir) = std::env::var("ASTRAL_LIVE_JAVA") {
        let game = std::path::PathBuf::from(&dir);
        let rts = java::scan(&game);
        log.push_str(&format!("LIVE java found {} runtimes\n", rts.len()));
        for r in rts.iter().take(5) {
            log.push_str(&format!(
                "  java {} (major {}) {} {}\n",
                r.version, r.major, r.vendor, r.path
            ));
        }
        if rts.is_empty() {
            fails += 1;
            log.push_str("LIVE java failed: 没有找到任何 Java\n");
        }
    }

    // 光影组件匹配逻辑
    check!(
        "component loader match",
        components::loader_allows(&["fabric", "quilt"], Some("fabric"))
            && components::loader_allows(&["fabric", "quilt"], Some("QUILT"))
            && !components::loader_allows(&["fabric"], Some("forge"))
            && !components::loader_allows(&["fabric"], None)
    );

    // 可选的真实组件目录：ASTRAL_LIVE_COMPONENTS="1.20.1|fabric"
    if let Ok(spec) = std::env::var("ASTRAL_LIVE_COMPONENTS") {
        let parts: Vec<&str> = spec.split('|').collect();
        if parts.len() == 2 {
            let cat = components::catalog(parts[0], Some(parts[1]));
            for c in &cat.shader_support {
                log.push_str(&format!(
                    "  component {} available={} version={:?} reason={:?}\n",
                    c.id, c.available, c.version, c.unavailable_reason
                ));
            }
            let iris = cat.shader_support.iter().find(|c| c.id == "iris");
            if iris.map(|c| c.available).unwrap_or(false) {
                log.push_str("LIVE components ok\n");
            } else {
                fails += 1;
                log.push_str("LIVE components failed: iris 不可用\n");
            }
        }
    }

    // 账号：客户端 ID 校验与离线档案
    check!(
        "msa client id shape",
        account::validate_client_id("12345678-1234-1234-1234-123456789abc").is_ok()
            && account::validate_client_id("not-a-guid").is_err()
    );
    let acct = get_account();
    check!(
        "account is offline",
        acct.kind == "offline" && !acct.online && !acct.name.is_empty()
    );
    let msa = msa_status();
    check!("msa status honest", msa.configured == msa.reason.is_none());

    // 可选的真实多类型资源安装：ASTRAL_LIVE_RESKINDS="gameDir"
    if let Ok(dir) = std::env::var("ASTRAL_LIVE_RESKINDS") {
        let game = std::path::PathBuf::from(&dir);
        log.push_str(&format!("LIVE reskinds start: {dir}\n"));
        let run = || -> Result<Vec<String>, String> {
            let meta = instance::InstanceMeta {
                id: instance::new_id(),
                name: "reskinds-test".into(),
                game_version: "1.20.1".into(),
                loader_kind: Some("fabric".into()),
                loader_version: Some("0.16.9".into()),
                launch_version: None,
                memory_mb: None,
                jvm_args: vec![],
                java_path: None,
                cover: None,
                created_at: time::now_iso(),
            };
            let inst = instance::create(&game, &meta)?;
            let saves = std::path::Path::new(&inst.dirs.saves);
            std::fs::create_dir_all(saves.join("TestWorld")).map_err(|e| e.to_string())?;
            std::fs::write(saves.join("TestWorld/level.dat"), b"x").map_err(|e| e.to_string())?;
            let world = worlds::list(saves)
                .into_iter()
                .next()
                .ok_or_else(|| "INTERNAL|测试存档没被识别".to_string())?;

            let mut out = Vec::new();
            for (kind, pid, wid) in [
                ("resourcepack", "w0TnApzs", None),
                ("shaderpack", "HVnmMxH1", None),
                ("datapack", "8oi3bsk5", Some(world.world_id.as_str())),
            ] {
                match resources::live_probe(
                    &game,
                    &inst.id,
                    kind,
                    pid,
                    wid,
                    "1.20.1",
                    Some("fabric"),
                ) {
                    Ok(p) => out.push(format!("{kind} ok: {p}")),
                    Err(e) => out.push(format!("{kind} failed: {e}")),
                }
            }
            Ok(out)
        };
        match run() {
            Ok(lines) => {
                for l in &lines {
                    log.push_str(&format!("  LIVE {l}\n"));
                    if l.contains("failed") {
                        fails += 1;
                    }
                }
            }
            Err(e) => {
                fails += 1;
                log.push_str(&format!("LIVE reskinds failed: {e}\n"));
            }
        }
        log.push_str("LIVE reskinds done\n");
    }

    // 可选的真实更新检查：ASTRAL_LIVE_UPDATES="gameDir|instanceId"
    if let Ok(spec) = std::env::var("ASTRAL_LIVE_UPDATES") {
        let parts: Vec<&str> = spec.split('|').collect();
        if parts.len() == 2 {
            let game = std::path::PathBuf::from(parts[0]);
            if let Some(inst) = instance::find(&game, parts[1], &play_stats) {
                let loader = inst.loader.as_ref().map(|l| l.kind.clone());
                let rep = updates::check(
                    std::path::Path::new(&inst.dirs.mods),
                    &inst.game_version,
                    loader.as_deref(),
                    Some(20),
                );
                log.push_str(&format!(
                    "LIVE updates checked={} unknown={} updates={} skipped={}\n",
                    rep.checked,
                    rep.unknown,
                    rep.updates.len(),
                    rep.skipped
                ));
                for u in rep.updates.iter().take(5) {
                    log.push_str(&format!(
                        "  update {} {} -> {}\n",
                        u.file_name, u.current_version, u.latest_version
                    ));
                }
                if rep.checked == 0 {
                    fails += 1;
                    log.push_str("LIVE updates failed: 没有识别出任何 Modrinth 文件\n");
                }
            } else {
                fails += 1;
                log.push_str("LIVE updates failed: 实例不存在\n");
            }
        }
    }

    // 可选的真实更新执行：ASTRAL_LIVE_UPDATE_APPLY="gameDir|instanceId"
    if let Ok(spec) = std::env::var("ASTRAL_LIVE_UPDATE_APPLY") {
        let parts: Vec<&str> = spec.split('|').collect();
        if parts.len() == 2 {
            let game = std::path::PathBuf::from(parts[0]);
            match instance::find(&game, parts[1], &play_stats) {
                Some(inst) => {
                    let loader = inst.loader.as_ref().map(|l| l.kind.clone());
                    let rep = updates::check(
                        std::path::Path::new(&inst.dirs.mods),
                        &inst.game_version,
                        loader.as_deref(),
                        Some(5),
                    );
                    if let Some(u) = rep.updates.first() {
                        log.push_str(&format!(
                            "LIVE update-apply start: {} -> {}\n",
                            u.file_name, u.latest_version
                        ));
                        match updates::apply_update(
                            &game,
                            parts[1],
                            &u.file_name,
                            &u.project_id,
                            &u.latest_version_id,
                            &*net::never_cancel(),
                            |_, _, _, _| {},
                        ) {
                            Ok((new, removed)) => {
                                log.push_str(&format!("LIVE update-apply ok: {removed} -> {new}\n"))
                            }
                            Err(e) => {
                                fails += 1;
                                log.push_str(&format!("LIVE update-apply failed: {e}\n"));
                            }
                        }
                    } else {
                        log.push_str("LIVE update-apply skip: 没有可更新的模组\n");
                    }
                }
                None => {
                    fails += 1;
                    log.push_str("LIVE update-apply failed: 实例不存在\n");
                }
            }
        }
    }

    // 可选的真实导出：ASTRAL_LIVE_EXPORT="gameDir|instanceId"
    if let Ok(spec) = std::env::var("ASTRAL_LIVE_EXPORT") {
        let parts: Vec<&str> = spec.split('|').collect();
        if parts.len() == 2 {
            let game = std::path::PathBuf::from(parts[0]);
            let req = export::ExportRequest {
                instance_id: parts[1].to_string(),
                name: "Astral Export Test".into(),
                version: Some("1.0.0".into()),
                include_config: false,
            };
            match export::export(&game, &req, &play_stats) {
                Ok(r) => {
                    log.push_str(&format!(
                        "LIVE export ok: {} (indexed={} overrides={} bytes={})\n",
                        r.path, r.indexed_files, r.override_files, r.bytes
                    ));
                    match modpack::read_index(std::path::Path::new(&r.path)) {
                        Ok(idx) => log.push_str(&format!(
                            "LIVE export roundtrip: mc={} files={} loader={:?}\n",
                            idx.mc_version,
                            idx.files.len(),
                            idx.loader_kind
                        )),
                        Err(e) => {
                            fails += 1;
                            log.push_str(&format!("LIVE export roundtrip failed: {e}\n"));
                        }
                    }
                }
                Err(e) => {
                    fails += 1;
                    log.push_str(&format!("LIVE export failed: {e}\n"));
                }
            }
        }
    }

    // 任务去重：同 kind + 同版本视为重复
    let dup = task::create("dedupe-test", None, Some("9.9.9".into()), "去重测试");
    check!(
        "task dedupe detects",
        task::has_running_kind("dedupe-test", None, Some("9.9.9"))
    );
    check!(
        "task dedupe ignores other version",
        !task::has_running_kind("dedupe-test", None, Some("8.8.8"))
    );
    task::cancelled(&dup.task_id);
    check!(
        "task dedupe ignores terminal",
        !task::has_running_kind("dedupe-test", None, Some("9.9.9"))
    );

    // 重试：只有终态 + 有参数 + 已知类型才允许
    let rt = task::create("retry-demo", None, None, "重试演示");
    check!("retry rejects running", retry_spec_of(&rt.task_id).is_err());
    task::fail(&rt.task_id, "NETWORK|演示失败".into());
    check!(
        "retry rejects missing spec",
        retry_spec_of(&rt.task_id).is_err()
    );
    task::set_retry_spec(&rt.task_id, serde_json::json!({"kind":"unknown-kind"}));
    check!(
        "retry rejects unknown kind",
        retry_task(rt.task_id.clone()).is_err()
    );
    let ok_spec = task::create("retry-demo-2", None, None, "重试演示2");
    task::fail(&ok_spec.task_id, "NETWORK|x".into());
    task::set_retry_spec(
        &ok_spec.task_id,
        serde_json::json!({"kind":"install","gameVersion":"1.2.3"}),
    );
    check!(
        "retry accepts known kind",
        retry_spec_of(&ok_spec.task_id)
            .map(|(k, _)| k == "install")
            .unwrap_or(false)
    );

    // 可选的真实 JDK 下载：ASTRAL_LIVE_JDK="major|gameDir"
    if let Ok(spec) = std::env::var("ASTRAL_LIVE_JDK") {
        let parts: Vec<&str> = spec.split('|').collect();
        if parts.len() == 2 {
            let major: u32 = parts[0].parse().unwrap_or(21);
            let game = std::path::PathBuf::from(parts[1]);
            log.push_str(&format!("LIVE jdk start: java {major}\n"));
            match jdk::install(major, &game, || false, |_s, _d, _t| {}) {
                Ok(r) => {
                    log.push_str(&format!(
                        "LIVE jdk ok: version={} path={} bytes={}\n",
                        r.version, r.path, r.bytes
                    ));
                    match std::process::Command::new(&r.path).arg("-version").output() {
                        Ok(o) => {
                            let txt = String::from_utf8_lossy(&o.stderr).to_string();
                            log.push_str(&format!(
                                "LIVE jdk runs: {}\n",
                                txt.lines().next().unwrap_or("")
                            ));
                        }
                        Err(e) => {
                            fails += 1;
                            log.push_str(&format!("LIVE jdk runs failed: {e}\n"));
                        }
                    }
                }
                Err(e) => {
                    fails += 1;
                    log.push_str(&format!("LIVE jdk failed: {e}\n"));
                }
            }
        }
    }

    // 启动阶段映射：内部 stage → 稳定 phase
    check!(
        "phase mapping",
        task::phase_of("validate") == "validating"
            && task::phase_of("resolve") == "checking-files"
            && task::phase_of("jvm") == "preparing-java"
            && task::phase_of("download") == "downloading"
            && task::phase_of("spawned") == "process-created"
            && task::phase_of("error") == "failed"
    );
    let pt = task::create("phase-demo", None, None, "阶段演示");
    task::progress(&pt.task_id, "jvm", "准备 Java", 1, 2);
    check!(
        "phase set on progress",
        task::get(&pt.task_id)
            .map(|x| x.phase == "preparing-java")
            .unwrap_or(false)
    );
    task::finish(&pt.task_id, serde_json::json!({"ok": true}));
    check!(
        "phase set on finish",
        task::get(&pt.task_id)
            .map(|x| x.phase == "done")
            .unwrap_or(false)
    );

    // 健康检查：跟着 inheritsFrom 看 jar / 依赖库 / 资源索引
    let hgame = std::env::temp_dir().join(format!("astral-health-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&hgame);
    let _ = std::fs::create_dir_all(hgame.join("versions/1.20.1"));
    let _ = std::fs::create_dir_all(hgame.join("versions/1.20.1-forge-47.2.0"));
    let parent = r#"{"assetIndex":{"id":"5"},"libraries":[{"downloads":{"artifact":{"path":"org/ow2/asm/asm/9.5/asm-9.5.jar"}}}]}"#;
    let child = r#"{"inheritsFrom":"1.20.1","libraries":[{"downloads":{"artifact":{"path":"net/forge/fmlloader/1.0/fmlloader-1.0.jar"}}}]}"#;
    let _ = std::fs::write(hgame.join("versions/1.20.1/1.20.1.json"), parent);
    let _ = std::fs::write(
        hgame.join("versions/1.20.1-forge-47.2.0/1.20.1-forge-47.2.0.json"),
        child,
    );
    let broken = instance::health_for_test(&hgame, "1.20.1-forge-47.2.0");
    check!(
        "health reports missing",
        !broken.ok
            && broken.reasons.iter().any(|r| r.contains("依赖库"))
            && broken.reasons.iter().any(|r| r.contains("资源索引"))
    );
    // 补齐 jar / 库 / 索引后应当健康
    let _ = std::fs::create_dir_all(hgame.join("versions/1.20.1"));
    let _ = std::fs::write(hgame.join("versions/1.20.1/1.20.1.jar"), b"x");
    let _ = std::fs::create_dir_all(hgame.join("libraries/org/ow2/asm/asm/9.5"));
    let _ = std::fs::write(
        hgame.join("libraries/org/ow2/asm/asm/9.5/asm-9.5.jar"),
        b"x",
    );
    let _ = std::fs::create_dir_all(hgame.join("libraries/net/forge/fmlloader/1.0"));
    let _ = std::fs::write(
        hgame.join("libraries/net/forge/fmlloader/1.0/fmlloader-1.0.jar"),
        b"x",
    );
    let _ = std::fs::create_dir_all(hgame.join("assets/indexes"));
    let _ = std::fs::write(hgame.join("assets/indexes/5.json"), b"{}");
    let healthy = instance::health_for_test(&hgame, "1.20.1-forge-47.2.0");
    check!(
        "health ok after fill",
        healthy.ok && healthy.reasons.is_empty()
    );
    let _ = std::fs::remove_dir_all(&hgame);

    // 可选的真实健康检查：ASTRAL_LIVE_HEALTH="gameDir"
    if let Ok(dir) = std::env::var("ASTRAL_LIVE_HEALTH") {
        let game = std::path::PathBuf::from(&dir);
        let list = instance::discover(&game, &play_stats);
        log.push_str(&format!("LIVE health instances={}\n", list.len()));
        for i in list.iter().take(6) {
            log.push_str(&format!(
                "  {} ok={} reasons={:?}\n",
                i.name, i.health.ok, i.health.reasons
            ));
        }
        if list.is_empty() {
            fails += 1;
            log.push_str("LIVE health failed: 没发现实例\n");
        }
    }

    // 平台规则：macOS 专用库不应在 Windows 上被判为缺失
    let osx_only = serde_json::json!({"rules":[{"action":"allow","os":{"name":"osx"}}]});
    let win_only = serde_json::json!({"rules":[{"action":"allow","os":{"name":"windows"}}]});
    let generic = serde_json::json!({});
    check!(
        "platform rules",
        !instance::lib_allowed_for_test(&osx_only)
            && instance::lib_allowed_for_test(&win_only)
            && instance::lib_allowed_for_test(&generic)
    );

    // 阶段中文名
    check!(
        "stage label mapping",
        task::stage_label_of("jvm") == "准备 Java"
            && task::stage_label_of("download") == "下载文件"
            && task::stage_label_of("spawned") == "进程已创建"
    );
    let lbl = task::create("label-demo", None, None, "标签演示");
    task::progress(&lbl.task_id, "download", "下载中", 1, 2);
    check!(
        "stage label set",
        task::get(&lbl.task_id)
            .map(|x| x.stage_label == "下载文件" && x.phase == "downloading")
            .unwrap_or(false)
    );

    // #8 已失效 PID 的会话不能被当成"还在运行"
    let dead = session::begin("inst_dead", "task_dead", 0);
    check!(
        "session dead not active",
        session::active(Some("inst_dead")).is_none()
    );
    check!(
        "session dead reconciled",
        session::list(Some("inst_dead"), 1)
            .first()
            .map(|s| s.state == "exited")
            .unwrap_or(false)
    );
    let _ = dead;

    // #2 兼容实例（managed=false）不允许安装资源（不发网络请求就会被拒绝）
    let compat_game = std::env::temp_dir().join(format!("astral-compat-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&compat_game);
    let _ = std::fs::create_dir_all(compat_game.join("versions/1.20.1"));
    let _ = std::fs::write(compat_game.join("versions/1.20.1/1.20.1.json"), "{}");
    let compat = instance::discover(&compat_game, &play_stats);
    let compat_id = compat.first().map(|i| i.id.clone()).unwrap_or_default();
    let req = resources::ResourceInstallRequest {
        instance_id: compat_id.clone(),
        kind: "mod".into(),
        project_id: "AANobbMI".into(),
        version_id: "nonexistent".into(),
        world_id: None,
        overwrite: false,
    };
    let r = resources::prepare(&compat_game, req, &play_stats);
    check!(
        "resource rejects compat instance",
        r.as_ref()
            .err()
            .map(|e| e.starts_with("UNSUPPORTED"))
            .unwrap_or(false)
    );
    let _ = std::fs::remove_dir_all(&compat_game);

    // #5 installer 生成的 version id 推导（多实例共享 / 重复安装复用）
    check!(
        "forge version id",
        core::forge_version_id("forge", "1.20.1", "47.2.0") == "1.20.1-forge-47.2.0"
            && core::forge_version_id("neoforge", "1.21.1", "21.1.72") == "neoforge-21.1.72"
            && core::forge_version_id("neoforge", "1.20.1", "47.1.106") == "1.20.1-forge-47.1.106"
    );
    // 已装过时直接复用，不发网络请求
    let forge_game =
        std::env::temp_dir().join(format!("astral-forge-reuse-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&forge_game);
    let vid = "1.20.1-forge-47.2.0";
    let _ = std::fs::create_dir_all(forge_game.join("versions").join(vid));
    let _ = std::fs::write(
        forge_game
            .join("versions")
            .join(vid)
            .join(format!("{vid}.json")),
        r#"{"id":"1.20.1-forge-47.2.0","inheritsFrom":"1.20.1"}"#,
    );
    let reused = core::install_forge_like("forge", "1.20.1", "47.2.0", &forge_game, |_| {});
    check!(
        "forge reuse installed",
        reused.as_ref().map(|s| s == vid).unwrap_or(false)
    );
    let _ = std::fs::remove_dir_all(&forge_game);

    // #3 取消：开始前就取消，不应发起下载
    let cancel_dir = std::env::temp_dir().join(format!("astral-cancel-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&cancel_dir);
    let stored = resources::StoredResourcePlan {
        plan: resources::ResourcePlan {
            plan_id: "rplan_test".into(),
            instance_id: "inst_test".into(),
            revision: 1,
            expires_at: String::new(),
            kind: "mod".into(),
            project_id: "P".into(),
            version_id: "V".into(),
            file_name: "test.jar".into(),
            target_dir: cancel_dir.to_string_lossy().to_string(),
            download_bytes: 0,
            replaces: None,
            issues: Vec::new(),
            dependencies: Vec::new(),
        },
        url: "https://cdn.modrinth.com/data/P/versions/V/test.jar".into(),
        sha1: None,
        sha512: None,
        dep_urls: Vec::new(),
        created_ms: 0,
    };
    let cancelled = resources::execute(&stored, &*net::cancel_from(|| true), |_, _| {});
    check!(
        "resource cancel short-circuits",
        cancelled
            .as_ref()
            .err()
            .map(|e| e.starts_with("CANCELLED"))
            .unwrap_or(false)
    );
    check!(
        "resource cancel wrote nothing",
        !cancel_dir.join("test.jar").exists()
    );
    let _ = std::fs::remove_dir_all(&cancel_dir);
    // #7 安装计划里的光影组件：不可自动安装的组件必须给出阻塞原因
    let comp_game = std::env::temp_dir().join(format!("astral-plan-comp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&comp_game);
    let _ = std::fs::create_dir_all(comp_game.join("versions/1.20.1"));
    let _ = std::fs::write(comp_game.join("versions/1.20.1/1.20.1.json"), "{}");
    let _ = std::fs::write(comp_game.join("versions/1.20.1/1.20.1.jar"), "x");
    let optifine_req = plan::GameInstallRequest {
        name: "光影计划".into(),
        game_version: "1.20.1".into(),
        loader: Some(plan::LoaderSelection {
            kind: "fabric".into(),
            version: "0.16.9".into(),
        }),
        memory_mb: None,
        shader_support: Some("optifine".into()),
        shader_support_version: None,
    };
    let opti = plan::prepare(&comp_game, optifine_req);
    let opti_blocking = opti
        .as_ref()
        .map(|p| p.issues.iter().any(|i| i.blocking))
        .unwrap_or(false);
    let opti_has_comp = opti
        .as_ref()
        .map(|p| !p.components.is_empty())
        .unwrap_or(false);
    check!("plan rejects optifine", opti_blocking && !opti_has_comp);
    let _ = std::fs::remove_dir_all(&comp_game);
    // 可选：真机验证安装计划的光影组件挑选
    // ASTRAL_LIVE_PLAN="gameDir|mc|loaderKind|loaderVer|shader"
    if let Ok(spec) = std::env::var("ASTRAL_LIVE_PLAN") {
        let parts: Vec<&str> = spec.split('|').collect();
        if parts.len() == 5 {
            let game = std::path::PathBuf::from(parts[0]);
            let req = plan::GameInstallRequest {
                name: "组件计划".into(),
                game_version: parts[1].to_string(),
                loader: Some(plan::LoaderSelection {
                    kind: parts[2].to_string(),
                    version: parts[3].to_string(),
                }),
                memory_mb: None,
                shader_support: Some(parts[4].to_string()),
                shader_support_version: None,
            };
            match plan::prepare(&game, req) {
                Ok(p) => {
                    log.push_str(&format!(
                        "LIVE plan components={} blocking={}\n",
                        p.components.len(),
                        p.issues.iter().filter(|i| i.blocking).count()
                    ));
                    for c in &p.components {
                        log.push_str(&format!(
                            "  component {} {} {}\n",
                            c.id, c.label, c.version_id
                        ));
                    }
                    for i in p.issues.iter().filter(|i| i.blocking) {
                        log.push_str(&format!("  blocking {}: {}\n", i.code, i.message));
                    }
                }
                Err(e) => {
                    fails += 1;
                    log.push_str(&format!("LIVE plan failed: {e}\n"));
                }
            }
        }
    }
    // 可选：真机跑完整安装计划（含光影组件），ASTRAL_LIVE_PLAN_INSTALL="gameDir|mc|loader|ver|shader"
    if let Ok(spec) = std::env::var("ASTRAL_LIVE_PLAN_INSTALL") {
        let parts: Vec<&str> = spec.split('|').collect();
        if parts.len() == 5 || parts.len() == 6 {
            let game = std::path::PathBuf::from(parts[0]);
            // 让 game_root() 指向测试目录
            let mut cfg = core::load_config();
            cfg.game_dir = game.to_string_lossy().to_string();
            if let Err(e) = core::save_config(&cfg) {
                fails += 1;
                log.push_str(&format!("LIVE plan-install config failed: {e}\n"));
            } else {
                let req = plan::GameInstallRequest {
                    name: format!("计划安装 {}", parts[1]),
                    game_version: parts[1].to_string(),
                    loader: Some(plan::LoaderSelection {
                        kind: parts[2].to_string(),
                        version: parts[3].to_string(),
                    }),
                    memory_mb: Some(2048),
                    shader_support: Some(parts[4].to_string()),
                    shader_support_version: parts.get(5).map(|s| s.to_string()),
                };
                match plan::prepare(&game, req) {
                    Ok(p) => {
                        log.push_str(&format!(
                            "LIVE plan-install prepared components={} blocking={}\n",
                            p.components.len(),
                            p.issues.iter().filter(|i| i.blocking).count()
                        ));
                        let t = task::create(
                            "game-install",
                            None,
                            Some(parts[1].to_string()),
                            "计划安装（自检）",
                        );
                        let (res, _created) = run_game_install(&t.task_id, &p);
                        match res {
                            Ok(inst) => {
                                let mods = std::path::Path::new(&inst.dir).join("mods");
                                let jars: Vec<String> = std::fs::read_dir(&mods)
                                    .map(|rd| {
                                        rd.flatten()
                                            .map(|e| e.file_name().to_string_lossy().to_string())
                                            .collect()
                                    })
                                    .unwrap_or_default();
                                log.push_str(&format!(
                                    "LIVE plan-install ok: instance={} mods={:?}\n",
                                    inst.id, jars
                                ));
                            }
                            Err(e) => {
                                fails += 1;
                                log.push_str(&format!("LIVE plan-install failed: {e}\n"));
                            }
                        }
                    }
                    Err(e) => {
                        fails += 1;
                        log.push_str(&format!("LIVE plan-install prepare failed: {e}\n"));
                    }
                }
            }
        }
    }
    // #6 加载器与游戏版本兼容性（装之前就挡掉明显不搭的组合）
    check!(
        "loader compat forge",
        core::loader_compatible("forge", "1.20.1", "47.2.0").is_ok()
            && core::loader_compatible("forge", "1.20.1", "1.20.1-47.2.0").is_err()
    );
    check!(
        "loader compat neoforge",
        core::loader_compatible("neoforge", "1.21.1", "21.1.72").is_ok()
            && core::loader_compatible("neoforge", "1.20.1", "21.1.72").is_err()
            && core::loader_compatible("neoforge", "1.20.1", "47.1.106").is_ok()
    );
    check!(
        "neoforge mc hint",
        core::neoforge_mc_hint("21.1.72") == Some("1.21.1")
            && core::neoforge_mc_hint("20.4.237") == Some("1.20.4")
            && core::neoforge_mc_hint("99.9.9").is_none()
    );
    // 版本不搭时在下载之前就失败，不会发出 404 请求
    let bad_forge = std::env::temp_dir().join(format!("astral-bad-forge-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&bad_forge);
    let r = core::install_forge_like("forge", "1.20.1", "1.20.1-47.2.0", &bad_forge, |_| {});
    check!(
        "forge mismatched rejected early",
        r.as_ref()
            .err()
            .map(|e| e.starts_with("INVALID_VALUE") || e.starts_with("INCOMPATIBLE"))
            .unwrap_or(false)
    );
    let _ = std::fs::remove_dir_all(&bad_forge);
    // 可选：真机验证组件版本列表与越权版本拦截
    // ASTRAL_LIVE_COMPONENT_VERSIONS="id|mc|loader"
    if let Ok(spec) = std::env::var("ASTRAL_LIVE_COMPONENT_VERSIONS") {
        let parts: Vec<&str> = spec.split('|').collect();
        if parts.len() == 3 {
            match components::versions(parts[0], parts[1], Some(parts[2])) {
                Ok(list) => {
                    log.push_str(&format!(
                        "LIVE component versions={} first={} {} recommended={}\n",
                        list.len(),
                        list.first()
                            .map(|v| v.version_id.clone())
                            .unwrap_or_default(),
                        list.first()
                            .map(|v| v.version_number.clone())
                            .unwrap_or_default(),
                        list.first().map(|v| v.recommended).unwrap_or(false)
                    ));
                    // 用别的组件版本 id 去校验，必须被拒
                    let foreign = components::validate_choice(
                        parts[0],
                        "NotARealVersionId",
                        parts[1],
                        Some(parts[2]),
                    );
                    log.push_str(&format!(
                        "LIVE foreign version rejected={}\n",
                        foreign.is_err()
                    ));
                    if foreign.is_ok() {
                        fails += 1;
                    }
                }
                Err(e) => {
                    fails += 1;
                    log.push_str(&format!("LIVE component versions failed: {e}\n"));
                }
            }
        }
    }
    // 旧下载接口的目录规则
    check!(
        "legacy dir rules",
        legacy_sub_dir("mod") == "mods"
            && legacy_sub_dir("shaderpack") == "shaderpacks"
            && legacy_requires_instance("mod")
            && legacy_requires_instance("datapack")
            && !legacy_requires_instance("resourcepack")
            && !legacy_requires_instance("shaderpack")
    );
    // Java 要求的主版本会沿 inheritsFrom 找（加载器版本自己通常不带 javaVersion）
    let jgame = std::env::temp_dir().join(format!("astral-java-chain-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&jgame);
    let _ = std::fs::create_dir_all(jgame.join("versions/1.20.1"));
    let _ = std::fs::create_dir_all(jgame.join("versions/1.20.1-forge-47.2.0"));
    let _ = std::fs::write(
        jgame.join("versions/1.20.1/1.20.1.json"),
        r#"{"javaVersion":{"majorVersion":17}}"#,
    );
    let _ = std::fs::write(
        jgame.join("versions/1.20.1-forge-47.2.0/1.20.1-forge-47.2.0.json"),
        r#"{"inheritsFrom":"1.20.1"}"#,
    );
    check!(
        "java required via inheritsFrom",
        java::required_major(&jgame, "1.20.1-forge-47.2.0") == Some(17)
            && java::required_major(&jgame, "1.20.1") == Some(17)
    );
    let _ = std::fs::remove_dir_all(&jgame);
    // 修复计划：只补缺失，不乱动已有文件
    let rgame = std::env::temp_dir().join(format!("astral-repair-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&rgame);
    let _ = std::fs::create_dir_all(rgame.join("versions/1.20.1"));
    let _ = std::fs::write(rgame.join("versions/1.20.1/1.20.1.json"), "{}");
    let mut rmeta = instance::InstanceMeta {
        id: "inst_repair".into(),
        name: "修复测试".into(),
        game_version: "1.20.1".into(),
        loader_kind: Some("fabric".into()),
        loader_version: Some("0.16.9".into()),
        launch_version: None,
        memory_mb: None,
        jvm_args: vec![],
        java_path: None,
        cover: None,
        created_at: time::now_iso(),
    };
    let steps = core::repair_steps(&rgame, &rmeta);
    check!(
        "repair plans base and loader",
        steps.contains(&"base") && steps.contains(&"loader")
    );
    // 补齐后不该再有步骤
    let _ = std::fs::write(rgame.join("versions/1.20.1/1.20.1.jar"), "x");
    let _ = std::fs::create_dir_all(rgame.join("assets/indexes"));
    let _ = std::fs::create_dir_all(rgame.join("versions/fabric-1.20.1-0.16.9"));
    let _ = std::fs::write(
        rgame.join("versions/fabric-1.20.1-0.16.9/fabric-1.20.1-0.16.9.json"),
        r#"{"inheritsFrom":"1.20.1"}"#,
    );
    rmeta.launch_version = Some("fabric-1.20.1-0.16.9".into());
    let fixed = core::repair_steps(&rgame, &rmeta);
    check!("repair nothing left", fixed.is_empty());
    let _ = std::fs::remove_dir_all(&rgame);
    // 日志诊断：能认出常见崩溃原因
    let dgame = std::env::temp_dir().join(format!("astral-diag-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dgame);
    let _ = std::fs::create_dir_all(dgame.join("logs"));
    let _ = std::fs::write(
        dgame.join("logs/latest.log"),
        "java.lang.UnsupportedClassVersionError: 65.0\n\
         java.lang.NoClassDefFoundError: net/minecraft/client/Minecraft\n\
         java.lang.OutOfMemoryError: Java heap space\n",
    );
    match logs::diagnose(&dgame, None, 100) {
        Ok(r) => {
            let ids: Vec<&str> = r.findings.iter().map(|f| f.id).collect();
            check!(
                "diagnose finds root causes",
                ids.contains(&"java-version")
                    && ids.contains(&"missing-class")
                    && ids.contains(&"out-of-memory")
                    && !r.text.is_empty()
            );
        }
        Err(_) => check!("diagnose finds root causes", false),
    }
    let _ = std::fs::remove_dir_all(&dgame);
    let cfg = core::Config::default();
    check!(
        "config defaults",
        cfg.active_instance_id.is_none() && !cfg.game_dir.is_empty()
    );

    // ── A1/A2/A4：本地 HTTP 服务器上的真实下载测试 ──────────────
    let dl = std::env::temp_dir().join(format!("astral-dl-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dl);
    let _ = std::fs::create_dir_all(&dl);

    match testsrv::TestServer::start() {
        Ok(srv) => {
            let local_opts = |sha1: Option<&str>| net::DownloadOptions {
                expected_sha1: sha1.map(|s| s.to_string()),
                allow_local_http: true,
                ..Default::default()
            };

            // 1) 普通下载 + SHA1 校验
            let size = 512u64 * 1024;
            let want = testsrv::expected_sha1(size);
            let dest = dl.join("plain.bin");
            let ok = net::download(
                &srv.data_url(size, 64 * 1024, 0),
                &dest,
                &local_opts(Some(&want)),
                &*net::never_cancel(),
                |_, _| {},
            )
            .map(|r| r.bytes == size && !r.reused)
            .unwrap_or(false);
            check!(
                "download plain + sha1",
                ok && net::sha1_file(&dest).ok().as_deref() == Some(want.as_str())
            );

            // 2) 已存在且哈希正确时不联网
            let srv_served_before = srv.served.load(std::sync::atomic::Ordering::SeqCst);
            let reused = net::download(
                &srv.data_url(size, 64 * 1024, 0),
                &dest,
                &local_opts(Some(&want)),
                &*net::never_cancel(),
                |_, _| {},
            )
            .map(|r| r.reused)
            .unwrap_or(false);
            let srv_served_after = srv.served.load(std::sync::atomic::Ordering::SeqCst);
            check!(
                "download reuses verified file",
                reused && srv_served_after == srv_served_before
            );

            // 3) 哈希不匹配 → 报错且不留正式文件
            let bad_dest = dl.join("bad.bin");
            let bad = net::download(
                &srv.data_url(size, 64 * 1024, 0),
                &bad_dest,
                &local_opts(Some("0000000000000000000000000000000000000000")),
                &*net::never_cancel(),
                |_, _| {},
            );
            check!(
                "download rejects bad hash",
                bad.is_err()
                    && !bad_dest.exists()
                    && bad
                        .as_ref()
                        .map(|_| false)
                        .unwrap_or_else(|e| e.starts_with("HASH_MISMATCH"))
            );

            // 4) 取消：慢速传输中途取消，必须真的停下并保留 .part
            let cancel_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let flag2 = cancel_flag.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(250));
                flag2.store(true, std::sync::atomic::Ordering::SeqCst);
            });
            let slow_dest = dl.join("slow.bin");
            let cancel = net::cancel_from({
                let f = cancel_flag.clone();
                move || f.load(std::sync::atomic::Ordering::SeqCst)
            });
            let cancel_size = 1024u64 * 1024;
            let slow_url = srv.data_url(cancel_size, 32 * 1024, 20);
            let started = std::time::Instant::now();
            let cancelled = net::download(
                &slow_url,
                &slow_dest,
                &local_opts(None),
                &*cancel,
                |_, _| {},
            );
            let elapsed = started.elapsed();
            let part_kept =
                std::path::PathBuf::from(format!("{}.part", slow_dest.to_string_lossy()));
            let part_len = std::fs::metadata(&part_kept).map(|m| m.len()).unwrap_or(0);
            check!(
                "download cancel stops early",
                cancelled
                    .as_ref()
                    .map(|_| false)
                    .unwrap_or_else(|e| e.starts_with("CANCELLED"))
                    && !slow_dest.exists()
                    && part_len > 0
                    && part_len < cancel_size
                    && elapsed < std::time::Duration::from_secs(3)
            );

            // 5) 续传：从刚才的 .part 接着下，最终文件必须完整且哈希正确
            let want_slow = testsrv::expected_sha1(cancel_size);
            let resumed = net::download(
                &slow_url,
                &slow_dest,
                &local_opts(Some(&want_slow)),
                &*net::never_cancel(),
                |_, _| {},
            );
            check!(
                "download resumes from .part",
                resumed
                    .map(|r| r.resumed && r.bytes == cancel_size)
                    .unwrap_or(false)
                    && net::sha1_file(&slow_dest).ok().as_deref() == Some(want_slow.as_str())
            );

            // 6) 503 会自动重试
            let flaky_dest = dl.join("flaky.bin");
            let flaky = net::download(
                &srv.flaky_url(2, 128 * 1024),
                &flaky_dest,
                &local_opts(None),
                &*net::never_cancel(),
                |_, _| {},
            );
            check!(
                "download retries 503",
                flaky
                    .map(|r| r.attempts == 3 && r.bytes == 128 * 1024)
                    .unwrap_or(false)
            );

            // 7) 重定向到白名单外必须被拦住
            let redir_dest = dl.join("redir.bin");
            let redir = net::download(
                &srv.redirect_url("https://evil.example.com/x.bin"),
                &redir_dest,
                &local_opts(None),
                &*net::never_cancel(),
                |_, _| {},
            );
            check!(
                "download blocks bad redirect",
                redir.is_err() && !redir_dest.exists()
            );

            // 8) 同一目标并发写：第二个任务必须等，不能互相覆盖
            let same = dl.join("same.bin");
            let same2 = same.clone();
            let url1 = srv.data_url(256 * 1024, 16 * 1024, 5);
            let h = std::thread::spawn(move || {
                net::download(
                    &url1,
                    &same2,
                    &net::DownloadOptions {
                        allow_local_http: true,
                        ..Default::default()
                    },
                    &*net::never_cancel(),
                    |_, _| {},
                )
                .map(|r| r.bytes)
            });
            std::thread::sleep(std::time::Duration::from_millis(30));
            let second = net::download(
                &srv.data_url(256 * 1024, 16 * 1024, 5),
                &same,
                &net::DownloadOptions {
                    allow_local_http: true,
                    ..Default::default()
                },
                &*net::never_cancel(),
                |_, _| {},
            )
            .map(|r| r.bytes);
            let first = h.join().ok().and_then(|r| r.ok());
            let ok_same = first == Some(256 * 1024) && second == Ok(256 * 1024);
            if !ok_same {
                log.push_str(&format!(
                    "LIVE same-target: first={first:?} second={second:?}\n"
                ));
            }
            check!("download same target serialized", ok_same);

            // 9) 内网地址仍然被拒绝（自检开关只放行 127.0.0.1 的 http）
            check!(
                "download blocks private ip",
                net::validate_url("https://192.168.1.1/a.jar").is_err()
                    && net::validate_url("https://10.0.0.5/a.jar").is_err()
                    && net::validate_url("http://127.0.0.1:1/a.jar").is_err()
            );

            // 10) .part 元数据在取消后写入，且 URL 变了不会误续传
            let meta_path =
                std::path::PathBuf::from(format!("{}.part.json", slow_dest.to_string_lossy()));
            check!("part meta cleaned after commit", !meta_path.exists());

            // 11) 并发上限：12 个文件同时下，活跃数不能超过配置上限
            let conc_dir = dl.join("conc");
            let _ = std::fs::create_dir_all(&conc_dir);
            let peak = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let mut handles = Vec::new();
            for i in 0..12 {
                let url = srv.data_url(96 * 1024, 8 * 1024, 25);
                let dest = conc_dir.join(format!("c{i}.bin"));
                let peak_c = peak.clone();
                handles.push(std::thread::spawn(move || {
                    let opts = net::DownloadOptions {
                        allow_local_http: true,
                        ..Default::default()
                    };
                    let mut max_seen = 0usize;
                    let r = net::download(&url, &dest, &opts, &*net::never_cancel(), |_, _| {
                        let a = net::scheduler_stats()["active"].as_u64().unwrap_or(0) as usize;
                        if a > max_seen {
                            max_seen = a;
                        }
                        peak_c.fetch_max(a, std::sync::atomic::Ordering::SeqCst);
                    });
                    (r.map(|x| x.bytes), max_seen)
                }));
            }
            let mut all_ok = true;
            for h in handles {
                match h.join() {
                    Ok((r, _)) if r == Ok(96 * 1024) => {}
                    _ => all_ok = false,
                }
            }
            let peak_seen = peak.load(std::sync::atomic::Ordering::SeqCst);
            let stats_after = net::scheduler_stats();
            check!(
                "download concurrency capped",
                all_ok
                    && peak_seen <= 6
                    && stats_after["active"].as_u64() == Some(0)
                    && stats_after["downloads"].as_u64().unwrap_or(0) >= 12
            );

            // 12) 同域上限：一个域名最多 3 个同时进行
            let host_peak = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let mut hs = Vec::new();
            for i in 0..8 {
                let url = srv.data_url(64 * 1024, 8 * 1024, 30);
                let dest = conc_dir.join(format!("h{i}.bin"));
                let hp = host_peak.clone();
                hs.push(std::thread::spawn(move || {
                    let opts = net::DownloadOptions {
                        allow_local_http: true,
                        ..Default::default()
                    };
                    let _ = net::download(&url, &dest, &opts, &*net::never_cancel(), |_, _| {
                        let s = net::scheduler_stats();
                        let used = s["hosts"]["127.0.0.1"].as_u64().unwrap_or(0) as usize;
                        hp.fetch_max(used, std::sync::atomic::Ordering::SeqCst);
                    });
                }));
            }
            for h in hs {
                let _ = h.join();
            }
            check!(
                "download per-host capped",
                host_peak.load(std::sync::atomic::Ordering::SeqCst) <= 3
            );

            // 13) 取消时排队中的下载不会卡死，也不会写出正式文件
            let queued_dest = dl.join("queued.bin");
            let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let stop2 = stop.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(120));
                stop2.store(true, std::sync::atomic::Ordering::SeqCst);
            });
            let qcancel = net::cancel_from({
                let f = stop.clone();
                move || f.load(std::sync::atomic::Ordering::SeqCst)
            });
            let qr = net::download(
                &srv.data_url(2 * 1024 * 1024, 16 * 1024, 40),
                &queued_dest,
                &local_opts(None),
                &*qcancel,
                |_, _| {},
            );
            check!(
                "download cancel while queued",
                qr.as_ref()
                    .map(|_| false)
                    .unwrap_or_else(|e| e.starts_with("CANCELLED"))
                    && !queued_dest.exists()
            );

            // 13b) portablemc 的批量下载器也能被取消（安装游戏本体走的就是这条路）
            {
                let bdir = dl.join("batch-cancel");
                let _ = std::fs::create_dir_all(&bdir);
                let mut batch = portablemc::download::Batch::new();
                for i in 0..6 {
                    let dest = bdir.join(format!("b{i}.bin"));
                    batch.push(
                        srv.data_url(512 * 1024, 32 * 1024, 30),
                        dest.into_boxed_path(),
                    );
                }
                let bstop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                let bstop2 = bstop.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    bstop2.store(true, std::sync::atomic::Ordering::SeqCst);
                });
                let bflag = bstop.clone();
                batch.set_cancel(std::sync::Arc::new(move || {
                    bflag.load(std::sync::atomic::Ordering::SeqCst)
                }));
                let bres = batch.download(());
                let cancelled_count = bres
                    .as_ref()
                    .map(|r| {
                        r.iter_errors()
                            .filter(|e| {
                                matches!(e.kind(), portablemc::download::EntryErrorKind::Cancelled)
                            })
                            .count()
                    })
                    .unwrap_or(0);
                // 已经下完的文件不应该存在（取消的分片会被清掉）
                let full = (0..6)
                    .filter(|i| {
                        std::fs::metadata(bdir.join(format!("b{i}.bin")))
                            .map(|m| m.len() == 512 * 1024)
                            .unwrap_or(false)
                    })
                    .count();
                check!(
                    "portablemc batch download cancels",
                    cancelled_count >= 1 && full < 6
                );
            }

            // 14) 共享缓存：第二次下同一个文件不再联网，缓存里能查到
            let c1 = dl.join("cache1.bin");
            let c2 = dl.join("cache2.bin");
            let csha = testsrv::expected_sha1(200 * 1024);
            let before_hits = srv.served.load(std::sync::atomic::Ordering::SeqCst);
            let _ = net::download(
                &srv.data_url(200 * 1024, 32 * 1024, 0),
                &c1,
                &local_opts(Some(&csha)),
                &*net::never_cancel(),
                |_, _| {},
            );
            let after_first = srv.served.load(std::sync::atomic::Ordering::SeqCst);
            let second = net::download(
                &srv.data_url(200 * 1024, 32 * 1024, 0),
                &c2,
                &local_opts(Some(&csha)),
                &*net::never_cancel(),
                |_, _| {},
            );
            let after_second = srv.served.load(std::sync::atomic::Ordering::SeqCst);
            check!(
                "cache serves second copy offline",
                after_first > before_hits
                    && after_second == after_first
                    && second
                        .map(|r| r.reused && r.bytes == 200 * 1024)
                        .unwrap_or(false)
                    && net::sha1_file(&c2).ok().as_deref() == Some(csha.as_str())
            );

            // 15) 缓存统计与预算淘汰
            let st = cache::stats(cache::DEFAULT_BUDGET_BYTES);
            let freed = cache::enforce_budget(1024 * 1024);
            let st_after = cache::stats(cache::DEFAULT_BUDGET_BYTES);
            check!(
                "cache stats and budget eviction",
                st.available
                    && st.entries >= 1
                    && st.bytes > 0
                    && st.budget_bytes == cache::DEFAULT_BUDGET_BYTES
                    && (freed > 0 || st_after.bytes <= 1024 * 1024)
                    && st_after.bytes <= st.bytes
            );

            // 16) 清空缓存：真的清空，且只动缓存目录
            let before_clear = cache::stats(cache::DEFAULT_BUDGET_BYTES).entries;
            let cleared = cache::clear();
            let after_clear = cache::stats(cache::DEFAULT_BUDGET_BYTES);
            check!(
                "cache clear empties only cache",
                before_clear >= 1
                    && cleared >= 1
                    && after_clear.entries == 0
                    && c1.exists()
                    && c2.exists()
            );

            // 17) 残留 .part 清理只删半截文件
            let stale_dir = dl.join("stale");
            let _ = std::fs::create_dir_all(&stale_dir);
            let keep = stale_dir.join("keep.jar");
            let stale = stale_dir.join("old.jar.part");
            let _ = std::fs::write(&keep, b"keep");
            let _ = std::fs::write(&stale, b"partial");
            let (rm, _) = cache::clean_stale_partials(&stale_dir, 0);
            check!(
                "clean stale partials",
                rm >= 1 && keep.exists() && !stale.exists()
            );

            // 18) 整合包解压拒绝路径穿越（真实 zip，不是纯函数测试）
            {
                use std::io::Write;
                let zip_path = dl.join("evil.mrpack");
                let f = std::fs::File::create(&zip_path).unwrap();
                let mut zw = zip::ZipWriter::new(f);
                let opts = zip::write::SimpleFileOptions::default();
                // 正常文件
                zw.start_file("overrides/config/ok.json", opts).unwrap();
                zw.write_all(b"{}").unwrap();
                // 穿越文件：写到实例目录外
                zw.start_file("overrides/../../escaped.txt", opts).unwrap();
                zw.write_all(b"bad").unwrap();
                zw.finish().unwrap();

                let inst = dl.join("evil-inst");
                let _ = std::fs::create_dir_all(&inst);
                let r =
                    modpack::extract_overrides(&zip_path, &inst, &*net::never_cancel(), |_, _| {});
                let escaped = dl.join("escaped.txt");
                check!(
                    "modpack rejects zip traversal",
                    r.is_err() && !escaped.exists()
                );
            }

            // 19) 整合包解压拒绝符号链接条目
            {
                let zip_path = dl.join("symlink.mrpack");
                let f = std::fs::File::create(&zip_path).unwrap();
                let mut zw = zip::ZipWriter::new(f);
                let opts = zip::write::SimpleFileOptions::default();
                zw.add_symlink("overrides/link", "/etc/passwd", opts)
                    .unwrap();
                zw.finish().unwrap();

                let inst = dl.join("symlink-inst");
                let _ = std::fs::create_dir_all(&inst);
                let r =
                    modpack::extract_overrides(&zip_path, &inst, &*net::never_cancel(), |_, _| {});
                check!("modpack rejects symlink entries", r.is_err());
            }

            // 20) 正常整合包仍然能解压
            {
                use std::io::Write;
                let zip_path = dl.join("good.mrpack");
                let f = std::fs::File::create(&zip_path).unwrap();
                let mut zw = zip::ZipWriter::new(f);
                let opts = zip::write::SimpleFileOptions::default();
                zw.start_file("overrides/config/a.json", opts).unwrap();
                zw.write_all(b"{\"a\":1}").unwrap();
                zw.start_file("client-overrides/options.txt", opts).unwrap();
                zw.write_all(b"lang:zh_cn").unwrap();
                zw.finish().unwrap();

                let inst = dl.join("good-inst");
                let _ = std::fs::create_dir_all(&inst);
                let r =
                    modpack::extract_overrides(&zip_path, &inst, &*net::never_cancel(), |_, _| {});
                check!(
                    "modpack extracts normal overrides",
                    r == Ok(2)
                        && inst.join("config").join("a.json").exists()
                        && inst.join("options.txt").exists()
                );
            }

            // 21) index 的 env 分类正确（optional / unsupported 不默认安装）
            {
                let j = r#"{
                  "name":"EnvPack","dependencies":{"minecraft":"1.20.1","fabric-loader":"0.16.9"},
                  "files":[
                    {"path":"mods/a.jar","downloads":["https://cdn.modrinth.com/a.jar"],"fileSize":1,"env":{"client":"required","server":"required"}},
                    {"path":"mods/b.jar","downloads":["https://cdn.modrinth.com/b.jar"],"fileSize":1,"env":{"client":"optional","server":"optional"}},
                    {"path":"mods/c.jar","downloads":["https://cdn.modrinth.com/c.jar"],"fileSize":1,"env":{"client":"unsupported","server":"required"}},
                    {"path":"mods/d.jar","downloads":["https://cdn.modrinth.com/d.jar"],"fileSize":1}
                  ]
                }"#;
                let idx = modpack::parse_index(j).unwrap();
                let wanted_default: Vec<&str> = idx
                    .files
                    .iter()
                    .filter(|f| f.wanted_on_client(false))
                    .map(|f| f.path.as_str())
                    .collect();
                let wanted_all: Vec<&str> = idx
                    .files
                    .iter()
                    .filter(|f| f.wanted_on_client(true))
                    .map(|f| f.path.as_str())
                    .collect();
                check!(
                    "modpack env filters client files",
                    wanted_default == vec!["mods/a.jar", "mods/d.jar"]
                        && wanted_all == vec!["mods/a.jar", "mods/b.jar", "mods/d.jar"]
                        && idx.client_files == 2
                        && idx.optional_files == 1
                        && idx.unsupported_files == 1
                        && idx.server_only_files == 1
                );
            }

            // 22) 本地 .mrpack 导入：拒绝非 mrpack、拒绝不存在的文件
            {
                let g = dl.join("modpack-game");
                let _ = std::fs::create_dir_all(&g);
                let txt = dl.join("not-a-pack.txt");
                let _ = std::fs::write(&txt, b"hello");
                let r1 = modpack::prepare_local(&g, &txt.to_string_lossy(), None);
                let missing = g.join("nope.mrpack");
                let r2 = modpack::prepare_local(&g, &missing.to_string_lossy(), None);
                check!(
                    "modpack local import rejects bad input",
                    r1.is_err() && r2.is_err()
                );
            }

            // 23) 本地 .mrpack 导入能解析并标记来源
            {
                use std::io::Write;
                let g = dl.join("modpack-game");
                let _ = std::fs::create_dir_all(&g);
                let pack = dl.join("local-test.mrpack");
                {
                    let f = std::fs::File::create(&pack).unwrap();
                    let mut zw = zip::ZipWriter::new(f);
                    let opts = zip::write::SimpleFileOptions::default();
                    let index = serde_json::json!({
                        "formatVersion": 1,
                        "game": "minecraft",
                        "name": "本地包",
                        "dependencies": {"minecraft": "1.20.1", "fabric-loader": "0.16.9"},
                        "files": [
                            {"path": "mods/x.jar", "downloads": ["https://cdn.modrinth.com/x.jar"],
                             "hashes": {"sha1": "aa", "sha512": "bb"}, "fileSize": 12}
                        ]
                    });
                    zw.start_file("modrinth.index.json", opts).unwrap();
                    zw.write_all(serde_json::to_string(&index).unwrap().as_bytes())
                        .unwrap();
                    zw.start_file("overrides/config/x.json", opts).unwrap();
                    zw.write_all(b"{}").unwrap();
                    zw.finish().unwrap();
                }
                match modpack::prepare_local(&g, &pack.to_string_lossy(), Some("我的包".into()))
                {
                    Ok(plan) => check!(
                        "modpack local import parses",
                        plan.info.source == "local"
                            && plan.info.name == "我的包"
                            && plan.info.mc_version == "1.20.1"
                            && plan.info.loader_kind.as_deref() == Some("fabric")
                            && plan.info.file_count == 1
                            && plan.info.override_entries == 1
                            && plan.info.project_id.is_empty()
                    ),
                    Err(e) => {
                        log.push_str(&format!("LIVE local import: {e}\n"));
                        check!("modpack local import parses", false);
                    }
                }
            }

            // 24) 整合包来源记录与"同包已装版本"查询
            {
                let g = dl.join("modpack-game");
                let inst_root = crate::instance::instances_root(&g);
                let dir = inst_root.join("inst_prov");
                let _ = std::fs::create_dir_all(&dir);
                let prov = modpack::PackProvenance {
                    project_id: "proj-1".into(),
                    version_id: "ver-1".into(),
                    version_name: "1.0.0".into(),
                    source: "modrinth".into(),
                    pack_name: "测试包".into(),
                    installed_at: "2026-01-01T00:00:00Z".into(),
                };
                let wrote = modpack::write_provenance(&dir, &prov).is_ok();
                let back = modpack::read_provenance(&dir);
                let list = modpack::installed_versions(&g, "proj-1");
                let other = modpack::installed_versions(&g, "proj-2");
                check!(
                    "modpack provenance roundtrip",
                    wrote
                        && back.as_ref().map(|p| p.version_id.as_str()) == Some("ver-1")
                        && list.len() == 1
                        && list[0]["instanceId"].as_str() == Some("inst_prov")
                        && other.is_empty()
                );
            }

            // 25) 磁盘预检能拿到可用空间
            {
                let g = dl.join("modpack-game");
                let idx = modpack::parse_index(
                    r#"{"dependencies":{"minecraft":"1.20.1"},"files":[
                        {"path":"mods/a.jar","downloads":["https://cdn.modrinth.com/a.jar"],"fileSize":2048}
                    ]}"#,
                )
                .unwrap();
                let (need, avail) = modpack::disk_precheck(&idx, &g);
                log.push_str(&format!(
                    "LIVE disk precheck: need={need} avail={avail:?} dir={}\n",
                    g.display()
                ));
                check!(
                    "modpack disk precheck reports space",
                    need == 2048 && avail.map(|a| a > 0).unwrap_or(false)
                );
            }

            // 26) 导出回读校验：只允许 exports 目录，且能识别缺哈希
            {
                use std::io::Write;
                let g = dl.join("modpack-game");
                let out_dir = g.join("exports");
                let _ = std::fs::create_dir_all(&out_dir);
                let pack = out_dir.join("roundtrip.mrpack");
                {
                    let f = std::fs::File::create(&pack).unwrap();
                    let mut zw = zip::ZipWriter::new(f);
                    let opts = zip::write::SimpleFileOptions::default();
                    let index = serde_json::json!({
                        "formatVersion": 1, "game": "minecraft", "name": "导出包",
                        "dependencies": {"minecraft": "1.20.1"},
                        "files": [
                            {"path": "mods/ok.jar", "downloads": ["https://cdn.modrinth.com/ok.jar"],
                             "hashes": {"sha1": "aa"}, "fileSize": 1},
                            {"path": "mods/nohash.jar", "downloads": ["https://cdn.modrinth.com/nohash.jar"],
                             "fileSize": 1}
                        ]
                    });
                    zw.start_file("modrinth.index.json", opts).unwrap();
                    zw.write_all(serde_json::to_string(&index).unwrap().as_bytes())
                        .unwrap();
                    zw.finish().unwrap();
                }
                let ok_report = export::verify_roundtrip(&g, &pack.to_string_lossy());
                let outside = dl.join("outside.mrpack");
                let _ = std::fs::write(&outside, b"x");
                let blocked = export::verify_roundtrip(&g, &outside.to_string_lossy());
                check!(
                    "export roundtrip verify",
                    blocked.is_err()
                        && ok_report
                            .map(|v| v["ok"] == serde_json::json!(false)
                                && v["files"].as_u64() == Some(2)
                                && v["issues"]
                                    .as_array()
                                    .map(|a| a.len() == 1)
                                    .unwrap_or(false))
                            .unwrap_or(false)
                );
            }

            // D1：元数据缓存（离线可用 + 如实标注数据时间）
            {
                if let Ok(srv) = testsrv::TestServer::start() {
                    let url = srv.json_url(2048);
                    // 1) 第一次走网络
                    let first = net::fetch_json_cached_for_test(&url, Some(60_000));
                    let first_ok = first
                        .as_ref()
                        .map(|v| {
                            v["__cache"]["source"] == serde_json::json!("network")
                                && v["__cache"]["stale"] == serde_json::json!(false)
                        })
                        .unwrap_or(false);
                    let served_after_first = srv.served.load(std::sync::atomic::Ordering::SeqCst);
                    // 2) 第二次应该命中缓存，不再请求服务器
                    let second = net::fetch_json_cached_for_test(&url, Some(60_000));
                    let second_ok = second
                        .as_ref()
                        .map(|v| v["__cache"]["source"] == serde_json::json!("cache"))
                        .unwrap_or(false);
                    let served_after_second = srv.served.load(std::sync::atomic::Ordering::SeqCst);
                    check!(
                        "meta cache serves fresh without network",
                        first_ok && second_ok && served_after_second == served_after_first
                    );

                    // 3) 关掉服务器模拟断网 + 强制过期：必须回落到缓存并标记 stale
                    drop(srv);
                    std::thread::sleep(std::time::Duration::from_millis(120));
                    let stale = net::fetch_json_cached_for_test(&url, Some(0));
                    let stale_ok = stale
                        .as_ref()
                        .map(|v| {
                            v["__cache"]["source"] == serde_json::json!("cache")
                                && v["__cache"]["stale"] == serde_json::json!(true)
                                && v["__cache"]["networkError"].is_string()
                                && v["__cache"]["fetchedAt"].is_string()
                        })
                        .unwrap_or(false);
                    check!("meta cache falls back when offline", stale_ok);

                    // 4) 统计结构
                    let st = net::meta_stats();
                    check!(
                        "meta cache stats shape",
                        st["count"].as_u64().unwrap_or(0) >= 1
                            && st["bytes"].as_u64().unwrap_or(0) >= 20
                            && st["ttlMs"].as_i64() == Some(net::META_TTL_MS)
                            && st["dir"].is_string()
                    );

                    // 5) 清理：保留天数极大时不该删掉刚写的缓存
                    let (removed, _) = net::meta_prune(3650);
                    let kept = net::meta_stats()["count"].as_u64().unwrap_or(0);
                    check!(
                        "meta cache prune keeps fresh entries",
                        removed == 0 && kept >= 1
                    );

                    // 6) 清空必须确认
                    let refused = net::meta_clear(false).is_err();
                    let cleared = net::meta_clear(true).map(|n| n >= 1).unwrap_or(false);
                    check!("meta cache clear requires confirm", refused && cleared);
                } else {
                    check!("meta cache serves fresh without network", false);
                }
            }

            // 20.5) Modrinth 资源搜索：参数拼装 + 响应解析 + 缓存来源标注
            {
                if let Ok(srv) = testsrv::TestServer::start() {
                    let base = srv.search_base();
                    // 1) 正常搜索：filters 必须真的进到 facets 里
                    let one = search::search_for_test(
                        &base,
                        "sodium",
                        Some("mod"),
                        Some("1.20.1"),
                        Some("fabric"),
                        Some(20),
                        Some(0),
                        Some("relevance"),
                    );
                    let one_ok = one
                        .as_ref()
                        .map(|r| {
                            r.hits.len() == 1
                                && r.hits[0].project_id == "testproject"
                                && r.hits[0].title == "Test sodium"
                                && r.hits[0].project_type == "mod"
                                && r.hits[0].description.contains("versions:1.20.1")
                                && r.hits[0].description.contains("categories:fabric")
                                && r.total_hits == 1
                                && r.source == "network"
                                && !r.stale
                        })
                        .unwrap_or(false);
                    check!("search returns parsed hits", one_ok);

                    // 2) 同一 URL 第二次必须命中缓存，且不再打服务器
                    let served_before = srv.served.load(std::sync::atomic::Ordering::SeqCst);
                    let two = search::search_for_test(
                        &base,
                        "sodium",
                        Some("mod"),
                        Some("1.20.1"),
                        Some("fabric"),
                        Some(20),
                        Some(0),
                        Some("relevance"),
                    );
                    let served_after = srv.served.load(std::sync::atomic::Ordering::SeqCst);
                    let two_ok = two
                        .as_ref()
                        .map(|r| r.source == "cache" && !r.stale && r.age_ms >= 0)
                        .unwrap_or(false);
                    check!(
                        "search reuses cached result",
                        two_ok && served_after == served_before
                    );

                    // 3) 非法类型 / 排序必须明确报错，不能静默降级
                    let bad_kind = search::search_for_test(
                        &base,
                        "x",
                        Some("taco"),
                        None,
                        None,
                        None,
                        None,
                        None,
                    )
                    .is_err();
                    let bad_sort = search::search_for_test(
                        &base,
                        "x",
                        None,
                        None,
                        None,
                        None,
                        None,
                        Some("taco"),
                    )
                    .is_err();
                    check!("search rejects bad filters", bad_kind && bad_sort);
                } else {
                    check!("search returns parsed hits", false);
                }
            }

            // 服务器列表查询：握手、VarInt、JSON 解析整条链走通
            match testsrv::FakeMcServer::start("§6Astral §fTest") {
                Ok(mc) => {
                    let st = server::ping("127.0.0.1", Some(mc.port), Some(3000));
                    match st {
                        Ok(s) => {
                            check!(
                                "server ping reads status",
                                s.online
                                    && s.version_name.as_deref() == Some("1.20.1")
                                    && s.version_protocol == Some(763)
                                    && s.players_online == Some(7)
                                    && s.players_max == Some(100)
                                    && s.sample.len() == 2
                                    && s.sample[0] == "Alice"
                            );
                            // 颜色代码要剥掉，原始串要留着
                            check!(
                                "server ping strips color codes",
                                s.motd == "Astral Test" && s.motd_raw == "§6Astral §fTest"
                            );
                            check!(
                                "server ping keeps favicon",
                                s.favicon
                                    .as_deref()
                                    .map(|f| f.starts_with("data:image/png;base64,"))
                                    .unwrap_or(false)
                                    && !s.legacy
                                    && s.latency_ms < 3000
                            );
                            // host:port 写法要能自己拆开
                            let addr = format!("127.0.0.1:{}", mc.port);
                            let st2 = server::ping(&addr, None, Some(3000));
                            check!(
                                "server ping accepts host:port",
                                st2.map(|s| s.online && s.port == mc.port).unwrap_or(false)
                            );
                        }
                        Err(e) => {
                            check!("server ping reads status", false);
                            check!("server ping strips color codes", false);
                            check!("server ping keeps favicon", false);
                            check!("server ping accepts host:port", false);
                            let _ = e;
                        }
                    }
                }
                Err(_) => {
                    check!("server ping reads status", false);
                    check!("server ping strips color codes", false);
                    check!("server ping keeps favicon", false);
                    check!("server ping accepts host:port", false);
                }
            }
            // 关着的端口：不报错，返回 online=false + 原因
            let closed = server::ping("127.0.0.1", Some(1), Some(800));
            check!(
                "server ping reports offline without error",
                closed
                    .map(|s| !s.online && s.error.is_some() && s.latency_ms < 5000)
                    .unwrap_or(false)
            );
            // 地址本身不合法才算错误
            check!(
                "server ping rejects bad address",
                server::ping("bad host", None, None).is_err()
                    && server::ping("", None, None).is_err()
                    && server::ping("x", Some(0), None).is_err()
            );

            // 收藏的服务器：增删改查 + 批量刷新（真实握手）
            {
                let sg = game.join("servers-test");
                let _ = std::fs::create_dir_all(&sg);
                match (
                    servers::add(&sg, "主服", "127.0.0.1:1", None, "测试"),
                    testsrv::FakeMcServer::start("§aHello"),
                ) {
                    (Ok(main_srv), Ok(mc)) => {
                        let live = servers::add(
                            &sg,
                            "本地测试",
                            &format!("127.0.0.1:{}", mc.port),
                            None,
                            "",
                        )
                        .unwrap();
                        check!(
                            "servers add keeps host and port apart",
                            main_srv.host == "127.0.0.1"
                                && main_srv.port == 1
                                && main_srv.name == "主服"
                                && main_srv.id.starts_with("s_")
                        );
                        check!(
                            "servers reject duplicates and bad addresses",
                            servers::add(&sg, "x", "127.0.0.1:1", None, "").is_err()
                                && servers::add(&sg, "y", "", None, "").is_err()
                                && servers::add(&sg, "z", "bad host", None, "").is_err()
                        );
                        let renamed = servers::update(
                            &sg,
                            &live.id,
                            Some("改名了"),
                            None,
                            None,
                            Some("备注"),
                        )
                        .unwrap();
                        check!(
                            "servers update is selective",
                            renamed.name == "改名了"
                                && renamed.note == "备注"
                                && renamed.port == mc.port
                        );
                        let reordered =
                            servers::reorder(&sg, &[live.id.clone(), main_srv.id.clone()]).unwrap();
                        check!(
                            "servers reorder follows given ids",
                            reordered.len() == 2 && reordered[0].id == live.id
                        );
                        // 批量刷新：一个通一个不通，都不能拖垮对方
                        let all = servers::refresh(&sg, None);
                        let ok_one = all
                            .iter()
                            .find(|(id, _)| id == &live.id)
                            .map(|(_, s)| s.online && s.players_online == Some(7))
                            .unwrap_or(false);
                        let dead_one = all
                            .iter()
                            .find(|(id, _)| id == &main_srv.id)
                            .map(|(_, s)| !s.online && s.error.is_some())
                            .unwrap_or(false);
                        check!(
                            "servers refresh reports each independently",
                            all.len() == 2 && ok_one && dead_one
                        );
                        // 查到的那条要记 lastSeen，没查到的不能记
                        let after = servers::list(&sg);
                        let seen = after
                            .iter()
                            .find(|e| e.id == live.id)
                            .map(|e| e.last_seen.is_some())
                            .unwrap_or(false);
                        let unseen = after
                            .iter()
                            .find(|e| e.id == main_srv.id)
                            .map(|e| e.last_seen.is_none())
                            .unwrap_or(false);
                        check!("servers mark seen only on success", seen && unseen);
                        // 只刷新指定的一批
                        let only = servers::refresh(&sg, Some(vec![main_srv.id.clone()]));
                        check!(
                            "servers refresh respects id filter",
                            only.len() == 1 && only[0].0 == main_srv.id
                        );
                        // 删除只删记录
                        servers::remove(&sg, &main_srv.id).unwrap();
                        check!(
                            "servers remove drops one entry",
                            servers::list(&sg).len() == 1
                                && servers::remove(&sg, &main_srv.id).is_err()
                        );
                    }
                    _ => {
                        check!("servers add keeps host and port apart", false);
                        check!("servers reject duplicates and bad addresses", false);
                        check!("servers update is selective", false);
                        check!("servers reorder follows given ids", false);
                        check!("servers refresh reports each independently", false);
                        check!("servers mark seen only on success", false);
                        check!("servers refresh respects id filter", false);
                        check!("servers remove drops one entry", false);
                    }
                }
                let _ = std::fs::remove_dir_all(&sg);
            }

            // 21) 调度统计字段齐全，前端能直接用
            let s = net::scheduler_stats();
            check!(
                "scheduler stats shape",
                s["active"].is_number()
                    && s["maxActive"].as_u64() == Some(6)
                    && s["maxPerHost"].as_u64() == Some(3)
                    && s["downloads"].is_number()
                    && s["bytes"].is_number()
                    && s["resumed"].is_number()
                    && s["retries"].is_number()
                    && s["hosts"].is_object()
            );

            // 27) 回收站：删除 → 列表 → 恢复（id 冲突改名）→ 永久删除
            {
                let g = dl.join("trash-game");
                let _ = std::fs::create_dir_all(crate::instance::instances_root(&g));
                let meta = instance::InstanceMeta {
                    id: instance::new_id(),
                    name: "回收测试".into(),
                    game_version: "1.20.1".into(),
                    loader_kind: Some("fabric".into()),
                    loader_version: Some("0.16.9".into()),
                    launch_version: None,
                    memory_mb: None,
                    jvm_args: vec![],
                    java_path: None,
                    cover: None,
                    created_at: time::now_iso(),
                };
                let inst = instance::create(&g, &meta).unwrap();
                let orig_id = inst.id.clone();
                // 放一个存档文件，验证恢复后还在
                let save = crate::instance::instance_dir(&g, &orig_id)
                    .join("saves")
                    .join("world1")
                    .join("level.dat");
                let _ = std::fs::create_dir_all(save.parent().unwrap());
                let _ = std::fs::write(&save, b"level");

                let del = instance::delete(&g, &orig_id, true);
                let listed = recycle::list(&g);
                check!(
                    "trash lists deleted instance",
                    del.is_ok()
                        && listed.len() == 1
                        && listed[0].instance_id == orig_id
                        && listed[0].name == "回收测试"
                        && listed[0].restorable
                        && listed[0].file_count >= 2
                        && !listed[0].deleted_at.is_empty()
                );

                // id 冲突：先占住原 id，再恢复，必须换新 id 且不覆盖
                let _ = instance::create(
                    &g,
                    &instance::InstanceMeta {
                        id: orig_id.clone(),
                        name: "占位".into(),
                        game_version: "1.20.1".into(),
                        loader_kind: None,
                        loader_version: None,
                        launch_version: None,
                        memory_mb: None,
                        jvm_args: vec![],
                        java_path: None,
                        cover: None,
                        created_at: time::now_iso(),
                    },
                );
                let trash_name = listed[0].trash_name.clone();
                let restored = recycle::restore(&g, &trash_name);
                let new_id = restored
                    .as_ref()
                    .map(|r| r.instance_id.clone())
                    .unwrap_or_default();
                let placeholder_kept = instance::read_meta(&g, &orig_id)
                    .map(|m| m.name == "占位")
                    .unwrap_or(false);
                let save_back = crate::instance::instance_dir(&g, &new_id)
                    .join("saves")
                    .join("world1")
                    .join("level.dat");
                let meta_fixed = instance::read_meta(&g, &new_id)
                    .map(|m| m.id == new_id && m.name == "回收测试")
                    .unwrap_or(false);
                check!(
                    "trash restore renames on conflict",
                    restored.map(|r| r.renamed).unwrap_or(false)
                        && new_id != orig_id
                        && placeholder_kept
                        && save_back.is_file()
                        && meta_fixed
                );

                // 恢复后列表应为空
                check!("trash empty after restore", recycle::list(&g).is_empty());

                // 永久删除：必须显式确认
                let del2 = instance::delete(&g, &new_id, true);
                let names: Vec<String> = recycle::list(&g)
                    .iter()
                    .map(|e| e.trash_name.clone())
                    .collect();
                let purged = recycle::purge(&g, &names);
                check!(
                    "trash purge removes only listed",
                    del2.is_ok()
                        && purged
                            .map(|p| p.removed == 1 && p.failed.is_empty())
                            .unwrap_or(false)
                        && recycle::list(&g).is_empty()
                        && instance::read_meta(&g, &orig_id).is_some()
                );
            }

            // 28) 回收站拒绝路径穿越，清空也要能列出
            {
                let g = dl.join("trash-game");
                let bad = recycle::restore(&g, "../escape");
                let names: Vec<String> = recycle::list(&g)
                    .iter()
                    .map(|e| e.trash_name.clone())
                    .collect();
                let _ = names;
                check!("trash rejects traversal", bad.is_err());
            }

            // 29) 存档备份 → 预览 → 恢复（不覆盖 / 覆盖留底）
            {
                let g = dl.join("world-game");
                let saves = g.join("saves");
                let world = saves.join("MyWorld");
                let _ = std::fs::create_dir_all(world.join("region"));
                let _ = std::fs::write(world.join("level.dat"), b"LEVELDATA");
                let _ = std::fs::write(world.join("region").join("r.0.0.mca"), b"REGION");
                let inst_dir = g.join("instances").join("inst_world");
                let _ = std::fs::create_dir_all(&inst_dir);
                let wid = worlds::world_id("MyWorld");

                let made = world_backup::create(
                    &inst_dir,
                    &saves,
                    &wid,
                    "手动备份",
                    false,
                    &*net::never_cancel(),
                    |_, _| {},
                );
                let bid = made
                    .as_ref()
                    .map(|b| b.backup_id.clone())
                    .unwrap_or_default();
                let ok_make = made
                    .as_ref()
                    .map(|b| b.file_count == 2 && b.bytes > 0 && !b.sha256.is_empty())
                    .unwrap_or(false);
                let listed = world_backup::list(&inst_dir);
                check!(
                    "world backup creates zip",
                    ok_make && listed.len() == 1 && listed[0].note == "手动备份"
                );

                // 游戏运行中不允许备份
                let running = world_backup::create(
                    &inst_dir,
                    &saves,
                    &wid,
                    "",
                    true,
                    &*net::never_cancel(),
                    |_, _| {},
                );
                check!("world backup rejects while running", running.is_err());

                // 改掉存档内容，再从备份恢复（目标已存在 → 需要 overwrite）
                let _ = std::fs::write(world.join("level.dat"), b"CHANGED");
                let preview = world_backup::preview_restore(&inst_dir, &saves, &bid);
                let blocked_without = world_backup::restore(
                    &inst_dir,
                    &saves,
                    &bid,
                    false,
                    &*net::never_cancel(),
                    |_, _| {},
                );
                let restored = world_backup::restore(
                    &inst_dir,
                    &saves,
                    &bid,
                    true,
                    &*net::never_cancel(),
                    |_, _| {},
                );
                let level = std::fs::read_to_string(world.join("level.dat")).unwrap_or_default();
                let region = std::fs::read_to_string(world.join("region").join("r.0.0.mca"))
                    .unwrap_or_default();
                let kept = restored
                    .as_ref()
                    .ok()
                    .and_then(|r| r.replaced_backup.clone())
                    .map(|p| std::path::Path::new(&p).join("level.dat").is_file())
                    .unwrap_or(false);
                check!(
                    "world restore needs overwrite then restores",
                    preview
                        .map(|p| p.target_exists && p.blockers.is_empty())
                        .unwrap_or(false)
                        && blocked_without.is_err()
                        && restored.map(|r| r.restored_files == 2).unwrap_or(false)
                        && level == "LEVELDATA"
                        && region == "REGION"
                        && kept
                );

                // 备份文件被篡改后必须拒绝恢复
                let zip_path = inst_dir
                    .join(world_backup::BACKUP_DIR)
                    .join(format!("{bid}.zip"));
                let mut bytes = std::fs::read(&zip_path).unwrap_or_default();
                if let Some(b) = bytes.last_mut() {
                    *b ^= 0xff;
                }
                let _ = std::fs::write(&zip_path, &bytes);
                let tampered = world_backup::preview_restore(&inst_dir, &saves, &bid);
                check!(
                    "world restore rejects corrupted backup",
                    tampered.map(|p| !p.blockers.is_empty()).unwrap_or(false)
                );
            }

            // 30) 实例复制：只复制勾选类别，新实例独立
            {
                let g = dl.join("copy-game");
                let meta = instance::InstanceMeta {
                    id: instance::new_id(),
                    name: "源实例".into(),
                    game_version: "1.20.1".into(),
                    loader_kind: Some("fabric".into()),
                    loader_version: Some("0.16.9".into()),
                    launch_version: None,
                    memory_mb: Some(4096),
                    jvm_args: vec!["-XX:+UseG1GC".into()],
                    java_path: None,
                    cover: None,
                    created_at: time::now_iso(),
                };
                let src_id = instance::create(&g, &meta)
                    .map(|i| i.id)
                    .unwrap_or_default();
                let src_dir = instance::instance_dir(&g, &src_id);
                let _ = std::fs::write(src_dir.join("mods").join("a.jar"), b"MOD");
                let _ = std::fs::write(src_dir.join("config").join("c.json"), b"{}");
                let _ = std::fs::create_dir_all(src_dir.join("saves").join("w1"));
                let _ = std::fs::write(src_dir.join("saves").join("w1").join("level.dat"), b"L");
                let _ = std::fs::write(src_dir.join("resourcepacks").join("r.zip"), b"R");

                let stats_fn = |_: &str| instance::PlayStats::default();
                let plan = migrate::plan_copy(
                    &g,
                    &src_id,
                    "复制出来的",
                    &migrate::CopyOptions::settings_only(),
                    &stats_fn,
                );
                let plan_id = plan.as_ref().map(|p| p.plan_id.clone()).unwrap_or_default();
                let planned = plan
                    .as_ref()
                    .map(|p| p.total_files == 2 && p.items.len() == 2 && p.kind == "copy")
                    .unwrap_or(false);
                let done = migrate::execute(&g, &plan_id, &*net::never_cancel(), |_, _| {});
                let new_id = done
                    .as_ref()
                    .map(|r| r.instance_id.clone())
                    .unwrap_or_default();
                let new_dir = instance::instance_dir(&g, &new_id);
                let copied_mods = new_dir.join("mods").join("a.jar").is_file();
                let copied_config = new_dir.join("config").join("c.json").is_file();
                let no_worlds = !new_dir.join("saves").join("w1").join("level.dat").exists();
                let no_packs = !new_dir.join("resourcepacks").join("r.zip").exists();
                let meta_ok = instance::read_meta(&g, &new_id)
                    .map(|m| {
                        m.memory_mb == Some(4096) && m.loader_kind.as_deref() == Some("fabric")
                    })
                    .unwrap_or(false);
                check!(
                    "instance copy respects categories",
                    planned
                        && copied_mods
                        && copied_config
                        && no_worlds
                        && no_packs
                        && meta_ok
                        && src_dir.join("mods").join("a.jar").is_file()
                );

                let twice = migrate::execute(&g, &plan_id, &*net::never_cancel(), |_, _| {});
                check!("instance copy plan is single use", twice.is_err());
            }

            // 31) 本地版本导入：只写实例记录，不移动原文件
            {
                let g = dl.join("import-game");
                let vdir = g.join("versions").join("1.20.1-fabric0.16.9");
                let _ = std::fs::create_dir_all(&vdir);
                let _ = std::fs::write(
                    vdir.join("1.20.1-fabric0.16.9.json"),
                    br#"{"id":"fabric-loader-0.16.9-1.20.1","inheritsFrom":"1.20.1"}"#,
                );
                let _ = std::fs::write(vdir.join("1.20.1-fabric0.16.9.jar"), b"JAR");

                let scanned = migrate::scan_importable(&g);
                let plan = migrate::plan_import(&g, "1.20.1-fabric0.16.9", Some("导入实例"));
                let plan_id = plan.as_ref().map(|p| p.plan_id.clone()).unwrap_or_default();
                let planned = plan
                    .as_ref()
                    .map(|p| p.kind == "import" && p.total_files == 2)
                    .unwrap_or(false);
                let done = migrate::execute(&g, &plan_id, &*net::never_cancel(), |_, _| {});
                let inst_id = done
                    .as_ref()
                    .map(|r| r.instance_id.clone())
                    .unwrap_or_default();
                let meta = instance::read_meta(&g, &inst_id);
                check!(
                    "version import keeps original files",
                    scanned.len() == 1
                        && planned
                        && vdir.join("1.20.1-fabric0.16.9.jar").is_file()
                        && meta
                            .map(|m| m.name == "导入实例"
                                && m.game_version == "1.20.1-fabric0.16.9"
                                && m.loader_kind.as_deref() == Some("fabric"))
                            .unwrap_or(false)
                );
            }

            // 32) 复制兼容实例必须被拒绝（不能把共享 .minecraft 整个复制走）
            {
                let g = dl.join("import-game");
                let stats_fn = |_: &str| instance::PlayStats::default();
                let compat = instance::discover(&g, &stats_fn)
                    .into_iter()
                    .find(|i| !i.managed)
                    .map(|i| i.id);
                let r = compat
                    .map(|id| {
                        migrate::plan_copy(
                            &g,
                            &id,
                            "不该成功",
                            &migrate::CopyOptions::settings_only(),
                            &stats_fn,
                        )
                        .is_err()
                    })
                    .unwrap_or(false);
                check!("instance copy rejects shared versions", r);
            }
        }
        Err(e) => {
            check!("download plain + sha1", false);
            log.push_str(&format!("LIVE local test server unavailable: {e}\n"));
        }
    }
    let _ = std::fs::remove_dir_all(&dl);

    let _ = std::fs::remove_dir_all(&game);

    // 只统计 PASS/FAIL 行，避免把 LIVE 等诊断行算进断言数
    let total = log
        .lines()
        .filter(|l| l.starts_with("PASS ") || l.starts_with("FAIL "))
        .count();
    log.push_str(&format!("\n{total} checks, {fails} failed\n"));
    let json = serde_json::json!({
        "total": results.len(),
        "failed": fails,
        "checks": results.iter().map(|(n, ok)| serde_json::json!({"name": n, "ok": ok})).collect::<Vec<_>>(),
    });
    let _ = std::fs::write(
        std::env::temp_dir().join("astral-selftest.json"),
        serde_json::to_string_pretty(&json).unwrap_or_default(),
    );
    let _ = std::fs::write(std::env::temp_dir().join("astral-selftest.log"), &log);
    fails
}
