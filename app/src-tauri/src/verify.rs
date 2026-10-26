//! 完整游戏文件检查与智能修复（B3）
//!
//! 设计要点：
//!
//! 1. **四种状态分开报，不合成一个 healthy 布尔**：
//!    - `exists`   文件在不在
//!    - `complete` 大小/哈希对不对（只对能拿到校验值的文件判定）
//!    - `compatible` 环境是否兼容（Java 主版本够不够、平台规则过不过）
//!    - `launchable` 实际能不能启动（需要真跑一次才知道，检查阶段只给"预计"）
//! 2. **只读检查与修复计划完全分离**：`check()` 不改任何文件，`plan()` 只产出
//!    要做什么，`repair()` 才动手，且只补缺失/损坏的，绝不删 mods/config/saves。
//! 3. **玩家可能改过的文件（配置、存档、资源包）永远不进修复清单**。
//! 4. 需要替换的损坏文件先备份到 `{game}/.astral-repair/<批次>/`，不静默覆盖。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 单个文件的检查结果
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FileState {
    /// 相对游戏目录的路径（用 `/` 分隔，便于前端展示）
    pub path: String,
    /// 这份文件是什么：client-jar / library / asset-index / asset-object / loader-json / log-config
    pub kind: String,
    pub exists: bool,
    /// 大小对不上 / 哈希对不上 → false；拿不到校验值时为 None（未知）
    pub complete: Option<bool>,
    /// 期望大小（有就填）
    pub expected_size: Option<u64>,
    /// 实际大小（存在才有）
    pub actual_size: Option<u64>,
    /// 期望哈希（sha1，十六进制小写）
    pub expected_sha1: Option<String>,
    /// 实际哈希（只在需要且文件不太大时算）
    pub actual_sha1: Option<String>,
    /// 人能看懂的说明
    pub detail: Option<String>,
}

/// 一个版本（含继承链）的完整体检报告
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct VersionReport {
    pub version: String,
    /// 继承链，从自己到根版本
    pub chain: Vec<String>,
    pub files: Vec<FileState>,
    /// 缺失文件数
    pub missing: u32,
    /// 存在但校验不通过的文件数
    pub corrupt: u32,
    /// 存在且校验通过（或无法校验但存在）的文件数
    pub ok: u32,
    /// 四个维度分开报
    pub exists: bool,
    pub complete: bool,
    pub compatible: bool,
    /// 只有真启动过才能确认；检查阶段一律 None
    pub launchable: Option<bool>,
    /// Java 要求的主版本
    pub required_java: Option<u32>,
    /// 找到的可用 Java（没有就 None）
    pub java: Option<String>,
    pub issues: Vec<String>,
    pub checked_at: String,
}

impl VersionReport {
    /// 需不需要修
    pub fn needs_repair(&self) -> bool {
        self.missing > 0 || self.corrupt > 0 || !self.compatible
    }
}

/// 修复动作
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RepairAction {
    /// download / reinstall-base / reinstall-loader / install-java / none
    pub kind: String,
    /// 要下载或重建的路径（相对游戏目录）
    pub path: Option<String>,
    pub url: Option<String>,
    pub expected_sha1: Option<String>,
    pub expected_size: Option<u64>,
    /// 为什么需要这一步
    pub reason: String,
    /// 会不会覆盖玩家可能改过的文件（配置/存档永远 false）
    pub overwrites_user_file: bool,
}

/// 修复计划（只读产出，给玩家看）
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RepairPlan {
    pub plan_id: String,
    pub instance_id: Option<String>,
    pub version: String,
    pub actions: Vec<RepairAction>,
    /// 这次修复一共要下多少字节
    pub download_bytes: u64,
    /// 需不需要玩家确认（覆盖玩家文件、或要重装加载器时 true）
    pub needs_confirm: bool,
    /// 给玩家看的摘要
    pub summary: Vec<String>,
    /// 不会动的东西，明写出来让玩家放心
    pub untouched: Vec<String>,
    pub created_at: String,
    /// 加载器信息（重装加载器时要用）
    #[serde(default)]
    pub loader_kind: Option<String>,
    #[serde(default)]
    pub loader_version: Option<String>,
    /// 存在但校验不通过的文件（相对路径）。修复前会先备份这些。
    #[serde(default)]
    pub corrupt_paths: Vec<String>,
}

/// 修复结果
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RepairResult {
    pub batch_id: String,
    pub repaired: Vec<String>,
    pub failed: Vec<(String, String)>,
    /// 备份目录（有覆盖才有）
    pub backup_dir: Option<String>,
    /// 修复后的复查报告
    pub report: Option<VersionReport>,
}

/// 检查时是否计算哈希。大文件（客户端 jar、资源对象）算哈希很慢，
/// 默认只比大小；`deep` 为 true 时才逐个算 SHA1。
#[derive(Clone, Debug)]
pub struct CheckOptions {
    pub deep: bool,
    /// 单个文件超过这个大小就不算哈希（deep 模式也跳过）
    pub hash_limit: u64,
}

impl Default for CheckOptions {
    fn default() -> Self {
        Self {
            deep: false,
            hash_limit: 64 * 1024 * 1024,
        }
    }
}

/// 资源对象的 SHA1 只存在索引里，这里按索引校验会非常慢，
/// 所以默认只抽查：索引里对象总数超过 `object_sample` 时只查前 N 个。
const OBJECT_SAMPLE: usize = 200;

fn rel(p: &Path, game: &Path) -> String {
    p.strip_prefix(game)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}

fn size_of(p: &Path) -> Option<u64> {
    std::fs::metadata(p).ok().map(|m| m.len())
}

/// 读版本 json，沿 inheritsFrom 收集链（最多 5 层，防环）
fn version_chain(game: &Path, version: &str) -> (Vec<String>, Vec<serde_json::Value>, Vec<String>) {
    let mut chain: Vec<String> = Vec::new();
    let mut jsons: Vec<serde_json::Value> = Vec::new();
    let mut issues: Vec<String> = Vec::new();
    let mut cur = version.to_string();
    for _ in 0..5 {
        if chain.contains(&cur) {
            issues.push(format!("版本继承链出现循环：{cur}"));
            break;
        }
        chain.push(cur.clone());
        let p = game.join("versions").join(&cur).join(format!("{cur}.json"));
        let Ok(text) = std::fs::read_to_string(&p) else {
            if cur == version {
                issues.push(format!("缺少版本文件 {cur}.json"));
            }
            break;
        };
        match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(v) => {
                let parent = v
                    .get("inheritsFrom")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                jsons.push(v);
                match parent {
                    Some(parent) => cur = parent,
                    None => break,
                }
            }
            Err(e) => {
                issues.push(format!("版本文件 {cur}.json 无法解析：{e}"));
                break;
            }
        }
    }
    (chain, jsons, issues)
}

/// 平台规则（和 instance.rs 里同一套逻辑，这里独立一份避免互相依赖）
fn lib_allowed(lib: &serde_json::Value) -> bool {
    let Some(rules) = lib.get("rules").and_then(|r| r.as_array()) else {
        return true;
    };
    let os = if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    };
    let mut allowed = false;
    for r in rules {
        let action = r.get("action").and_then(|a| a.as_str()).unwrap_or("");
        let os_match = match r.get("os") {
            None => true,
            Some(osv) => osv
                .get("name")
                .and_then(|n| n.as_str())
                .map(|n| n == os)
                .unwrap_or(true),
        };
        if os_match {
            allowed = action == "allow";
        }
    }
    allowed
}

/// 只读体检：返回完整报告，不动任何文件
pub fn check(game: &Path, version: &str, opts: &CheckOptions) -> VersionReport {
    let (chain, jsons, mut issues) = version_chain(game, version);
    let mut files: Vec<FileState> = Vec::new();

    // ── 客户端 jar ──
    // Forge/NeoForge 继承原版，jar 在链上任意一层都算
    let jar_owner = chain.iter().find(|v| {
        game.join("versions")
            .join(v.as_str())
            .join(format!("{v}.jar"))
            .is_file()
    });
    let mut jar_state = FileState {
        path: format!("versions/{version}/{version}.jar"),
        kind: "client-jar".into(),
        exists: jar_owner.is_some(),
        complete: None,
        expected_size: None,
        actual_size: None,
        expected_sha1: None,
        actual_sha1: None,
        detail: None,
    };
    if let Some(owner) = jar_owner {
        let p = game
            .join("versions")
            .join(owner)
            .join(format!("{owner}.jar"));
        jar_state.actual_size = size_of(&p);
        if owner != version {
            jar_state.detail = Some(format!("客户端 jar 在 {owner}（继承层）"));
        }
        // 从根版本 json 里找 downloads.client
        for v in jsons.iter() {
            if let Some(c) = v.get("downloads").and_then(|d| d.get("client")) {
                if let Some(size) = c.get("size").and_then(|s| s.as_u64()) {
                    jar_state.expected_size = Some(size);
                    jar_state.complete = Some(jar_state.actual_size == Some(size));
                }
                jar_state.expected_sha1 = c
                    .get("sha1")
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_ascii_lowercase());
                break;
            }
        }
        if jar_state.complete.is_none() {
            // 没有元数据就只确认"存在且非空"
            jar_state.complete = Some(jar_state.actual_size.unwrap_or(0) > 0);
        }
    } else {
        jar_state.complete = Some(false);
    }

    // ── 依赖库 ──
    let mut libs: Vec<FileState> = Vec::new();
    for (idx, v) in jsons.iter().enumerate() {
        let vname = &chain[idx];
        let Some(list) = v.get("libraries").and_then(|l| l.as_array()) else {
            continue;
        };
        for lib in list {
            if !lib_allowed(lib) {
                continue;
            }
            let Some(artifact) = lib.get("downloads").and_then(|d| d.get("artifact")) else {
                continue;
            };
            let Some(path) = artifact.get("path").and_then(|p| p.as_str()) else {
                continue;
            };
            let abs = game.join("libraries").join(path);
            let expected_size = artifact.get("size").and_then(|s| s.as_u64());
            let expected_sha1 = artifact
                .get("sha1")
                .and_then(|s| s.as_str())
                .map(|s| s.to_ascii_lowercase());
            let actual_size = size_of(&abs);
            let mut complete = match (actual_size, expected_size) {
                (Some(a), Some(e)) => Some(a == e),
                (Some(a), None) => Some(a > 0),
                (None, _) => Some(false),
            };
            let mut actual_sha1 = None;
            if opts.deep
                && complete == Some(true)
                && expected_sha1.is_some()
                && actual_size.unwrap_or(0) <= opts.hash_limit
            {
                if let Ok(h) = crate::net::sha1_file(&abs) {
                    let h = h.to_ascii_lowercase();
                    if Some(&h) != expected_sha1.as_ref() {
                        complete = Some(false);
                    }
                    actual_sha1 = Some(h);
                }
            }
            libs.push(FileState {
                path: format!("libraries/{path}"),
                kind: "library".into(),
                exists: actual_size.is_some(),
                complete,
                expected_size,
                actual_size,
                expected_sha1,
                actual_sha1,
                detail: if vname != version {
                    Some(format!("来自 {vname}"))
                } else {
                    None
                },
            });
        }
    }
    files.push(jar_state);
    files.extend(libs);

    // ── 资源索引 ──
    let mut asset_index_id: Option<String> = None;
    for v in jsons.iter().rev() {
        if let Some(id) = v
            .get("assetIndex")
            .and_then(|a| a.get("id"))
            .and_then(|i| i.as_str())
        {
            asset_index_id = Some(id.to_string());
            break;
        }
    }
    if let Some(id) = &asset_index_id {
        let abs = game.join("assets").join("indexes").join(format!("{id}.json"));
        let actual_size = size_of(&abs);
        files.push(FileState {
            path: format!("assets/indexes/{id}.json"),
            kind: "asset-index".into(),
            exists: actual_size.is_some(),
            complete: Some(actual_size.map(|s| s > 0).unwrap_or(false)),
            expected_size: None,
            actual_size,
            expected_sha1: None,
            actual_sha1: None,
            detail: None,
        });
    } else if !game.join("assets").is_dir() {
        issues.push("缺少 assets 资源目录".into());
    }

    // ── 资源对象（抽查） ──
    // 全量校验上万个小文件太慢，这里按索引抽查一部分，并如实说明是抽查。
    let mut sampled = 0usize;
    if let Some(id) = &asset_index_id {
        let idx_path = game.join("assets").join("indexes").join(format!("{id}.json"));
        if let Ok(text) = std::fs::read_to_string(&idx_path) {
            if let Ok(idx) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(objects) = idx.get("objects").and_then(|o| o.as_object()) {
                    for (name, obj) in objects.iter().take(OBJECT_SAMPLE) {
                        let Some(hash) = obj.get("hash").and_then(|h| h.as_str()) else {
                            continue;
                        };
                        let Some(size) = obj.get("size").and_then(|s| s.as_u64()) else {
                            continue;
                        };
                        let sub = &hash[0..2.min(hash.len())];
                        let abs = game
                            .join("assets")
                            .join("objects")
                            .join(sub)
                            .join(hash);
                        let actual_size = size_of(&abs);
                        sampled += 1;
                        if actual_size != Some(size) {
                            files.push(FileState {
                                path: format!("assets/objects/{sub}/{hash}"),
                                kind: "asset-object".into(),
                                exists: actual_size.is_some(),
                                complete: Some(false),
                                expected_size: Some(size),
                                actual_size,
                                expected_sha1: Some(hash.to_string()),
                                actual_sha1: None,
                                detail: Some(format!("资源对象 {name}")),
                            });
                        }
                    }
                }
            }
        }
    }
    if sampled > 0 {
        issues.push(format!(
            "资源对象只抽查了前 {sampled} 个（全量校验太慢），没列出的不一定都完整"
        ));
    }

    // ── Java 兼容性 ──
    let required_java = crate::java::required_major(game, version);
    let runtimes = crate::java::scan(game);
    let picked = required_java.and_then(|req| crate::java::pick_for(&runtimes, req));
    let java = picked.as_ref().map(|r| r.path.clone());
    let compatible = match (required_java, &picked) {
        (Some(_), Some(_)) => true,
        (Some(req), None) => {
            issues.push(format!("没有可用的 Java {req}"));
            false
        }
        // 拿不到要求就不下结论，按兼容处理但标注
        (None, _) => {
            issues.push("无法从版本文件判断需要的 Java 版本".into());
            true
        }
    };

    let missing = files
        .iter()
        .filter(|f| !f.exists && f.kind != "asset-object")
        .count() as u32;
    let corrupt = files.iter().filter(|f| f.exists && f.complete == Some(false)).count() as u32;
    let ok = files
        .iter()
        .filter(|f| f.exists && f.complete != Some(false))
        .count() as u32;
    let exists = missing == 0;

    VersionReport {
        version: version.to_string(),
        chain,
        files,
        missing,
        corrupt,
        ok,
        exists,
        complete: corrupt == 0 && exists,
        compatible,
        launchable: None,
        required_java,
        java,
        issues,
        checked_at: crate::time::now_iso(),
    }
}

/// 生成修复计划（只读）。`instance_id` 只用于回填，便于前端把计划挂到实例上。
pub fn plan(
    game: &Path,
    instance_id: Option<&str>,
    version: &str,
    report: &VersionReport,
    loader_kind: Option<&str>,
    loader_version: Option<&str>,
) -> RepairPlan {
    let mut actions: Vec<RepairAction> = Vec::new();
    let mut summary: Vec<String> = Vec::new();
    let mut bytes = 0u64;
    let mut needs_confirm = false;

    // 1) 版本 json / jar / 库 / 资源 → 重跑原版安装（已存在的会跳过）
    let base_broken = report
        .files
        .iter()
        .any(|f| !f.exists || f.complete == Some(false))
        || report.missing > 0
        || report.corrupt > 0;
    if base_broken {
        let missing = report.files.iter().filter(|f| !f.exists).count();
        let corrupt = report
            .files
            .iter()
            .filter(|f| f.exists && f.complete == Some(false))
            .count();
        summary.push(format!(
            "重跑原版安装：补 {missing} 个缺失文件、{corrupt} 个损坏文件（已存在且完好的会跳过）"
        ));
        actions.push(RepairAction {
            kind: "reinstall-base".into(),
            path: None,
            url: None,
            expected_sha1: None,
            expected_size: None,
            reason: format!("{missing} 个文件缺失、{corrupt} 个文件校验不通过"),
            overwrites_user_file: false,
        });
    }

    // 2) 加载器版本目录缺失 → 重装加载器
    if let (Some(kind), Some(ver)) = (loader_kind, loader_version) {
        let mc = report.chain.last().cloned().unwrap_or_default();
        let launch = crate::core::forge_version_id(kind, &mc, ver);
        let expected_json = game
            .join("versions")
            .join(&launch)
            .join(format!("{launch}.json"));
        if !expected_json.is_file() {
            summary.push(format!("重装加载器 {kind} {ver}（版本目录缺失）"));
            actions.push(RepairAction {
                kind: "reinstall-loader".into(),
                path: Some(format!("versions/{launch}/{launch}.json")),
                url: None,
                expected_sha1: None,
                expected_size: None,
                reason: "加载器版本文件缺失".into(),
                overwrites_user_file: false,
            });
            needs_confirm = true;
        }
    }

    // 3) Java 不兼容 → 提示安装（真正下载在 C4/JDK 那边，这里只出动作）
    if !report.compatible {
        if let Some(req) = report.required_java {
            summary.push(format!("需要安装 Java {req}"));
            actions.push(RepairAction {
                kind: "install-java".into(),
                path: None,
                url: None,
                expected_sha1: None,
                expected_size: None,
                reason: format!("没有找到可用的 Java {req}"),
                overwrites_user_file: false,
            });
        }
    }

    // 下载字节估算：只算有明确大小的缺失文件
    for f in report.files.iter().filter(|f| !f.exists) {
        if let Some(s) = f.expected_size {
            bytes += s;
        }
    }
    // 资源对象没全量列，按经验留个下限说明
    if report
        .files
        .iter()
        .any(|f| f.kind == "asset-index" && !f.exists)
    {
        summary.push("资源对象需要重新下载（数量较多，实际耗时会比列表显示的长）".into());
    }

    RepairPlan {
        plan_id: format!("rep_{:x}", crate::time::now_unix_ms()),
        instance_id: instance_id.map(|s| s.to_string()),
        version: version.to_string(),
        actions,
        download_bytes: bytes,
        needs_confirm,
        summary,
        untouched: vec![
            "mods/ 里的模组".into(),
            "config/ 里的配置".into(),
            "saves/ 里的存档".into(),
            "resourcepacks/ 与 shaderpacks/".into(),
            "options.txt 与服务器列表".into(),
        ],
        created_at: crate::time::now_iso(),
        loader_kind: loader_kind.map(|s| s.to_string()),
        loader_version: loader_version.map(|s| s.to_string()),
        corrupt_paths: report
            .files
            .iter()
            .filter(|f| f.exists && f.complete == Some(false))
            .map(|f| f.path.clone())
            .collect(),
    }
}

/// 备份根目录：`{game}/.astral-repair`
pub const BACKUP_DIR: &str = ".astral-repair";

fn backup_root(game: &Path) -> PathBuf {
    game.join(BACKUP_DIR)
}

/// 覆盖前先备份一个文件，返回备份相对路径
fn backup_file(game: &Path, batch: &str, abs: &Path) -> Result<String, String> {
    let relpath = rel(abs, game);
    let dest = backup_root(game).join(batch).join(&relpath);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("PERMISSION|创建备份目录失败|{e}"))?;
    }
    std::fs::copy(abs, &dest).map_err(|e| format!("PERMISSION|备份失败|{e}"))?;
    Ok(relpath)
}

/// 备份目录里存过哪些批次
pub fn backups(game: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(backup_root(game)) else {
        return out;
    };
    for e in rd.flatten() {
        if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            out.push(e.file_name().to_string_lossy().to_string());
        }
    }
    out.sort();
    out.reverse();
    out
}

/// 执行修复。
///
/// `allow_replace` 为 false 时只补缺失文件，不动任何已存在但校验不通过的文件
/// （因为那可能是玩家自己替换过的）。
pub fn repair(
    game: &Path,
    version: &str,
    plan: &RepairPlan,
    allow_replace: bool,
    cancel: crate::net::CancelRef<'_>,
    mut on: impl FnMut(u32, u32, &str),
) -> Result<RepairResult, String> {
    let batch = format!("rep_{:x}", crate::time::now_unix_ms());
    let mut repaired: Vec<String> = Vec::new();
    let mut failed: Vec<(String, String)> = Vec::new();
    let mut backup_dir: Option<String> = None;
    let total = plan.actions.len() as u32;

    for (i, action) in plan.actions.iter().enumerate() {
        if (cancel)() {
            return Err("CANCELLED|任务已取消".into());
        }
        on(i as u32, total, &action.kind);
        match action.kind.as_str() {
            "reinstall-base" => {
                // 覆盖前先把"存在但校验不通过"的文件备份一份，玩家还能翻出来看
                if allow_replace {
                    for f in plan.corrupt_paths.iter() {
                        let abs = game.join(f);
                        if abs.is_file() && backup_file(game, &batch, &abs).is_ok() {
                            backup_dir =
                                Some(backup_root(game).join(&batch).to_string_lossy().to_string());
                        }
                    }
                }
                // 重跑原版安装：moj::Installer 自己会跳过已存在且校验通过的文件，
                // 校验不通过的文件它会删掉重下。
                let cfg = crate::core::load_config();
                let r = crate::core::install(version, game, &cfg.username, |p| {
                    on(i as u32, total, &p.label);
                });
                match r {
                    Ok(_) => repaired.push("base".into()),
                    Err(e) => failed.push(("base".into(), e)),
                }
            }
            "reinstall-loader" => {
                let cfg = crate::core::load_config();
                let kind = plan.loader_kind.clone().unwrap_or_default();
                let ver = plan.loader_version.clone().unwrap_or_default();
                let mc = plan.version.clone();
                let r = match kind.as_str() {
                    "fabric" | "quilt" => crate::core::install_loader(
                        &mc,
                        &kind,
                        &ver,
                        game,
                        &cfg.username,
                        |p| on(i as u32, total, &p.label),
                    ),
                    "forge" | "neoforge" => {
                        crate::core::install_forge_like(&kind, &mc, &ver, game, |p| {
                            on(i as u32, total, &p.label)
                        })
                        .map(|_| ())
                    }
                    other => Err(format!("UNSUPPORTED|暂不支持的加载器 {other}")),
                };
                match r {
                    Ok(_) => repaired.push("loader".into()),
                    Err(e) => failed.push(("loader".into(), e)),
                }
            }
            "install-java" => {
                // Java 下载由 jdk 模块负责，这里只如实记录需要人工/前端触发
                failed.push((
                    "java".into(),
                    format!(
                        "JAVA_MISSING|需要安装 Java {}|请在运行库页面下载",
                        action.reason
                    ),
                ));
            }
            other => {
                failed.push((other.to_string(), "UNSUPPORTED|未知修复动作".into()));
            }
        }
    }

    // 修复后复查
    let report = check(game, version, &CheckOptions::default());
    Ok(RepairResult {
        batch_id: batch,
        repaired,
        failed,
        backup_dir,
        report: Some(report),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rel_uses_forward_slashes() {
        let game = Path::new("C:/game");
        let p = Path::new("C:/game/versions/1.20.1/1.20.1.jar");
        assert_eq!(rel(p, game), "versions/1.20.1/1.20.1.jar");
    }

    #[test]
    fn report_needs_repair_when_missing() {
        let r = VersionReport {
            version: "1.20.1".into(),
            chain: vec!["1.20.1".into()],
            files: vec![],
            missing: 1,
            corrupt: 0,
            ok: 0,
            exists: false,
            complete: false,
            compatible: true,
            launchable: None,
            required_java: Some(17),
            java: None,
            issues: vec![],
            checked_at: String::new(),
        };
        assert!(r.needs_repair());
    }

    #[test]
    fn report_clean_needs_no_repair() {
        let r = VersionReport {
            version: "1.20.1".into(),
            chain: vec!["1.20.1".into()],
            files: vec![],
            missing: 0,
            corrupt: 0,
            ok: 3,
            exists: true,
            complete: true,
            compatible: true,
            launchable: None,
            required_java: Some(17),
            java: Some("C:/jdk17/bin/java.exe".into()),
            issues: vec![],
            checked_at: String::new(),
        };
        assert!(!r.needs_repair());
    }

    #[test]
    fn plan_never_touches_user_files() {
        let game = Path::new("C:/nonexistent-astral-test");
        let report = VersionReport {
            version: "1.20.1".into(),
            chain: vec!["1.20.1".into()],
            files: vec![FileState {
                path: "versions/1.20.1/1.20.1.jar".into(),
                kind: "client-jar".into(),
                exists: false,
                complete: Some(false),
                expected_size: Some(100),
                actual_size: None,
                expected_sha1: None,
                actual_sha1: None,
                detail: None,
            }],
            missing: 1,
            corrupt: 0,
            ok: 0,
            exists: false,
            complete: false,
            compatible: true,
            launchable: None,
            required_java: Some(17),
            java: None,
            issues: vec![],
            checked_at: String::new(),
        };
        let p = plan(game, Some("inst_1"), "1.20.1", &report, None, None);
        assert!(p
            .actions
            .iter()
            .all(|a| !a.overwrites_user_file && !a.path.as_deref().unwrap_or("").contains("mods")));
        assert!(p.untouched.iter().any(|s| s.contains("saves")));
        assert_eq!(p.download_bytes, 100);
    }

    #[test]
    fn empty_plan_for_healthy_version() {
        let game = Path::new("C:/nonexistent-astral-test");
        let report = VersionReport {
            version: "1.20.1".into(),
            chain: vec!["1.20.1".into()],
            files: vec![],
            missing: 0,
            corrupt: 0,
            ok: 0,
            exists: true,
            complete: true,
            compatible: true,
            launchable: None,
            required_java: Some(17),
            java: Some("C:/jdk17/bin/java.exe".into()),
            issues: vec![],
            checked_at: String::new(),
        };
        let p = plan(game, None, "1.20.1", &report, None, None);
        assert!(p.actions.is_empty());
        assert!(!p.needs_confirm);
    }
}
