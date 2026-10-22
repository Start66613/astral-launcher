mod core;

use core::Config;
use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter};

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

#[tauri::command]
fn set_config(config: Config) -> Result<(), String> {
    core::save_config(&config)
}

#[tauri::command]
fn scan_versions() -> Vec<String> {
    core::scan_installed(std::path::Path::new(&core::load_config().game_dir))
}

#[tauri::command]
fn open_path(path: String) -> Result<(), String> {
    let p = PathBuf::from(&path);
    if !p.exists() {
        return Err(format!("路径不存在: {path}"));
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&p)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        return Err("仅支持 Windows".into());
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
    })
    .await
    .map_err(|e| e.to_string())?
}

struct DlgHandler<F: FnMut(u32, u32, u32, u32)>(F);

impl<F: FnMut(u32, u32, u32, u32)> portablemc::download::Handler for DlgHandler<F> {
    fn on_progress(&mut self, count: u32, total: u32, size: u32, total_size: u32) {
        (self.0)(count, total, size, total_size)
    }
}

/// 把 Modrinth 的文件下到对应的游戏子目录（mods / resourcepacks / shaderpacks）
#[tauri::command]
async fn download_to(
    app: AppHandle,
    url: String,
    kind: String,
    filename: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let cfg = core::load_config();
        let sub = match kind.as_str() {
            "resourcepack" => "resourcepacks",
            "shaderpack" => "shaderpacks",
            "modpack" => "modpacks",
            "datapack" => "datapacks",
            _ => "mods",
        };
        let dir = PathBuf::from(&cfg.game_dir).join(sub);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

        let safe = PathBuf::from(&filename)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "download.bin".into());
        let dest = dir.join(&safe);

        let mut single = portablemc::download::single(url, dest.clone());
        let name = safe.clone();
        let app2 = app.clone();
        let mut handler = DlgHandler(move |c: u32, tc: u32, s: u32, ts: u32| {
            let _ = app2.emit("astral://file", (&name, c, tc, s, ts));
        });
        single
            .download(&mut handler)
            .map_err(|e| format!("下载失败: {e:?}"))?;
        Ok(dest.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_system_info,
            get_config,
            set_config,
            scan_versions,
            open_path,
            list_versions,
            install_version,
            launch_version,
            download_to
        ])
        .run(tauri::generate_context!())
        .expect("error while running astral");
}
