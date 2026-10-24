//! 启动器核心：配置 / 版本列表 / 安装 / 启动。
//!
//! 安装与启动来自 `portablemc`（Apache-2.0），这里只做：读配置、
//! 拼路径、把 crate 的事件翻译成前端进度。

use portablemc::base::{Event, Game, Handler};
use portablemc::moj;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const VERSION_MANIFEST: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const BMCLAPI_MANIFEST: &str = "https://bmclapi2.bangbang93.com/mc/game/version_manifest_v2.json";

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    /// 游戏根目录
    pub game_dir: String,
    /// official | bmclapi
    pub source: String,
    /// 离线登录用的名字
    pub username: String,
    /// 当前选中实例（契约 v3 §3）
    pub active_instance_id: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            game_dir: default_game_dir().to_string_lossy().to_string(),
            source: "official".into(),
            username: "Player".into(),
            active_instance_id: None,
        }
    }
}

pub fn default_game_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(|p| PathBuf::from(p).join(".minecraft"))
        .unwrap_or_else(|| PathBuf::from(".minecraft"))
}

fn config_path() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(|p| PathBuf::from(p).join("Astral").join("config.json"))
        .unwrap_or_else(|| PathBuf::from("astral-config.json"))
}

pub fn load_config() -> Config {
    std::fs::read_to_string(config_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_config(cfg: &Config) -> Result<(), String> {
    let p = config_path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    // 先写 .part，再备份旧配置，最后替换：断电也不会留下半截 JSON
    let tmp = p.with_extension("json.part");
    std::fs::write(&tmp, &text).map_err(|e| e.to_string())?;
    if p.exists() {
        let _ = std::fs::copy(&p, p.with_extension("json.bak"));
    }
    std::fs::rename(&tmp, &p).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VersionSummary {
    pub id: String,
    pub kind: String,
    pub released: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub stage: String,
    pub label: String,
    pub done: u32,
    pub total: u32,
    pub bytes: u64,
    pub total_bytes: u64,
}

pub fn list_versions(source: &str) -> Result<Vec<VersionSummary>, String> {
    let url = if source == "bmclapi" {
        BMCLAPI_MANIFEST
    } else {
        VERSION_MANIFEST
    };
    let mut single = portablemc::download::single_cached(url);
    single
        .download(())
        .map_err(|e| format!("拉取版本清单失败: {e:?}"))?;

    let text = std::fs::read_to_string(single.file()).map_err(|e| e.to_string())?;
    let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let arr = json["versions"].as_array().ok_or("版本清单格式不对")?;

    Ok(arr
        .iter()
        .map(|v| VersionSummary {
            id: v["id"].as_str().unwrap_or_default().to_string(),
            kind: v["type"].as_str().unwrap_or_default().to_string(),
            released: v["releaseTime"].as_str().unwrap_or_default().to_string(),
        })
        .collect())
}

fn blank(stage: &str, label: &str) -> Progress {
    Progress {
        stage: stage.into(),
        label: label.into(),
        done: 0,
        total: 0,
        bytes: 0,
        total_bytes: 0,
    }
}

fn to_progress(event: &Event) -> Option<Progress> {
    Some(match event {
        Event::LoadHierarchy { root_version } => {
            blank("resolve", &format!("解析版本 {root_version}"))
        }
        Event::DownloadProgress {
            count,
            total_count,
            size,
            total_size,
        } => Progress {
            stage: "download".into(),
            label: format!("下载 {count}/{total_count}"),
            done: *count,
            total: *total_count,
            bytes: *size as u64,
            total_bytes: *total_size as u64,
        },
        Event::DownloadResources { .. } => blank("download", "准备下载文件"),
        Event::LoadAssets { id } => blank("assets", &format!("读取资源索引 {id}")),
        Event::LoadedAssets { count, .. } => blank("assets", &format!("校验 {count} 个资源文件")),
        Event::LoadedJvm { version, .. } => {
            blank("jvm", &format!("使用 Java {}", version.unwrap_or("")))
        }
        Event::LoadJvm { major_version } => blank("jvm", &format!("查找 Java {major_version}")),
        Event::ExtractedBinaries { .. } => blank("done", "解压完成"),
        _ => return None,
    })
}

struct Fwd<F: FnMut(Progress)>(F);

impl<F: FnMut(Progress)> Handler for Fwd<F> {
    fn on_event(&mut self, event: Event) {
        if let Some(p) = to_progress(&event) {
            (self.0)(p);
        }
    }
}

impl<F: FnMut(Progress)> portablemc::moj::Handler for Fwd<F> {
    fn on_event(&mut self, event: portablemc::moj::Event) {
        match event {
            portablemc::moj::Event::Base(e) => {
                if let Some(p) = to_progress(&e) {
                    (self.0)(p);
                }
            }
            portablemc::moj::Event::FetchVersion { version } => {
                (self.0)(blank("download", &format!("获取版本元数据 {version}")))
            }
            _ => {}
        }
    }
}

impl<F: FnMut(Progress)> portablemc::fabric::Handler for Fwd<F> {
    fn on_event(&mut self, event: portablemc::fabric::Event) {
        match event {
            portablemc::fabric::Event::Mojang(e) => match e {
                portablemc::moj::Event::Base(b) => {
                    if let Some(p) = to_progress(&b) {
                        (self.0)(p);
                    }
                }
                portablemc::moj::Event::FetchVersion { version } => {
                    (self.0)(blank("download", &format!("获取版本元数据 {version}")))
                }
                _ => {}
            },
            portablemc::fabric::Event::FetchVersion {
                game_version,
                loader_version,
            } => (self.0)(blank(
                "download",
                &format!("获取 Fabric {loader_version} / {game_version} 元数据"),
            )),
            _ => {}
        }
    }
}

fn apply_auth(inst: &mut moj::Installer, username: &str, saves: Option<&Path>) {
    // 已有存档若用的是旧 UUID 算法，继续沿用，避免玩家数据对不上
    let legacy = saves
        .map(|s| crate::account::has_legacy_identity(s, username))
        .unwrap_or(false);
    if legacy {
        inst.set_auth_offline(crate::account::legacy_uuid(username), username.to_string());
    } else {
        // 标准离线 UUID：MD5("OfflinePlayer:username")
        inst.set_auth_offline_username(username);
    }
}

fn base_installer(version: &str, dir: &Path, username: &str) -> moj::Installer {
    let mut inst = moj::Installer::new(version);
    let base = inst.base_mut();
    base.set_main_dir(dir);
    base.set_launcher_name("Astral");
    base.set_launcher_version("0.1.0");
    inst.set_auth_offline_username(username);
    inst
}

pub fn install(
    version: &str,
    dir: &Path,
    username: &str,
    on: impl FnMut(Progress),
) -> Result<Game, String> {
    portablemc::mirror::set_mirror(source_is_mirror(&load_config().source));
    let mut inst = base_installer(version, dir, username);
    inst.install(Fwd(on)).map_err(|e| format!("安装失败: {e:?}"))
}

pub fn source_is_mirror(source: &str) -> bool {
    source == "bmclapi"
}

/// 装 + 起进程，返回 Child（调用方负责 wait 与会话记录）
///
/// `main_dir` 是共享根（assets/libraries/versions），`mc_dir` 是游戏的 --gameDir，
/// 托管实例会指向实例目录，实现存档/配置隔离（契约 v3 §7）。
pub fn spawn(
    version: &str,
    main_dir: &Path,
    mc_dir: Option<&Path>,
    username: &str,
    memory_mb: Option<u32>,
    extra_args: &[String],
    on: impl FnMut(Progress),
) -> Result<std::process::Child, String> {
    spawn_with(
        version, None, main_dir, mc_dir, username, memory_mb, extra_args, None, on,
    )
}

/// 带加载器启动：`loader = Some((kind, loader_version))`
pub fn spawn_with(
    version: &str,
    loader: Option<(&str, &str)>,
    main_dir: &Path,
    mc_dir: Option<&Path>,
    username: &str,
    memory_mb: Option<u32>,
    extra_args: &[String],
    java_path: Option<&Path>,
    on: impl FnMut(Progress),
) -> Result<std::process::Child, String> {
    portablemc::mirror::set_mirror(source_is_mirror(&load_config().source));

    // 自定义 Java：显式指定后不再自动探测/下载 JVM
    let java_override: Option<PathBuf> = match java_path {
        Some(p) if p.is_file() => Some(p.to_path_buf()),
        Some(p) => {
            return Err(format!(
                "JAVA_MISSING|指定的 Java 不存在|{}",
                p.to_string_lossy()
            ))
        }
        None => None,
    };

    // 实例内存与自定义 JVM 参数（契约 v3 §7）
    let mut jvm: Vec<String> = Vec::new();
    if let Some(mb) = memory_mb {
        jvm.push(format!("-Xmx{mb}M"));
    }
    jvm.extend_from_slice(extra_args);

    match loader {
        Some((kind, loader_version)) => {
            let l = match kind {
                "fabric" => portablemc::fabric::Loader::Fabric,
                "quilt" => portablemc::fabric::Loader::Quilt,
                other => {
                    return Err(format!("UNSUPPORTED|暂不支持的加载器 {other}"));
                }
            };
            let mut inst = portablemc::fabric::Installer::new(l, version, loader_version);
            let moj = inst.mojang_mut();
            moj.base_mut().set_main_dir(main_dir);
            if let Some(dir) = mc_dir {
                moj.base_mut().set_mc_dir(dir);
            }
            moj.base_mut().set_launcher_name("Astral");
            moj.base_mut().set_launcher_version("0.1.0");
            if !jvm.is_empty() {
                moj.base_mut().set_extra_jvm_args(jvm);
            }
            if let Some(java) = &java_override {
                moj.base_mut().set_jvm_override(Some(java.clone()));
            }
            let saves = mc_dir
                .map(|d| d.join("saves"))
                .unwrap_or_else(|| main_dir.join("saves"));
            apply_auth(moj, username, Some(&saves));

            let game = inst
                .install(Fwd(on))
                .map_err(|e| format!("INTERNAL|加载器启动失败|{e:?}"))?;
            let mut cmd = game.command();
            cmd.spawn()
                .map_err(|e| format!("JAVA_MISSING|无法启动 Java|{e}"))
        }
        None => {
            let saves = mc_dir
                .map(|d| d.join("saves"))
                .unwrap_or_else(|| main_dir.join("saves"));
            let mut inst = base_installer(version, main_dir, username);
            if let Some(dir) = mc_dir {
                inst.base_mut().set_mc_dir(dir);
            }
            if !jvm.is_empty() {
                inst.base_mut().set_extra_jvm_args(jvm);
            }
            if let Some(java) = &java_override {
                inst.base_mut().set_jvm_override(Some(java.clone()));
            }
            apply_auth(&mut inst, username, Some(&saves));
            let game = inst
                .install(Fwd(on))
                .map_err(|e| format!("INTERNAL|启动失败|{e:?}"))?;
            let mut cmd = game.command();
            cmd.spawn()
                .map_err(|e| format!("JAVA_MISSING|无法启动 Java|{e}"))
        }
    }
}

/// legacy：装 + 启动，返回 PID
pub fn launch(
    version: &str,
    dir: &Path,
    username: &str,
    on: impl FnMut(Progress),
) -> Result<u32, String> {
    Ok(spawn(version, dir, None, username, None, &[], on)?.id())
}

/// 只安装加载器（不启动），把版本文件补齐到共享 versions/
pub fn install_loader(
    game_version: &str,
    loader_kind: &str,
    loader_version: &str,
    main_dir: &Path,
    username: &str,
    on: impl FnMut(Progress),
) -> Result<(), String> {
    portablemc::mirror::set_mirror(source_is_mirror(&load_config().source));
    let l = match loader_kind {
        "fabric" => portablemc::fabric::Loader::Fabric,
        "quilt" => portablemc::fabric::Loader::Quilt,
        other => return Err(format!("UNSUPPORTED|暂不支持的加载器 {other}")),
    };
    let mut inst = portablemc::fabric::Installer::new(l, game_version, loader_version);
    let moj = inst.mojang_mut();
    moj.base_mut().set_main_dir(main_dir);
    moj.base_mut().set_launcher_name("Astral");
    moj.base_mut().set_launcher_version("0.1.0");
    moj.set_auth_offline_username(username);
    inst.install(Fwd(on))
        .map(|_| ())
        .map_err(|e| format!("INTERNAL|加载器安装失败|{e:?}"))
}

/// 判断某个版本是否已完整安装（有版本 json 和 client jar）
pub fn version_installed(dir: &Path, version: &str) -> bool {
    let v = dir.join("versions").join(version);
    v.join(format!("{version}.json")).exists() && v.join(format!("{version}.jar")).exists()
}

pub fn scan_installed(dir: &Path) -> Vec<String> {
    let Ok(read) = std::fs::read_dir(dir.join("versions")) else {
        return Vec::new();
    };
    let mut out: Vec<String> = read
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter(|e| {
            e.path()
                .join(format!("{}.json", e.file_name().to_string_lossy()))
                .exists()
        })
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// cargo test --release -- --ignored --nocapture
    #[test]
    #[ignore]
    fn list_all_versions() {
        let v = list_versions("official").expect("拉取清单");
        assert!(v.len() > 100, "版本数量异常: {}", v.len());
        println!("✓ {} 个版本，最新一个是 {}", v.len(), v[0].id);
    }

    #[test]
    #[ignore]
    fn list_from_bmclapi() {
        let v = list_versions("bmclapi").expect("拉取镜像清单");
        println!("✓ 镜像清单 {} 个版本，最新 {} ", v.len(), v[0].id);
        assert!(v.len() > 100);
    }

    #[test]
    #[ignore]
    fn install_old_version() {
        let dir = std::env::temp_dir().join("astral-mc-test");
        let t = Instant::now();
        let mut last = String::new();
        install("b1.7.3", &dir, "Player", |p| {
            if p.label != last {
                println!("  · {}", p.label);
                last = p.label;
            }
        })
        .expect("安装 b1.7.3");
        println!("✓ 安装完成，用时 {:?}", t.elapsed());
        assert!(version_installed(&dir, "b1.7.3"));
    }

    /// 单文件探测：确认镜像改写生效、文件真的能下
    #[test]
    #[ignore]
    fn mirror_probe() {
        use portablemc::download::single;
        portablemc::mirror::set_mirror(true);
        let dir = std::env::temp_dir().join("astral-mirror-probe");
        std::fs::create_dir_all(&dir).unwrap();
        let libs = [
            "https://libraries.minecraft.net/org/ow2/asm/asm/9.3/asm-9.3.jar",
            "https://libraries.minecraft.net/com/google/guava/guava/31.1-jre/guava-31.1-jre.jar",
            "https://libraries.minecraft.net/org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1.jar",
            "https://resources.download.minecraft.net/b6/b62ca8ec10d07e6bf5ac8dae0c8c1d2e6a1e3356",
        ];
        for (i, u) in libs.iter().enumerate() {
            let mut s = single(*u, dir.join(format!("f{i}.bin")));
            match s.download(()) {
                Ok(ok) => println!("✓ {} bytes\n   <- {}", ok.size(), ok.url()),
                Err(e) => println!("✗ {}\n   err: {e:?}", u),
            }
        }
        portablemc::mirror::set_mirror(false);
    }

    /// 用 BMCLAPI 镜像装一遍，验证改写规则真的能下到文件
    #[test]
    #[ignore]
    fn install_via_mirror() {
        let dir = std::env::temp_dir().join("astral-mc-mirror");
        let old = load_config();
        let mut cfg = old.clone();
        cfg.source = "bmclapi".into();
        cfg.game_dir = dir.to_string_lossy().to_string();
        save_config(&cfg).expect("写配置");

        let t = Instant::now();
        let res = install("b1.7.3", &dir, "Player", |_| {});
        let ok = version_installed(&dir, "b1.7.3");
        println!("✓ 镜像安装用时 {:?}", t.elapsed());

        save_config(&old).expect("恢复配置");
        res.expect("镜像安装");
        assert!(ok, "镜像装完但文件不齐");
    }

    /// 真正启动一次：起进程 → 等 12 秒 → 确认还活着 → 杀掉
    #[test]
    #[ignore]
    fn launch_b1_7_3() {
        let dir = std::env::temp_dir().join("astral-mc-test");
        assert!(version_installed(&dir, "b1.7.3"), "先跑 install_old_version");
        let t = Instant::now();
        let pid = launch("b1.7.3", &dir, "Astral", |_| {}).expect("启动");
        println!("✓ 已启动 PID {pid}，准备用 {:?}", t.elapsed());

        std::thread::sleep(std::time::Duration::from_secs(12));

        #[cfg(target_os = "windows")]
        {
            let out = std::process::Command::new("tasklist")
                .args(["/FI", &format!("PID eq {pid}")])
                .output()
                .expect("tasklist");
            let text = String::from_utf8_lossy(&out.stdout);
            println!("{}", text);
            assert!(text.contains(&pid.to_string()), "进程已经退出了");
            let _ = std::process::Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/F"])
                .output();
        }
    }
}
