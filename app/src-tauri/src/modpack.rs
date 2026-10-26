//! Modrinth 整合包（.mrpack）解析与安装（契约 V2 §8）。
//!
//! 安全要点：
//! - 只接受 Modrinth 的 CDN 下载地址（走 net 白名单）；
//! - 解压前校验每个路径，拒绝绝对路径、`..`、盘符与反斜杠；
//! - 默认新建实例，不覆盖已有实例目录、不碰已有存档。

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

const API: &str = "https://api.modrinth.com/v2";

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModpackInfo {
    pub name: String,
    pub summary: String,
    pub version_name: String,
    pub mc_version: String,
    pub loader_kind: Option<String>,
    pub loader_version: Option<String>,
    pub file_count: u32,
    pub total_bytes: u64,
    /// 需要装的客户端文件数（不含 optional / unsupported）
    pub client_files: u32,
    /// 可选文件数（玩家勾选后才装）
    pub optional_files: u32,
    /// 明确声明不支持客户端的文件数（永远不装）
    pub unsupported_files: u32,
    /// 只对服务端有意义的文件数（env.client = unsupported）
    pub server_only_files: u32,
    /// overrides / client-overrides 里的条目数（只统计，安装时才解压）
    pub override_entries: u32,
    /// 安装来源：`modrinth` 或 `local`
    pub source: String,
    /// Modrinth 项目 id（本地导入的包为空）
    pub project_id: String,
    /// Modrinth 版本 id（本地导入的包为空）
    pub version_id: String,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModpackRequest {
    pub project_id: String,
    pub version_id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub memory_mb: Option<u32>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModpackPlan {
    pub plan_id: String,
    pub revision: u32,
    pub expires_at: String,
    pub info: ModpackInfo,
    pub issues: Vec<crate::plan::PlanIssue>,
    pub download_bytes: u64,
}

pub struct StoredModpackPlan {
    pub plan: ModpackPlan,
    pub pack_path: PathBuf,
    /// 从请求带下来，安装实例时写入 instance.json
    pub memory_mb: Option<u32>,
    pub created_ms: i64,
}

static PLANS: Mutex<Option<HashMap<String, StoredModpackPlan>>> = Mutex::new(None);

use std::collections::HashMap;
use std::sync::Mutex;

const TTL_MS: i64 = 30 * 60 * 1000;

fn store() -> std::sync::MutexGuard<'static, Option<HashMap<String, StoredModpackPlan>>> {
    PLANS.lock().unwrap()
}

fn issue(code: &str, message: impl Into<String>, blocking: bool) -> crate::plan::PlanIssue {
    crate::plan::PlanIssue {
        code: code.to_string(),
        message: message.into(),
        blocking,
    }
}

/// index 里描述的一个待下载文件
#[derive(Clone, Debug)]
pub struct IndexFile {
    pub path: String,
    pub url: String,
    /// 预留给进度与校验展示，暂未参与计算
    #[allow(dead_code)]
    pub size: u64,
    pub sha1: Option<String>,
    pub sha512: Option<String>,
    /// mrpack 的 `env.client`：required / optional / unsupported
    pub env_client: String,
    /// mrpack 的 `env.server`：required / optional / unsupported
    pub env_server: String,
}

impl IndexFile {
    /// 客户端要不要装这个文件
    ///
    /// - `unsupported`：明确不支持客户端 → 跳过
    /// - `optional`：可选 → 默认跳过，玩家勾了才装
    /// - 其他：装
    pub fn wanted_on_client(&self, include_optional: bool) -> bool {
        match self.env_client.as_str() {
            "unsupported" => false,
            "optional" => include_optional,
            _ => true,
        }
    }

    pub fn is_optional(&self) -> bool {
        self.env_client == "optional"
    }
}

#[derive(Clone, Debug, Default)]
pub struct ModpackIndex {
    pub name: String,
    pub summary: String,
    pub mc_version: String,
    pub loader_kind: Option<String>,
    pub loader_version: Option<String>,
    pub files: Vec<IndexFile>,
    pub total_bytes: u64,
    /// 需要装的客户端文件数（不含 optional/unsupported）
    pub client_files: u32,
    /// 可选文件数
    pub optional_files: u32,
    /// 明确不支持客户端的文件数
    pub unsupported_files: u32,
    /// 只对服务端有意义的文件数
    pub server_only_files: u32,
    /// 整合包内 overrides 条目数（只统计，不解压）
    pub override_entries: u32,
}

/// 校验 index 里的相对路径：必须相对、无 `..`、无反斜杠、无盘符
pub fn safe_rel_path(path: &str) -> Result<PathBuf, String> {
    if path.is_empty() {
        return Err("INVALID_PATH|路径为空".into());
    }
    if path.contains('\\') {
        return Err(format!("INVALID_PATH|路径包含反斜杠：{path}"));
    }
    if path.contains(':') {
        return Err(format!("INVALID_PATH|路径包含盘符：{path}"));
    }
    let p = Path::new(path);
    if p.is_absolute() {
        return Err(format!("INVALID_PATH|不允许绝对路径：{path}"));
    }
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            _ => return Err(format!("INVALID_PATH|路径包含上跳或根：{path}")),
        }
    }
    if out.as_os_str().is_empty() {
        return Err("INVALID_PATH|路径解析为空".into());
    }
    Ok(out)
}

/// 解析 modrinth.index.json
pub fn parse_index(json: &str) -> Result<ModpackIndex, String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("INTERNAL|index 不是合法 JSON|{e}"))?;

    let deps = v.get("dependencies").cloned().unwrap_or_default();
    let mc = deps
        .get("minecraft")
        .and_then(|x| x.as_str())
        .ok_or_else(|| "INTERNAL|index 缺少 minecraft 版本".to_string())?
        .to_string();

    let (loader_kind, loader_version) = if let Some(v) = deps.get("fabric-loader") {
        ("fabric".to_string(), v.as_str().map(|s| s.to_string()))
    } else if let Some(v) = deps.get("quilt-loader") {
        ("quilt".to_string(), v.as_str().map(|s| s.to_string()))
    } else if let Some(v) = deps.get("forge") {
        ("forge".to_string(), v.as_str().map(|s| s.to_string()))
    } else if let Some(v) = deps.get("neoforge") {
        ("neoforge".to_string(), v.as_str().map(|s| s.to_string()))
    } else {
        (String::new(), None)
    };

    if !loader_kind.is_empty() && loader_version.is_none() {
        return Err(format!(
            "INVALID_VALUE|整合包声明了 {loader_kind} 加载器但没有版本号"
        ));
    }

    let mut files = Vec::new();
    let mut total = 0u64;
    let mut seen_paths: std::collections::HashSet<String> = std::collections::HashSet::new();
    if let Some(arr) = v.get("files").and_then(|f| f.as_array()) {
        for f in arr {
            let path = f
                .get("path")
                .and_then(|p| p.as_str())
                .ok_or_else(|| "INTERNAL|文件缺少 path".to_string())?;
            // 路径先校验，坏路径直接拒绝整个整合包
            safe_rel_path(path)?;
            let url = f
                .get("downloads")
                .and_then(|d| d.as_array())
                .and_then(|a| a.first())
                .and_then(|u| u.as_str())
                .unwrap_or_default()
                .to_string();
            if url.is_empty() {
                return Err(format!("INVALID_PATH|文件缺少下载地址：{path}"));
            }
            if !seen_paths.insert(path.to_ascii_lowercase()) {
                return Err(format!("INVALID_VALUE|整合包内路径重复：{path}"));
            }
            let size = f.get("fileSize").and_then(|s| s.as_u64()).unwrap_or(0);
            total = total.saturating_add(size);
            // 64 GiB：正常整合包远低于这个量级，超过基本可以确定是坏包
            if total > 64 * 1024 * 1024 * 1024 {
                return Err("INVALID_VALUE|整合包声明体积异常（超过 64 GiB）".into());
            }
            // env 只认三个合法值，其他一律当 required（保守：宁可装也别漏）
            let env_of = |key: &str| -> String {
                match f
                    .get("env")
                    .and_then(|e| e.get(key))
                    .and_then(|v| v.as_str())
                {
                    Some("unsupported") => "unsupported".into(),
                    Some("optional") => "optional".into(),
                    _ => "required".into(),
                }
            };
            files.push(IndexFile {
                path: path.to_string(),
                url,
                size,
                sha512: f
                    .get("hashes")
                    .and_then(|h| h.get("sha512"))
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_string()),
                sha1: f
                    .get("hashes")
                    .and_then(|h| h.get("sha1"))
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_string()),
                env_client: env_of("client"),
                env_server: env_of("server"),
            });
        }
    }

    if files.is_empty() {
        return Err("INVALID_VALUE|整合包没有任何可安装文件".into());
    }

    let client_files = files.iter().filter(|f| f.wanted_on_client(false)).count() as u32;
    let optional_files = files.iter().filter(|f| f.is_optional()).count() as u32;
    let unsupported_files = files
        .iter()
        .filter(|f| f.env_client == "unsupported")
        .count() as u32;
    let server_only_files = files
        .iter()
        .filter(|f| f.env_server != "unsupported" && f.env_client == "unsupported")
        .count() as u32;

    Ok(ModpackIndex {
        name: v
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or("整合包")
            .to_string(),
        summary: v
            .get("summary")
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string(),
        mc_version: mc,
        loader_kind: if loader_kind.is_empty() {
            None
        } else {
            Some(loader_kind)
        },
        loader_version,
        total_bytes: total,
        files,
        client_files,
        optional_files,
        unsupported_files,
        server_only_files,
        override_entries: 0,
    })
}

/// 从 .mrpack 里读 index
pub fn read_index(pack: &Path) -> Result<ModpackIndex, String> {
    let file = std::fs::File::open(pack).map_err(|e| format!("PERMISSION|打不开整合包|{e}"))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("INTERNAL|不是有效的 zip|{e}"))?;
    let mut entry = zip
        .by_name("modrinth.index.json")
        .map_err(|_| "INTERNAL|整合包缺少 modrinth.index.json".to_string())?;
    if entry.size() > 8 * 1024 * 1024 {
        return Err("INTERNAL|index 过大，拒绝解析".into());
    }
    let mut text = String::new();
    entry
        .read_to_string(&mut text)
        .map_err(|e| format!("INTERNAL|读取 index 失败|{e}"))?;
    let mut index = parse_index(&text)?;
    // 顺便数一下 overrides 条目（只数名字，不解压）
    index.override_entries = count_overrides(pack).unwrap_or(0);
    Ok(index)
}

/// 数一下 overrides / client-overrides 里有多少条目
fn count_overrides(pack: &Path) -> Result<u32, String> {
    let file = std::fs::File::open(pack).map_err(|e| format!("PERMISSION|打不开整合包|{e}"))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("INTERNAL|不是有效 zip|{e}"))?;
    let mut n = 0u32;
    for i in 0..zip.len() {
        if let Ok(entry) = zip.by_index(i) {
            let name = entry.name().map(|c| c.to_string()).unwrap_or_default();
            if (name.starts_with("overrides/") || name.starts_with("client-overrides/"))
                && !name.ends_with('/')
            {
                n += 1;
            }
        }
    }
    Ok(n)
}

/// 规划安装：下载 mrpack 并解析，不创建实例
pub fn prepare(game: &Path, req: ModpackRequest) -> Result<ModpackPlan, String> {
    let mut issues = Vec::new();

    // 取版本信息（文件名与下载地址）
    let ver = crate::net::fetch_json(&format!("{API}/version/{}", req.version_id))?;
    let file = ver
        .get("files")
        .and_then(|f| f.as_array())
        .and_then(|a| a.first())
        .ok_or_else(|| "VERSION_NOT_FOUND|该整合包版本没有文件".to_string())?;
    // 版本必须属于请求的项目，避免装错东西
    if let Some(vp) = ver.get("project_id").and_then(|x| x.as_str()) {
        if vp != req.project_id {
            return Err("INVALID_VALUE|版本与项目不匹配".into());
        }
    }
    let url = file
        .get("url")
        .and_then(|u| u.as_str())
        .ok_or_else(|| "INTERNAL|缺少下载地址".to_string())?;
    let filename = file
        .get("filename")
        .and_then(|f| f.as_str())
        .unwrap_or("pack.mrpack");

    let cache = game.join(".astral").join("modpacks");
    std::fs::create_dir_all(&cache).map_err(|e| format!("PERMISSION|创建缓存目录失败|{e}"))?;
    let safe_name = Path::new(filename)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "pack.mrpack".into());
    let pack_path = cache.join(&safe_name);

    if !pack_path.is_file() {
        crate::net::download_atomic(url, &pack_path, None, |_, _, _, _| {})?;
    }

    let index = read_index(&pack_path)?;

    if index.loader_kind.is_none() {
        issues.push(issue(
            "UNSUPPORTED",
            "整合包没有声明加载器，只能按原版安装".to_string(),
            false,
        ));
    }
    for f in &index.files {
        if crate::net::check_url(&f.url).is_err() {
            issues.push(issue(
                "BLOCKED_URL",
                format!("文件来源不在白名单：{}", f.path),
                true,
            ));
        }
    }

    let info = ModpackInfo {
        name: req.name.clone().unwrap_or_else(|| index.name.clone()),
        summary: index.summary.clone(),
        version_name: ver
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or_default()
            .to_string(),
        mc_version: index.mc_version.clone(),
        loader_kind: index.loader_kind.clone(),
        loader_version: index.loader_version.clone(),
        file_count: index.files.len() as u32,
        total_bytes: index.total_bytes,
        client_files: index.client_files,
        optional_files: index.optional_files,
        unsupported_files: index.unsupported_files,
        server_only_files: index.server_only_files,
        override_entries: index.override_entries,
        source: "modrinth".to_string(),
        project_id: req.project_id.clone(),
        version_id: req.version_id.clone(),
    };

    let plan = ModpackPlan {
        plan_id: format!("mplan_{:x}", crate::time::now_unix_ms()),
        revision: 1,
        expires_at: crate::time::unix_ms_to_iso(crate::time::now_unix_ms() + TTL_MS),
        info: info.clone(),
        issues,
        download_bytes: index.total_bytes,
    };

    let mut s = store();
    let map = s.get_or_insert_with(HashMap::new);
    let now = crate::time::now_unix_ms();
    map.retain(|_, p| now - p.created_ms < TTL_MS);
    map.insert(
        plan.plan_id.clone(),
        StoredModpackPlan {
            plan: plan.clone(),
            pack_path,
            memory_mb: req.memory_mb,
            created_ms: now,
        },
    );
    Ok(plan)
}

/// 本地 .mrpack 导入：把玩家选中的文件复制进受管缓存再解析
///
/// 严格限制：
/// - 必须是 `.mrpack` 扩展名、普通文件、不是符号链接；
/// - 大小上限 512 MiB（正常整合包远小于此）；
/// - 只读取 zip 的 index 与条目名，不解压到任意位置；
/// - 复制到 `{game}/.astral/modpacks/local-<hash>.mrpack`，不改动原文件。
pub const MAX_LOCAL_PACK: u64 = 512 * 1024 * 1024;

pub fn prepare_local(game: &Path, path: &str, name: Option<String>) -> Result<ModpackPlan, String> {
    let src = Path::new(path);
    if !src.is_file() {
        return Err("NOT_FOUND|找不到这个整合包文件".into());
    }
    let ext_ok = src
        .extension()
        .map(|e| e.to_string_lossy().eq_ignore_ascii_case("mrpack"))
        .unwrap_or(false);
    if !ext_ok {
        return Err("INVALID_VALUE|只支持 .mrpack 整合包".into());
    }
    let meta = std::fs::metadata(src).map_err(|e| format!("PERMISSION|读取文件信息失败|{e}"))?;
    if !meta.is_file() {
        return Err("INVALID_VALUE|不是普通文件".into());
    }
    if meta.len() > MAX_LOCAL_PACK {
        return Err(format!(
            "INVALID_VALUE|整合包过大（{} MB），上限 {} MB",
            meta.len() / 1024 / 1024,
            MAX_LOCAL_PACK / 1024 / 1024
        ));
    }
    // 符号链接：读到的可能是任意系统文件，直接拒绝
    if crate::net::is_reparse_point(src) {
        return Err("INVALID_PATH|不接受符号链接".into());
    }

    let cache = game.join(".astral").join("modpacks");
    std::fs::create_dir_all(&cache).map_err(|e| format!("PERMISSION|创建缓存目录失败|{e}"))?;
    // 用内容哈希命名，同一个包重复导入不会重复占空间
    let sha1 = crate::net::sha1_file(src).map_err(|e| format!("PERMISSION|读取整合包失败|{e}"))?;
    let short: String = sha1.chars().take(16).collect();
    let pack_path = cache.join(format!("local-{short}.mrpack"));
    if !pack_path.is_file() {
        let tmp = cache.join(format!("local-{short}.mrpack.part"));
        std::fs::copy(src, &tmp).map_err(|e| format!("PERMISSION|复制整合包失败|{e}"))?;
        if std::fs::rename(&tmp, &pack_path).is_err() {
            let _ = std::fs::remove_file(&tmp);
            return Err("PERMISSION|保存整合包失败".into());
        }
    }

    let index = read_index(&pack_path)?;
    let mut issues = Vec::new();
    if index.loader_kind.is_none() {
        issues.push(issue(
            "UNSUPPORTED",
            "整合包没有声明加载器，只能按原版安装".to_string(),
            false,
        ));
    }
    for f in &index.files {
        if crate::net::check_url(&f.url).is_err() {
            issues.push(issue(
                "BLOCKED_URL",
                format!("文件来源不在白名单：{}", f.path),
                true,
            ));
        }
    }

    let display = name
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| index.name.clone());
    let info = ModpackInfo {
        name: display,
        summary: index.summary.clone(),
        version_name: src
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default(),
        mc_version: index.mc_version.clone(),
        loader_kind: index.loader_kind.clone(),
        loader_version: index.loader_version.clone(),
        file_count: index.files.len() as u32,
        total_bytes: index.total_bytes,
        client_files: index.client_files,
        optional_files: index.optional_files,
        unsupported_files: index.unsupported_files,
        server_only_files: index.server_only_files,
        override_entries: index.override_entries,
        source: "local".to_string(),
        project_id: String::new(),
        version_id: String::new(),
    };

    let plan = ModpackPlan {
        plan_id: format!("mplan_{:x}", crate::time::now_unix_ms()),
        revision: 1,
        expires_at: crate::time::unix_ms_to_iso(crate::time::now_unix_ms() + TTL_MS),
        info,
        issues,
        download_bytes: index.total_bytes,
    };

    let mut s = store();
    let map = s.get_or_insert_with(HashMap::new);
    let now = crate::time::now_unix_ms();
    map.retain(|_, p| now - p.created_ms < TTL_MS);
    map.insert(
        plan.plan_id.clone(),
        StoredModpackPlan {
            plan: plan.clone(),
            pack_path,
            memory_mb: None,
            created_ms: now,
        },
    );
    Ok(plan)
}

pub fn take(plan_id: &str, revision: u32) -> Result<StoredModpackPlan, String> {
    let mut s = store();
    let map = s
        .as_mut()
        .ok_or_else(|| "PLAN_EXPIRED|没有可用计划".to_string())?;
    let stored = map
        .remove(plan_id)
        .ok_or_else(|| "PLAN_EXPIRED|计划不存在或已使用".to_string())?;
    let now = crate::time::now_unix_ms();
    if now - stored.created_ms >= TTL_MS {
        return Err("PLAN_EXPIRED|计划已过期".into());
    }
    if stored.plan.revision != revision {
        return Err("PLAN_EXPIRED|计划已变更".into());
    }
    Ok(stored)
}

/// 解压 overrides / client-overrides 到实例目录，逐条校验路径
pub fn extract_overrides(
    pack: &Path,
    instance_dir: &Path,
    should_cancel: crate::net::CancelRef<'_>,
    mut on: impl FnMut(u32, u32),
) -> Result<u32, String> {
    const MAX_TOTAL: u64 = 512 * 1024 * 1024;
    /// 单个文件上限，防 zip 炸弹
    const MAX_ENTRY: u64 = 256 * 1024 * 1024;
    /// 条目数上限，防"几百万个小文件"拖死磁盘
    const MAX_ENTRIES: usize = 20000;

    let file = std::fs::File::open(pack).map_err(|e| format!("PERMISSION|打不开整合包|{e}"))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("INTERNAL|不是有效 zip|{e}"))?;

    if zip.len() > MAX_ENTRIES {
        return Err(format!(
            "INVALID_VALUE|整合包条目过多（{} 个），已拒绝",
            zip.len()
        ));
    }

    let mut count = 0u32;
    let mut written = 0u64;
    let mut names: Vec<String> = Vec::new();
    for i in 0..zip.len() {
        if let Ok(entry) = zip.by_index(i) {
            let entry_name = entry.name().map(|c| c.to_string()).unwrap_or_default();
            // 符号链接条目一律拒绝：否则解压出来可能指到实例目录外面
            if let Some(mode) = entry.unix_mode() {
                if mode & 0o170000 == 0o120000 {
                    return Err(format!(
                        "INVALID_PATH|整合包含符号链接条目，已拒绝：{entry_name}"
                    ));
                }
            }
            if entry.size() > MAX_ENTRY {
                return Err(format!(
                    "INVALID_VALUE|整合包内单个文件过大，已拒绝：{entry_name}"
                ));
            }
            if !entry_name.is_empty() {
                names.push(entry_name);
            }
        }
    }

    for name in names {
        if should_cancel() {
            return Err("CANCELLED|已取消".into());
        }
        let rel = if let Some(r) = name.strip_prefix("overrides/") {
            r
        } else if let Some(r) = name.strip_prefix("client-overrides/") {
            r
        } else {
            continue;
        };
        if rel.is_empty() || rel.ends_with('/') {
            continue;
        }
        let safe = safe_rel_path(rel)?;
        // 真正的越界检查：父目录必须落在实例目录内，且目标不能是符号链接
        let dest = crate::net::ensure_within(instance_dir, &instance_dir.join(&safe))?;
        let mut entry = zip
            .by_name(&name)
            .map_err(|e| format!("INTERNAL|读取条目失败|{e}"))?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("PERMISSION|创建目录失败|{e}"))?;
        }
        // 先写临时文件再改名，中途失败不会留下半个覆盖文件
        let tmp = PathBuf::from(format!("{}.part", dest.to_string_lossy()));
        let mut out =
            std::fs::File::create(&tmp).map_err(|e| format!("PERMISSION|写入失败|{e}"))?;
        let copied =
            std::io::copy(&mut entry, &mut out).map_err(|e| format!("PERMISSION|解压失败|{e}"))?;
        drop(out);
        // 按实际写出的字节算总量，不信压缩包自称的大小
        written = written.saturating_add(copied);
        if written > MAX_TOTAL {
            let _ = std::fs::remove_file(&tmp);
            return Err("INVALID_VALUE|解压体积异常，已中止".into());
        }
        if std::fs::rename(&tmp, &dest).is_err() {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("PERMISSION|写入文件失败|{}", dest.display()));
        }
        count += 1;
        on(count, 0);
    }
    Ok(count)
}

/// 下载 index 里的全部文件到实例目录。
///
/// 每个文件下载期间都可以被取消；取消时保留 `.part` 以便续传。
pub fn download_files(
    index: &ModpackIndex,
    instance_dir: &Path,
    include_optional: bool,
    should_cancel: crate::net::CancelRef<'_>,
    mut on: impl FnMut(u32, u32),
) -> Result<u32, String> {
    let wanted: Vec<&IndexFile> = index
        .files
        .iter()
        .filter(|f| f.wanted_on_client(include_optional))
        .collect();
    let total = wanted.len() as u32;
    let cancel = should_cancel;
    let mut done = 0u32;
    for f in wanted {
        if should_cancel() {
            return Err("CANCELLED|已取消".into());
        }
        crate::net::validate_url(&f.url)?;
        let rel = safe_rel_path(&f.path)?;
        let dest = instance_dir.join(&rel);
        if !dest.starts_with(instance_dir) {
            return Err(format!("INVALID_PATH|下载路径越界：{}", f.path));
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("PERMISSION|创建目录失败|{e}"))?;
        }
        crate::net::download_with_cancel(
            &f.url,
            &dest,
            f.sha1.as_deref(),
            f.sha512.as_deref(),
            cancel,
            |_, _| {},
        )?;
        done += 1;
        on(done, total);
    }
    Ok(done)
}

/// 安装前磁盘预检：目标盘剩余空间要够放"还没下载的文件 + overrides 解压"
///
/// 返回 `(需要字节, 可用字节)`。可用空间拿不到时返回 `None`（不阻断，只提示）。
pub fn disk_precheck(index: &ModpackIndex, instance_dir: &Path) -> (u64, Option<u64>) {
    let needed = index.total_bytes;
    let disks = sysinfo::Disks::new_with_refreshed_list();
    // Windows 上 canonicalize 会带 \\?\ 前缀，和 sysinfo 给的 C:\ 对不上，
    // 所以两边都规范化后再比；规范化失败就退回原路径比。
    let canon = |p: &Path| -> PathBuf {
        let c = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        let s = c.to_string_lossy().to_string();
        match s.strip_prefix(r"\\?\") {
            Some(rest) => PathBuf::from(rest),
            None => PathBuf::from(s),
        }
    };
    let target = canon(instance_dir);
    let mut best: Option<(usize, u64)> = None;
    for d in disks.list() {
        let mp = canon(d.mount_point());
        if target.starts_with(&mp) {
            let depth = mp.components().count();
            if best.map(|(bd, _)| depth >= bd).unwrap_or(true) {
                best = Some((depth, d.available_space()));
            }
        }
    }
    (needed, best.map(|(_, a)| a))
}

/// 记录实例是从哪个整合包来的（写在实例目录的 `.astral-pack.json`）
///
/// 用来回答"这个包装过哪些版本""这个实例属于哪个整合包"，
/// 以及未来做"整合包更新"时判断新旧版本关系。
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct PackProvenance {
    pub project_id: String,
    pub version_id: String,
    pub version_name: String,
    /// `modrinth` 或 `local`
    pub source: String,
    pub pack_name: String,
    pub installed_at: String,
}

pub const PROVENANCE_FILE: &str = ".astral-pack.json";

pub fn read_provenance(instance_dir: &Path) -> Option<PackProvenance> {
    let p = instance_dir.join(PROVENANCE_FILE);
    let text = std::fs::read_to_string(p).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn write_provenance(instance_dir: &Path, prov: &PackProvenance) -> Result<(), String> {
    let p = instance_dir.join(PROVENANCE_FILE);
    let text = serde_json::to_string_pretty(prov)
        .map_err(|e| format!("INTERNAL|序列化整合包来源失败|{e}"))?;
    let tmp = instance_dir.join(format!("{PROVENANCE_FILE}.part"));
    std::fs::write(&tmp, text).map_err(|e| format!("PERMISSION|写入整合包来源失败|{e}"))?;
    std::fs::rename(&tmp, &p).map_err(|e| format!("PERMISSION|保存整合包来源失败|{e}"))?;
    Ok(())
}

/// 同一个整合包在本地装过哪些版本（只读扫描实例目录，不联网）
pub fn installed_versions(game: &Path, project_id: &str) -> Vec<serde_json::Value> {
    let root = crate::instance::instances_root(game);
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(&root) else {
        return out;
    };
    for e in rd.flatten() {
        let dir = e.path();
        if !dir.is_dir() {
            continue;
        }
        let Some(prov) = read_provenance(&dir) else {
            continue;
        };
        if prov.project_id != project_id {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        out.push(serde_json::json!({
            "instanceId": name,
            "versionId": prov.version_id,
            "versionName": prov.version_name,
            "installedAt": prov.installed_at,
            "source": prov.source,
        }));
    }
    out.sort_by(|a, b| {
        b.get("installedAt")
            .and_then(|v| v.as_str())
            .cmp(&a.get("installedAt").and_then(|v| v.as_str()))
    });
    out
}

/// 执行安装：建实例 → 游戏本体 → 加载器 → 下载文件 → 解压覆盖 → 健康检查
pub fn install(
    stored: &StoredModpackPlan,
    game: &Path,
    username: &str,
    should_cancel: impl Fn() -> bool + Send + Sync,
    mut on: impl FnMut(&str, &str, u32, u32),
) -> Result<String, String> {
    let index = read_index(&stored.pack_path)?;
    let meta = crate::instance::InstanceMeta {
        id: crate::instance::new_id(),
        name: stored.plan.info.name.clone(),
        game_version: index.mc_version.clone(),
        loader_kind: index.loader_kind.clone(),
        loader_version: index.loader_version.clone(),
        launch_version: None,
        memory_mb: stored.memory_mb,
        jvm_args: vec![],
        java_path: None,
        cover: None,
        created_at: crate::time::now_iso(),
    };
    let inst = crate::instance::create(game, &meta)?;
    let inst_id = inst.id.clone();

    if should_cancel() {
        let _ = crate::instance::delete(game, &inst_id, false);
        return Err("CANCELLED|已取消".into());
    }

    let mut run = || -> Result<(), String> {
        if !crate::core::version_installed(game, &index.mc_version) {
            crate::core::install(&index.mc_version, game, username, |p| {
                on("install", &format!("游戏本体 {}", p.label), p.done, p.total)
            })?;
        }
        if let (Some(kind), Some(ver)) = (&index.loader_kind, &index.loader_version) {
            if kind == "forge" || kind == "neoforge" {
                let generated =
                    crate::core::install_forge_like(kind, &index.mc_version, ver, game, |p| {
                        on(&p.stage, &p.label, p.done, p.total)
                    })?;
                let mut m = crate::instance::read_meta(game, &inst_id)
                    .ok_or_else(|| "INSTANCE_NOT_FOUND|元数据丢失".to_string())?;
                m.launch_version = Some(generated);
                crate::instance::save_meta(game, &m)?;
            } else {
                crate::core::install_loader(&index.mc_version, kind, ver, game, username, |p| {
                    on(&p.stage, &p.label, p.done, p.total)
                })?;
            }
        }
        let dir = crate::instance::instance_dir(game, &inst_id);
        if should_cancel() {
            return Err("CANCELLED|已取消".into());
        }
        download_files(&index, &dir, false, &should_cancel, |d, total| {
            on("download", "整合包文件", d, total)
        })?;
        extract_overrides(&stored.pack_path, &dir, &should_cancel, |d, _| {
            on("extract", "解压覆盖文件", d, 0)
        })?;
        // 记下来源，供"同一个包装了哪些版本"使用
        let prov = PackProvenance {
            project_id: stored.plan.info.project_id.clone(),
            version_id: stored.plan.info.version_id.clone(),
            version_name: stored.plan.info.version_name.clone(),
            source: stored.plan.info.source.clone(),
            pack_name: stored.plan.info.name.clone(),
            installed_at: crate::time::now_iso(),
        };
        let _ = write_provenance(&dir, &prov);
        Ok(())
    };

    if let Err(e) = run() {
        // 回滚：本次新建的实例目录
        let _ = crate::instance::delete(game, &inst_id, false);
        return Err(e);
    }
    Ok(inst_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_guard() {
        assert!(safe_rel_path("mods/a.jar").is_ok());
        assert!(safe_rel_path("../evil.jar").is_err());
        assert!(safe_rel_path("/abs/a.jar").is_err());
        assert!(safe_rel_path("C:/win.jar").is_err());
        assert!(safe_rel_path("mods\\a.jar").is_err());
        assert!(safe_rel_path("./mods/../x").is_err());
    }

    #[test]
    fn safe_rel_path_rejects_traversal() {
        for bad in [
            "",
            "../evil.jar",
            "./mods/../x.jar",
            "C:/windows/x.jar",
            "\\\\server\\share\\x.jar",
            "/etc/passwd",
            "a/../../b",
        ] {
            assert!(safe_rel_path(bad).is_err(), "{bad} 不该通过");
        }
        assert!(safe_rel_path("mods/sodium.jar").is_ok());
        assert!(safe_rel_path("./config/x.json").is_ok());
    }

    #[test]
    fn parse_index_rejects_bad_packs() {
        // 没有文件
        let empty = r#"{"dependencies":{"minecraft":"1.20.1"},"files":[]}"#;
        assert!(parse_index(empty).is_err());

        // 路径重复
        let dup = r#"{"dependencies":{"minecraft":"1.20.1"},"files":[
            {"path":"mods/a.jar","downloads":["https://cdn.modrinth.com/a.jar"]},
            {"path":"mods/A.jar","downloads":["https://cdn.modrinth.com/b.jar"]}
        ]}"#;
        assert!(parse_index(dup).is_err());

        // 声明加载器但没有版本
        let no_ver = r#"{"dependencies":{"minecraft":"1.20.1","fabric-loader":null},"files":[
            {"path":"mods/a.jar","downloads":["https://cdn.modrinth.com/a.jar"]}
        ]}"#;
        assert!(parse_index(no_ver).is_err());

        // 正常的最小包
        let ok = r#"{"dependencies":{"minecraft":"1.20.1"},"files":[
            {"path":"mods/a.jar","downloads":["https://cdn.modrinth.com/a.jar"],"fileSize":10}
        ]}"#;
        assert!(parse_index(ok).is_ok());
    }
    #[test]
    fn parse_index_basic() {
        let j = r#"{
          "name":"Pack","summary":"s","dependencies":{"minecraft":"1.20.1","fabric-loader":"0.16.9"},
          "files":[{"path":"mods/a.jar","hashes":{"sha1":"aa"},"downloads":["https://cdn.modrinth.com/data/x/y.jar"],"fileSize":10}]
        }"#;
        let idx = parse_index(j).unwrap();
        assert_eq!(idx.mc_version, "1.20.1");
        assert_eq!(idx.loader_kind.as_deref(), Some("fabric"));
        assert_eq!(idx.files.len(), 1);
        assert_eq!(idx.total_bytes, 10);
    }
}
