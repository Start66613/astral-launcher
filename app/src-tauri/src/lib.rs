mod components;
mod core;
mod error;
mod account;
mod instance;
mod java;
mod loader;
mod logs;
mod modpack;
mod mods;
mod net;
mod plan;
mod resources;
mod worlds;
mod session;
mod stats;
mod task;
mod time;
mod updates;

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
    instance::find(&game, &id, &play_stats)
        .ok_or_else(|| "INTERNAL|保存后读不到实例".to_string())
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
async fn start_install(
    app: AppHandle,
    game_version: String,
) -> Result<Task, String> {
    let t = task::create(
        "install",
        None,
        Some(game_version.clone()),
        format!("安装 {game_version}"),
    );
    let tid = t.task_id.clone();
    let app2 = app.clone();
    std::thread::spawn(move || {
        task::start(&tid);
        let cfg = core::load_config();
        let dir = PathBuf::from(&cfg.game_dir);
        let t2 = tid.clone();
        let v = game_version.clone();
        // 后台线程 panic 必须变成任务失败，不能让任务永远 running
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            core::install(&game_version, &dir, &cfg.username, move |p| {
                task::progress(&t2, &p.stage, p.label, p.done, p.total);
            })
        }))
        .unwrap_or_else(|_| Err("INTERNAL|后台任务异常|安装线程崩溃".to_string()));
        match res {
            // 活儿已经干完才收到取消：如实报告完成，不假装已取消
            Ok(_) if task::is_cancelled(&tid) => task::finish(
                &tid,
                serde_json::json!({ "kind": "install", "gameVersion": v, "cancelledLate": true }),
            ),
            Ok(_) => task::finish(&tid, serde_json::json!({ "kind": "install", "gameVersion": v })),
            Err(_) if task::is_cancelled(&tid) => task::cancelled(&tid),
            Err(e) => {
                task::fail(&tid, e);
            }
        }
        let _ = app2;
    });
    Ok(t)
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
        let is_installer_loader = matches!(inst.loader.as_ref().map(|l| l.kind.as_str()), Some("forge") | Some("neoforge"));
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
        let spawned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            core::spawn_with(
                &version,
                loader.as_ref().map(|(k, v)| (k.as_str(), v.as_str())),
                &main_dir,
                mc_dir.as_deref(),
                &cfg.username,
                inst.memory_mb,
                &inst.jvm_args,
                inst.java_path.as_deref().map(std::path::Path::new),
                move |p| task::progress(&t2, &p.stage, p.label, p.done, p.total),
            )
        }))
        .unwrap_or_else(|_| Err("INTERNAL|后台任务异常|启动线程崩溃".to_string()));

        match spawned {
            Ok(mut child) => {
                let pid = child.id();
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
    let t = task::create("resource-install", None, None, title);
    let tid = t.task_id.clone();
    std::thread::spawn(move || {
        task::start(&tid);
        let t2 = tid.clone();
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            resources::execute(&stored, move |c, total| {
                task::progress(&t2, "download", format!("下载 {c}/{total}"), c, total);
            })
        }))
        .unwrap_or_else(|_| Err("INTERNAL|后台任务异常|资源安装线程崩溃".to_string()));

        match res {
            Ok(path) => task::finish(
                &tid,
                serde_json::json!({
                    "kind": "resource-install",
                    "path": path.to_string_lossy(),
                    "file": stored.plan.file_name,
                }),
            ),
            Err(_) if task::is_cancelled(&tid) => task::cancelled(&tid),
            Err(e) => task::fail(&tid, e),
        }
    });
    Ok(t)
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
    let t = task::create(
        "mod-update",
        Some(instance_id.clone()),
        None,
        format!("更新 {file_name}"),
    );
    let tid = t.task_id.clone();
    std::thread::spawn(move || {
        task::start(&tid);
        let game = game_root();
        let t2 = tid.clone();
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            updates::apply_update(
                &game,
                &instance_id,
                &file_name,
                &project_id,
                &version_id,
                move |c, t, _s, _ts| {
                    task::progress(&t2, "download", format!("下载 {c}/{t}"), c, t);
                },
            )
        }))
        .unwrap_or_else(|_| Err("INTERNAL|后台任务异常|更新线程崩溃".to_string()));

        match res {
            Ok((new_file, removed)) => task::finish(
                &tid,
                serde_json::json!({ "kind": "mod-update", "file": new_file, "removed": removed }),
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
    match cfg.msa_client_id.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        Some(id) => MsaStatus {
            configured: true,
            client_id_masked: Some(format!("{}…{}", &id[..8.min(id.len())], &id[id.len().saturating_sub(4)..])),
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

#[tauri::command]
async fn start_modpack_install(plan_id: String, revision: u32) -> Result<Task, String> {
    let stored = modpack::take(&plan_id, revision)?;
    if let Some(first) = stored.plan.issues.iter().find(|i| i.blocking) {
        return Err(format!("{}|{}", first.code, first.message));
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

                if !core::version_installed(&game, &index.mc_version) {
                    let t2 = tid.clone();
                    core::install(&index.mc_version, &game, &cfg.username, move |p| {
                        task::progress(&t2, &p.stage, format!("游戏本体 {}", p.label), p.done, p.total);
                    })?;
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
                        core::install_loader(
                            &index.mc_version,
                            kind,
                            ver,
                            &game,
                            &cfg.username,
                            move |p| task::progress(&t2, &p.stage, p.label, p.done, p.total),
                        )?;
                    }
                }
                task::check_cancel(&tid)?;

                let inst_dir = instance::instance_dir(&game, &inst.id);
                let t2 = tid.clone();
                modpack::download_files(&index, &inst_dir, move |d, total| {
                    task::progress(&t2, "download", format!("整合包文件 {d}/{total}"), d, total);
                })?;
                task::check_cancel(&tid)?;

                let t3 = tid.clone();
                modpack::extract_overrides(&stored.pack_path, &inst_dir, move |d, _| {
                    task::progress(&t3, "extract", format!("解压覆盖文件 {d}"), d, 0);
                })?;

                let fresh = instance::find(&game, &inst.id, &play_stats)
                    .ok_or_else(|| "INTERNAL|安装后读不到实例".to_string())?;
                Ok(fresh)
            },
        ));

        match result {
            Ok(Ok(inst)) => {
                emit_instance_changed(&inst.id, "created");
                task::finish(
                    &tid,
                    serde_json::json!({ "kind": "modpack", "instance": inst }),
                );
            }
            Ok(Err(_)) if task::is_cancelled(&tid) => {
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

    let t = task::create(
        "game-install",
        None,
        Some(p.request.game_version.clone()),
        format!("安装 {}", p.request.name),
    );
    let tid = t.task_id.clone();

    std::thread::spawn(move || {
        task::start(&tid);
        let game = game_root();
        let cfg = core::load_config();
        let v = p.request.game_version.clone();
        let mut created: Option<String> = None;

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<Instance, String> {
            if !core::version_installed(&game, &v) {
                let t2 = tid.clone();
                core::install(&v, &game, &cfg.username, move |pr| {
                    task::progress(&t2, &pr.stage, pr.label, pr.done, pr.total);
                })?;
            }
            task::check_cancel(&tid)?;

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
            task::check_cancel(&tid)?;

            if let Some(l) = &p.request.loader {
                let t2 = tid.clone();
                if l.kind == "forge" || l.kind == "neoforge" {
                    // installer 会生成 {mc}-forge-{ver} 之类的版本，记下来供启动使用
                    let generated = core::install_forge_like(&l.kind, &v, &l.version, &game, move |pr| {
                        task::progress(&t2, &pr.stage, pr.label, pr.done, pr.total);
                    })?;
                    let mut meta = instance::read_meta(&game, &inst.id)
                        .ok_or_else(|| "INSTANCE_NOT_FOUND|实例元数据丢失".to_string())?;
                    meta.launch_version = Some(generated);
                    instance::save_meta(&game, &meta)?;
                } else {
                    core::install_loader(&v, &l.kind, &l.version, &game, &cfg.username, move |pr| {
                        task::progress(&t2, &pr.stage, pr.label, pr.done, pr.total);
                    })?;
                }
            }
            task::check_cancel(&tid)?;

            let fresh = instance::find(&game, &inst.id, &play_stats)
                .ok_or_else(|| "INTERNAL|安装后读不到实例".to_string())?;
            if !fresh.health.ok {
                return Err(format!(
                    "VERIFY_FAILED|安装完成但文件不完整|{}",
                    fresh.health.reasons.join("; ")
                ));
            }
            Ok(fresh)
        }));

        match result {
            Ok(Ok(inst)) => {
                emit_instance_changed(&inst.id, "created");
                task::finish(
                    &tid,
                    serde_json::json!({ "kind": "game-install", "instance": inst }),
                );
            }
            Ok(Err(_)) if task::is_cancelled(&tid) => {
                if let Some(id) = &created {
                    let _ = instance::delete(&game, id, false);
                }
                task::cancelled(&tid);
            }
            Ok(Err(e)) => {
                // 失败回滚：只删本次新建的实例目录
                if let Some(id) = &created {
                    let _ = instance::delete(&game, id, false);
                }
                task::fail(&tid, e);
            }
            Err(_) => {
                if let Some(id) = &created {
                    let _ = instance::delete(&game, id, false);
                }
                task::fail(&tid, "INTERNAL|后台任务异常|安装线程崩溃".into());
            }
        }
    });

    Ok(t)
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
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<Option<String>, String> {
            if kind == "forge" || kind == "neoforge" {
                // installer 产出的 version id 要记进实例，启动时用它
                core::install_forge_like(&kind, &inst.game_version, &version, &main_dir, move |p| {
                    task::progress(&t2, &p.stage, p.label, p.done, p.total);
                })
                .map(Some)
            } else {
                core::install_loader(
                    &inst.game_version,
                    &kind,
                    &version,
                    &main_dir,
                    &cfg.username,
                    move |p| task::progress(&t2, &p.stage, p.label, p.done, p.total),
                )
                .map(|_| None)
            }
        }))
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

struct DlgHandler<F: FnMut(u32, u32, u32, u32)>(F);

impl<F: FnMut(u32, u32, u32, u32)> portablemc::download::Handler for DlgHandler<F> {
    fn on_progress(&mut self, count: u32, total: u32, size: u32, total_size: u32) {
        (self.0)(count, total, size, total_size)
    }
}

#[tauri::command]
async fn download_to(
    app: AppHandle,
    url: String,
    kind: String,
    filename: String,
    instance_id: Option<String>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let cfg = core::load_config();
        let game = PathBuf::from(&cfg.game_dir);
        let sub = match kind.as_str() {
            "resourcepack" => "resourcepacks",
            "shaderpack" => "shaderpacks",
            "modpack" => "modpacks",
            "datapack" => "datapacks",
            _ => "mods",
        };
        // 指定实例时落到实例目录（契约 v3 §5.7）
        let dir = match instance_id {
            Some(id) => {
                let inst = instance::find(&game, &id, &play_stats)
                    .ok_or_else(|| "INSTANCE_NOT_FOUND|实例不存在".to_string())?;
                PathBuf::from(inst.dir).join(sub)
            }
            None => game.join(sub),
        };

        let safe = PathBuf::from(&filename)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "download.bin".into());

        // 目标必须落在游戏目录内（防空目录穿越与 junction 逃逸），且先落 .part
        let dest = net::ensure_within(&game, &dir.join(&safe))?;

        let name = safe.clone();
        let app2 = app.clone();
        net::download_atomic(
            &url,
            &dest,
            None,
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
            get_tasks,
            get_task,
            cancel_task,
            start_install,
            start_launch,
            start_create_instance,
            prepare_game_install,
            start_game_install,
            get_active_session,
            list_sessions,
            list_saves,
            get_stats,
            list_mods,
            set_mod_enabled,
            delete_mod,
            list_loaders,
            start_install_loader,
            list_loader_builds,
            list_target_worlds,
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
    macro_rules! check {
        ($name:expr, $cond:expr) => {{
            if $cond {
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
        time::iso_to_unix_ms(iso).map(time::unix_ms_to_iso).as_deref() == Some(iso)
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
    check!("mod path guard", mods::delete(&mod_dir, "../a.jar").is_err());
    let _ = std::fs::remove_dir_all(&mod_dir);

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
    check!("task snapshot", !task::snapshot(None, None, None).is_empty());

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

    let save = game.join("saves").join("World1");
    let _ = std::fs::create_dir_all(&save);
    let _ = std::fs::write(save.join("level.dat"), "x");
    check!("saves listed", stats::list_saves(&game, None).len() == 1);

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
        p_bad.issues.iter().any(|i| i.blocking && i.code == "INVALID_NAME")
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
    check!("plan revision guard", plan::take(&p_rev.plan_id, 2).is_err());
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
            let r = core::spawn_with(
                parts[1],
                loader.as_ref().map(|(k, v)| (k.as_str(), v.as_str())),
                &dir,
                Some(&dir),
                "Steve",
                Some(2048),
                &[],
                None,
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
        let mut spec = dir.split(|c| c == '|');
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
            let p = resources::execute(&stored, |_, _| {})?;
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
                modpack::install(&stored, &game, "Steve", |_s, _l, _d, _t| {})
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
            std::fs::create_dir_all(saves.join("TestWorld"))
                .map_err(|e| e.to_string())?;
            std::fs::write(saves.join("TestWorld/level.dat"), b"x").map_err(|e| e.to_string())?;
            let world = worlds::list(saves).into_iter().next()
                .ok_or_else(|| "INTERNAL|测试存档没被识别".to_string())?;

            let mut out = Vec::new();
            for (kind, pid, wid) in [
                ("resourcepack", "w0TnApzs", None),
                ("shaderpack", "HVnmMxH1", None),
                ("datapack", "8oi3bsk5", Some(world.world_id.as_str())),
            ] {
                match resources::live_probe(&game, &inst.id, kind, pid, wid, "1.20.1", Some("fabric")) {
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
                            |_, _, _, _| {},
                        ) {
                            Ok((new, removed)) => log.push_str(&format!(
                                "LIVE update-apply ok: {removed} -> {new}\n"
                            )),
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

    let cfg = core::Config::default();
    check!(
        "config defaults",
        cfg.active_instance_id.is_none() && !cfg.game_dir.is_empty()
    );

    let _ = std::fs::remove_dir_all(&game);

    // 只统计 PASS/FAIL 行，避免把 LIVE 等诊断行算进断言数
    let total = log
        .lines()
        .filter(|l| l.starts_with("PASS ") || l.starts_with("FAIL "))
        .count();
    log.push_str(&format!("\n{total} checks, {fails} failed\n"));
    let _ = std::fs::write(std::env::temp_dir().join("astral-selftest.log"), &log);
    fails
}
