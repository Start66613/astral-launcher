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
    /// 微软登录用的 Azure 应用 ID（BYOK；未配置时 MSA 不可用）
    pub msa_client_id: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            game_dir: default_game_dir().to_string_lossy().to_string(),
            source: "official".into(),
            username: "Player".into(),
            active_instance_id: None,
            msa_client_id: None,
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
    list_versions_cached(source).map(|(v, _)| v)
}

/// 版本清单 + 数据来源说明（`__cache` 里是 network / cache / stale）。
///
/// 断网时用上次成功的清单，并在返回值里明确写清数据是什么时候抓的。
pub fn list_versions_cached(
    source: &str,
) -> Result<(Vec<VersionSummary>, serde_json::Value), String> {
    let url = if source == "bmclapi" {
        BMCLAPI_MANIFEST
    } else {
        VERSION_MANIFEST
    };
    let json =
        crate::net::fetch_json_cached(url, None).map_err(|e| format!("拉取版本清单失败: {e}"))?;
    let arr = json["versions"].as_array().ok_or("版本清单格式不对")?;
    let cache = json
        .get("__cache")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let list = arr
        .iter()
        .map(|v| VersionSummary {
            id: v["id"].as_str().unwrap_or_default().to_string(),
            kind: v["type"].as_str().unwrap_or_default().to_string(),
            released: v["releaseTime"].as_str().unwrap_or_default().to_string(),
        })
        .collect();
    Ok((list, cache))
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
    inst.install(Fwd(on))
        .map_err(|e| format!("安装失败: {e:?}"))
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
#[allow(clippy::too_many_arguments)]
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
/// Forge / NeoForge 的 installer 下载地址
/// NeoForge 版本号前缀 → 对应的 Minecraft 版本（官方约定，不是猜的）
pub fn neoforge_mc_hint(ver: &str) -> Option<&'static str> {
    if ver.starts_with("20.2.") {
        Some("1.20.2")
    } else if ver.starts_with("20.4.") {
        Some("1.20.4")
    } else if ver.starts_with("20.6.") {
        Some("1.20.6")
    } else if ver.starts_with("21.0.") {
        Some("1.21")
    } else if ver.starts_with("21.1.") {
        Some("1.21.1")
    } else {
        None
    }
}

/// 加载器版本与游戏版本是否匹配（装之前就挡掉明显不搭的组合）
pub fn loader_compatible(kind: &str, mc: &str, ver: &str) -> Result<(), String> {
    match kind {
        "forge" => {
            // 这里收到的是裸版本号（如 47.2.0），maven 坐标才是 {mc}-{ver}；
            // 若已经带了前缀会拼出 1.20.1-1.20.1-47.2.0，提前挡掉
            if ver.starts_with(&format!("{mc}-")) {
                return Err(format!(
                    "INVALID_VALUE|Forge 版本号不需要带游戏版本前缀，收到 {ver}"
                ));
            }
            if !ver
                .chars()
                .next()
                .map(|c| c.is_ascii_digit())
                .unwrap_or(false)
            {
                return Err(format!("INVALID_VALUE|Forge 版本号格式不对：{ver}"));
            }
            Ok(())
        }
        "neoforge" if mc == "1.20.1" => {
            if !ver.starts_with("47.1.") {
                return Err(format!(
                    "INCOMPATIBLE|1.20.1 上的 NeoForge 应使用 47.1.x，收到 {ver}"
                ));
            }
            Ok(())
        }
        "neoforge" => {
            if let Some(hint) = neoforge_mc_hint(ver) {
                if hint != mc {
                    return Err(format!(
                        "INCOMPATIBLE|NeoForge {ver} 对应 Minecraft {hint}，不是 {mc}"
                    ));
                }
            }
            // 前缀未知时不拦：宁可让下载给出明确错误，也不误伤新版本
            Ok(())
        }
        _ => Ok(()),
    }
}

pub fn installer_url(kind: &str, mc: &str, ver: &str) -> Result<String, String> {
    loader_compatible(kind, mc, ver)?;
    let url = match kind {
        "forge" => format!(
            "https://maven.minecraftforge.net/net/minecraftforge/forge/{mc}-{ver}/forge-{mc}-{ver}-installer.jar"
        ),
        "neoforge" if mc == "1.20.1" => format!(
            "https://maven.neoforged.net/releases/net/neoforged/forge/1.20.1-{ver}/forge-1.20.1-{ver}-installer.jar"
        ),
        "neoforge" => format!(
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/{ver}/neoforge-{ver}-installer.jar"
        ),
        other => return Err(format!("UNSUPPORTED|未知的安装器类型 {other}")),
    };
    Ok(url)
}

/// 找一个可用的 java：JAVA_HOME → 共享 jvm 目录 → PATH
pub fn find_java(main_dir: &Path) -> Result<std::path::PathBuf, String> {
    let mut cands: Vec<std::path::PathBuf> = Vec::new();
    if let Some(home) = std::env::var_os("JAVA_HOME") {
        cands.push(std::path::PathBuf::from(home).join("bin").join("java.exe"));
    }
    if let Ok(rd) = std::fs::read_dir(main_dir.join("jvm")) {
        for e in rd.flatten() {
            for sub in ["bin/java.exe", "bin/javaw.exe"] {
                let p = e.path().join(sub);
                if p.is_file() {
                    cands.push(p);
                }
            }
        }
    }
    for c in cands {
        if c.is_file() {
            return Ok(c);
        }
    }
    if std::process::Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        return Ok(std::path::PathBuf::from("java"));
    }
    Err("JAVA_MISSING|找不到可用的 Java，无法运行安装器".into())
}

/// 从环境变量取代理（http://host:port 或 host:port）
fn proxy_from_env() -> Option<(String, String)> {
    for key in ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy"] {
        if let Ok(raw) = std::env::var(key) {
            let s = raw
                .trim()
                .trim_start_matches("http://")
                .trim_start_matches("https://");
            let s = s.trim_end_matches('/');
            if let Some((host, port)) = s.rsplit_once(':') {
                if !host.is_empty() && port.chars().all(|c| c.is_ascii_digit()) {
                    return Some((host.to_string(), port.to_string()));
                }
            }
        }
    }
    None
}

/// 修复计划：返回需要补做的步骤（只补缺失，不动已有文件）
///
/// - `base`：游戏本体（版本 json / jar / 依赖库 / 资源索引任一缺失）
/// - `loader`：加载器版本目录缺失（例如 Forge 的版本 json 被删）
pub fn repair_steps(game: &Path, meta: &crate::instance::InstanceMeta) -> Vec<&'static str> {
    let mut steps = Vec::new();

    let version = meta
        .launch_version
        .clone()
        .unwrap_or_else(|| meta.game_version.clone());
    let health = crate::instance::health_for_test(game, &version);
    if !health.ok {
        // 版本链本身缺东西 → 重跑安装（已存在的文件会跳过）
        steps.push("base");
    }

    if let Some(kind) = &meta.loader_kind {
        let done = match meta.launch_version.as_deref() {
            Some(lv) => game
                .join("versions")
                .join(lv)
                .join(format!("{lv}.json"))
                .is_file(),
            None => false,
        };
        if !done {
            steps.push("loader");
        }
        let _ = kind;
    }

    steps
}
/// 启动前自动挑一个 Java：按版本要求的主版本扫描本机运行时
///
/// 显式配置的 javaPath 优先；这里只在没有配置时使用。
pub fn auto_java_for(game_dir: &Path, version: &str) -> Option<std::path::PathBuf> {
    let runtimes = crate::java::scan(game_dir);
    crate::java::required_major(game_dir, version)
        .and_then(|req| crate::java::pick_for(&runtimes, req))
        .map(|r| std::path::PathBuf::from(r.path))
}
/// installer 会生成的 version id（用于"已装过就复用"与多实例共享判定）
pub fn forge_version_id(kind: &str, mc: &str, ver: &str) -> String {
    match kind {
        "neoforge" if mc != "1.20.1" => format!("neoforge-{ver}"),
        _ => format!("{mc}-forge-{ver}"),
    }
}

/// 用官方 installer 安装 Forge / NeoForge，返回生成的 version id
pub fn install_forge_like(
    kind: &str,
    mc: &str,
    ver: &str,
    main_dir: &Path,
    mut on: impl FnMut(Progress),
) -> Result<String, String> {
    // 已装过就复用：多个实例共享同一份加载器文件，重复安装不该报错
    let expected = forge_version_id(kind, mc, ver);
    let expected_json = main_dir
        .join("versions")
        .join(&expected)
        .join(format!("{expected}.json"));
    if expected_json.is_file() {
        on(blank("install", &format!("{kind} {ver} 已安装，复用")));
        return Ok(expected);
    }

    let url = installer_url(kind, mc, ver)?;

    let cache = main_dir.join(".astral").join("installers");
    std::fs::create_dir_all(&cache).map_err(|e| format!("PERMISSION|创建缓存目录失败|{e}"))?;
    let dest = cache.join(format!("{kind}-{mc}-{ver}-installer.jar"));

    if !dest.is_file() {
        on(blank("download", &format!("下载 {kind} 安装器")));
        let mut progress = |c: u32, t: u32, _s: u32, _ts: u32| {
            on(blank("download", &format!("下载安装器 {c}/{t}")));
        };
        crate::net::download_atomic(&url, &dest, None, &mut progress)?;
    }

    let before = scan_installed(main_dir);
    let java = find_java(main_dir)?;

    // Forge/NeoForge 安装器要求目标目录里有 launcher_profiles.json，否则直接退出
    let profiles = main_dir.join("launcher_profiles.json");
    if !profiles.is_file() {
        let _ = std::fs::write(&profiles, r#"{"profiles":{},"settings":{},"version":3}"#);
    }

    on(blank("install", &format!("运行 {kind} 安装器")));

    // 安装器可重入（已下载的库会跳过），网络抖动时重试一次往往就过了
    let mut last_tail = String::new();
    let mut succeeded = false;
    for attempt in 0..2u32 {
        let mut cmd = std::process::Command::new(&java);
        // JVM 参数必须在 -jar 之前，否则会被安装器当成自己的参数
        if let Some((host, port)) = proxy_from_env() {
            for scheme in ["http", "https"] {
                cmd.arg(format!("-D{scheme}.proxyHost={host}"))
                    .arg(format!("-D{scheme}.proxyPort={port}"));
            }
        }
        cmd.arg("-jar")
            .arg(&dest)
            .arg("--installClient")
            .arg(main_dir);

        let output = cmd
            .current_dir(main_dir)
            .output()
            .map_err(|e| format!("JAVA_MISSING|无法运行安装器|{e}"))?;

        if output.status.success() {
            succeeded = true;
            break;
        }

        // 失败：清掉这次写出来的半成品版本目录
        for v in scan_installed(main_dir) {
            if !before.contains(&v) {
                let _ = std::fs::remove_dir_all(main_dir.join("versions").join(&v));
            }
        }
        let err = String::from_utf8_lossy(&output.stderr);
        let out = String::from_utf8_lossy(&output.stdout);
        let tail: Vec<String> = err
            .lines()
            .chain(out.lines())
            .filter(|l| !l.trim().is_empty())
            .rev()
            .take(4)
            .map(|s| s.to_string())
            .collect();
        last_tail = tail.join(" / ");

        if attempt == 0 {
            on(blank("install", &format!("{kind} 安装器失败，重试一次")));
            std::thread::sleep(std::time::Duration::from_millis(1500));
        }
    }

    if !succeeded {
        return Err(format!("INTERNAL|{kind} 安装器执行失败|{last_tail}"));
    }

    // 安装器成功：优先认它应该生成的 id（可能已被别的实例装过），再看差集
    if expected_json.is_file() {
        return Ok(expected);
    }
    let created: Vec<String> = scan_installed(main_dir)
        .into_iter()
        .filter(|v| !before.contains(v))
        .collect();

    created
        .iter()
        .find(|v| v.to_ascii_lowercase().contains(kind))
        .cloned()
        .or_else(|| created.first().cloned())
        .ok_or_else(|| format!("INTERNAL|{kind} 安装器没有生成新版本|检查 Java 与目录权限"))
}

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
        assert!(
            version_installed(&dir, "b1.7.3"),
            "先跑 install_old_version"
        );
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
