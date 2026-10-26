//! 诊断包：把"玩家遇到问题时该给你的东西"一次收集好，本机脱敏后打成一个 zip。
//!
//! 为什么要有这个模块：日志、崩溃报告、体检结果、Java 检查、Mod 扫描、任务状态
//! 分散在六七个命令里。玩家来问"启动不了"，让他一个个点、一个个复制，最后拿到的
//! 多半还缺东西。这里把它们收成一个文件，玩家直接把这个 zip 发过来就行。
//!
//! 三条硬约束：
//! 1. **只在本机处理**。收集、脱敏、打包全在本地完成，不上传任何东西。
//! 2. **只读**。除了最后写出的那个 zip，不改动游戏目录里的任何文件。
//! 3. **不假装完整**。哪一段没收到（比如没崩过、没有日志），就在报告里写清楚，
//!    而不是留一个空文件让人以为一切正常。

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

/// 整个包的上限。超了就少收日志，而不是把包撑到几百 MB 发不出去。
pub const MAX_BUNDLE_BYTES: u64 = 32 * 1024 * 1024;
/// 最多带几份日志
pub const MAX_LOG_FILES: usize = 5;
/// 最多带几份崩溃报告
pub const MAX_CRASH_FILES: usize = 2;
/// 单份日志脱敏后保留的字节上限（取尾部，尾部才是出事的地方）
pub const MAX_LOG_BYTES: usize = 512 * 1024;
/// 报告正文里内嵌文本的字节上限
pub const MAX_TEXT_BYTES: usize = 256 * 1024;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BundleSection {
    /// report / instance / logs / crash / mods
    pub key: String,
    pub label: String,
    pub files: u32,
    pub bytes: u64,
    /// 这一段里被脱敏的处数
    pub redactions: u32,
    /// 没收到东西时说明原因（不硬编"正常"）
    pub note: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BundlePlan {
    pub instance_id: String,
    pub instance_name: String,
    pub file_name: String,
    pub target_path: String,
    /// 同名包已存在（会覆盖）
    pub exists: bool,
    pub sections: Vec<BundleSection>,
    pub estimated_bytes: u64,
    pub free_bytes: Option<u64>,
    pub issues: Vec<String>,
    pub blocking: bool,
    pub collected_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BundleResult {
    pub path: String,
    pub bytes: u64,
    pub sections: Vec<BundleSection>,
    pub redactions: u32,
    /// 收集过程中遇到的问题（缺日志、读不出来……），不影响包本身
    pub warnings: Vec<String>,
}

/// 待写进 zip 的一份文件
struct Entry {
    name: String,
    data: Vec<u8>,
}

fn text_entry(name: &str, text: &str) -> Entry {
    Entry {
        name: name.to_string(),
        data: text.as_bytes().to_vec(),
    }
}

/// zip 内部名字不能带路径分隔符和冒号，否则在别人的解压工具里会乱
fn entry_name(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').to_string();
    if trimmed.is_empty() {
        "unnamed.txt".to_string()
    } else {
        trimmed
    }
}

/// 把一个绝对路径里的游戏目录和用户名换成占位符。
///
/// `logs::redact` 已经处理了 `\Users\<名字>`、邮箱、IPv4、token，
/// 这里补两处它管不到的：游戏目录本身（可能暴露盘符和目录结构）、
/// 玩家自己起的离线用户名。
fn scrub(input: &str, game: &Path, username: &str) -> String {
    let mut out = input.to_string();
    let game_str = game.to_string_lossy().to_string();
    if !game_str.is_empty() {
        out = out.replace(&game_str, "<game>");
    }
    // Windows 上日志里常见的是带反斜杠的写法，大小写也可能不同
    if !username.is_empty() && username.len() >= 3 {
        out = out.replace(username, "<user>");
    }
    out
}

/// 全部文本走一遍：先换掉路径与用户名，再按规则脱敏，最后限长。
///
/// 顺序不能反。`logs::redact` 会把 `C:\Users\ASUS\...` 里的用户名换成 `<user>`，
/// 换完这段路径就跟真实的游戏目录对不上了，`scrub` 再想按整条路径替换就找不到。
/// 所以先把整条路径换成 `<game>`，剩下的交给 `redact`。
fn clean(input: &str, game: &Path, username: &str, max_bytes: usize) -> (String, u32) {
    let scrubbed = scrub(input, game, username);
    let (redacted, count) = crate::logs::redact(&scrubbed);
    let bytes = redacted.as_bytes();
    if bytes.len() <= max_bytes {
        return (redacted, count);
    }
    // 超长就留尾部（出事的地方在最后），并保证不从 UTF-8 字符中间切开
    let mut start = bytes.len() - max_bytes;
    while start < bytes.len() && !redacted.is_char_boundary(start) {
        start += 1;
    }
    let head = format!("……（前面省略 {} 字节）\n", start);
    (format!("{head}{}", &redacted[start..]), count)
}

/// `CODE|消息|细节` → 给玩家看的消息（没有编码就原样返回）
fn human_error(encoded: &str) -> String {
    let parts: Vec<&str> = encoded.split('|').collect();
    if parts.len() >= 2 {
        parts[1].to_string()
    } else {
        encoded.to_string()
    }
}

fn human_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / 1024.0 / 1024.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

/// 机器可读的那一份：给以后接 AI 分析用，字段都是稳定的。
fn build_json(
    inst: &crate::instance::Instance,
    verify: Option<&crate::verify::VersionReport>,
    java: Option<&crate::java_audit::JavaCheck>,
    mods: Option<&crate::modscan::ModScanReport>,
) -> serde_json::Value {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    let cpu_cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(0);
    let cfg = crate::core::load_config();

    serde_json::json!({
        "schema": 1,
        "collectedAt": crate::time::now_iso(),
        "launcher": {
            "appVersion": env!("CARGO_PKG_VERSION"),
            "apiVersion": crate::API_VERSION,
        },
        "system": {
            "os": "Windows",
            "cpuCores": cpu_cores,
            "totalMemGb": ((sys.total_memory() as f64 / 1024.0 / 1024.0 / 1024.0) * 10.0).round() / 10.0,
            "freeMemGb": ((sys.available_memory() as f64 / 1024.0 / 1024.0 / 1024.0) * 10.0).round() / 10.0,
        },
        "config": {
            "gameDir": "<game>",
            "source": cfg.source,
            // 用户名不进包：那是玩家自己的名字，对排查没帮助
            "username": if cfg.username.is_empty() { "" } else { "<user>" },
            "hasMsaClientId": cfg.msa_client_id.as_ref().map(|s| !s.is_empty()).unwrap_or(false),
        },
        "instance": inst,
        // 真实路径不进包：盘符和目录结构对排查没帮助，反而暴露玩家机器布局
        "gameDir": "<game>",
        "verify": verify.map(|v| serde_json::json!({
            "version": v.version,
            "chain": v.chain,
            "exists": v.exists,
            "complete": v.complete,
            "compatible": v.compatible,
            "launchable": v.launchable,
            "requiredJava": v.required_java,
            "java": v.java,
            "missing": v.missing,
            "corrupt": v.corrupt,
            "ok": v.ok,
            "issues": v.issues,
        })),
        "java": java,
        "mods": mods.map(|m| serde_json::json!({
            "counts": m.counts,
            "summary": m.summary,
            "issues": m.issues.iter().map(|i| serde_json::json!({
                "code": i.code,
                "severity": i.severity,
                "fileName": i.file_name,
                "modId": i.mod_id,
                "message": i.message,
            })).collect::<Vec<_>>(),
        })),
        "tasks": crate::task::health(),
        "sessions": crate::session::list(Some(&inst.id), 10),
    })
}

/// 人读的那一份。用中文写，因为拿到这个包的是玩家和帮玩家看的人。
fn build_report(
    inst: &crate::instance::Instance,
    verify: Option<&crate::verify::VersionReport>,
    java: Option<&crate::java_audit::JavaCheck>,
    mods: Option<&crate::modscan::ModScanReport>,
    logs: &[crate::logs::LogFile],
    crashes: &[crate::logs::LogFile],
) -> String {
    let mut s = String::new();
    s.push_str("Astral Launcher 诊断包\n");
    s.push_str("======================\n\n");
    s.push_str(
        "这个文件是启动器在本机自动整理的，敏感信息已经在本机替换掉，不会上传到任何地方。\n\n",
    );

    s.push_str(&format!("收集时间：{}\n", crate::time::now_iso()));
    s.push_str(&format!("启动器版本：{}\n", env!("CARGO_PKG_VERSION")));
    s.push_str(&format!("接口版本：{}\n\n", crate::API_VERSION));

    s.push_str("一、实例\n");
    s.push_str(&format!("名称：{}\n", inst.name));
    s.push_str(&format!("游戏版本：{}\n", inst.game_version));
    match &inst.loader {
        Some(l) => s.push_str(&format!("加载器：{} {}\n", l.kind, l.version)),
        None => s.push_str("加载器：原版\n"),
    }
    if let Some(lv) = &inst.launch_version {
        s.push_str(&format!("实际启动版本：{lv}\n"));
    }
    s.push_str("游戏目录：<game>\n");
    s.push_str(&format!(
        "内存设置：{}\n",
        inst.memory_mb
            .map(|m| format!("{m} MB"))
            .unwrap_or_else(|| "跟随默认".into())
    ));
    s.push_str(&format!(
        "Java：{}\n",
        inst.java_path.clone().unwrap_or_else(|| "自动选择".into())
    ));
    s.push_str(&format!(
        "Mod：{} 个启用 / {} 个禁用\n",
        inst.mods_count, inst.mods_disabled
    ));
    if !inst.health.ok {
        s.push_str(&format!(
            "实例状态异常：{}\n",
            inst.health.reasons.join("；")
        ));
    }
    s.push('\n');

    s.push_str("二、文件完整性\n");
    match verify {
        Some(v) => {
            s.push_str(&format!(
                "版本：{}（继承链：{}）\n",
                v.version,
                v.chain.join(" → ")
            ));
            s.push_str(&format!(
                "存在：{}／完整：{}／环境兼容：{}／真启动过：{}\n",
                yes_no(v.exists),
                yes_no(v.complete),
                yes_no(v.compatible),
                match v.launchable {
                    Some(true) => "是",
                    Some(false) => "否",
                    None => "还没启动过，无法确认",
                }
            ));
            s.push_str(&format!(
                "文件统计：正常 {}，缺失 {}，损坏 {}\n",
                v.ok, v.missing, v.corrupt
            ));
            if !v.issues.is_empty() {
                s.push_str("问题：\n");
                for i in &v.issues {
                    s.push_str(&format!("  - {i}\n"));
                }
            }
        }
        None => s.push_str("没拿到（体检失败，见 logs 段）\n"),
    }
    s.push('\n');

    s.push_str("三、Java\n");
    match java {
        Some(j) => {
            s.push_str(&format!(
                "需要 Java {}（{}）\n",
                j.required
                    .map(|m| m.to_string())
                    .unwrap_or_else(|| "未知".into()),
                j.requirement
            ));
            s.push_str(&format!(
                "实际：{}\n",
                j.version.clone().unwrap_or_else(|| "没找到".into())
            ));
            if let Some(p) = &j.problem {
                s.push_str(&format!("问题：{p}\n"));
            }
            if let Some(g) = &j.suggestion {
                s.push_str(&format!("建议：{g}\n"));
            }
        }
        None => s.push_str("没拿到（检查失败）\n"),
    }
    s.push('\n');

    s.push_str("四、Mod\n");
    match mods {
        Some(m) => {
            s.push_str(&format!(
                "共 {} 个（启用 {}／禁用 {}），可读 {}，读不出 {}\n",
                m.counts.total,
                m.counts.enabled,
                m.counts.disabled,
                m.counts.readable,
                m.counts.unreadable
            ));
            if let Some(sum) = &m.summary {
                s.push_str(&format!("结论：{sum}\n"));
            }
            if !m.issues.is_empty() {
                s.push_str("问题：\n");
                for i in m.issues.iter().take(30) {
                    s.push_str(&format!(
                        "  - [{}] {}：{}（{}）\n",
                        i.severity,
                        i.code,
                        i.message,
                        i.file_name.clone().unwrap_or_default()
                    ));
                }
            }
        }
        None => s.push_str("没拿到（扫描失败）\n"),
    }
    s.push('\n');

    s.push_str("五、日志\n");
    if logs.is_empty() {
        s.push_str("这个实例还没有游戏日志（可能一次都没启动过）\n");
    } else {
        for l in logs {
            s.push_str(&format!(
                "  - {}（{}，{}）\n",
                l.name,
                human_bytes(l.size_bytes),
                l.modified_at
            ));
        }
    }
    s.push('\n');

    s.push_str("六、崩溃报告\n");
    if crashes.is_empty() {
        s.push_str("没有崩溃报告（没崩过，或者崩在启动器层面）\n");
    } else {
        for c in crashes {
            s.push_str(&format!("  - {}（{}）\n", c.name, c.modified_at));
        }
    }
    s.push('\n');

    s.push_str("七、任务与进程\n");
    let health = crate::task::health();
    s.push_str(&format!(
        "任务：共 {} 个，运行中 {} 个，取消中 {} 个\n",
        health.get("tasks").and_then(|v| v.as_u64()).unwrap_or(0),
        health.get("running").and_then(|v| v.as_u64()).unwrap_or(0),
        health
            .get("cancelling")
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
    ));
    let sessions = crate::session::list(Some(&inst.id), 5);
    if sessions.is_empty() {
        s.push_str("还没有启动记录\n");
    } else {
        for x in sessions {
            s.push_str(&format!(
                "  - {} 开始，状态 {}，退出码 {}，时长 {}\n",
                x.started_at,
                x.state,
                x.exit_code
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "—".into()),
                x.duration_ms
                    .map(|d| format!("{} 分钟", d / 60000))
                    .unwrap_or_else(|| "—".into())
            ));
        }
    }
    s.push_str("\n游戏目录：<game>\n");
    s.push_str("（上面出现的 <game>、<user>、<ip>、<email> 都是本机替换过的占位符）\n");
    s
}

fn yes_no(v: bool) -> &'static str {
    if v {
        "是"
    } else {
        "否"
    }
}

/// 收集所有材料（纯内存，不写盘）
fn collect(
    game: &Path,
    inst: &crate::instance::Instance,
    stats: &dyn Fn(&str) -> crate::instance::PlayStats,
    include_logs: bool,
) -> (Vec<Entry>, Vec<BundleSection>, Vec<String>) {
    let dir = Path::new(&inst.dir);
    let username = crate::core::load_config().username;
    let mut entries: Vec<Entry> = Vec::new();
    let mut sections: Vec<BundleSection> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // ── 体检 / Java / Mod 扫描：都失败也不影响出包 ──
    let version = inst
        .launch_version
        .clone()
        .unwrap_or_else(|| inst.game_version.clone());
    let verify = crate::verify::check(game, &version, &Default::default());
    let java = crate::java_audit::check_instance(game, &inst.id, stats).ok();
    if java.is_none() {
        warnings.push("Java 检查失败，报告里这一段会显示「没拿到」".into());
    }
    let mods = crate::modscan::scan(Path::new(&inst.dirs.mods));

    // ── 实例 ──
    let inst_text = serde_json::to_string_pretty(inst).unwrap_or_else(|_| "{}".into());
    let (inst_clean, inst_red) = clean(&inst_text, game, &username, MAX_TEXT_BYTES);
    entries.push(text_entry("instance.json", &inst_clean));
    sections.push(BundleSection {
        key: "instance".into(),
        label: "实例信息".into(),
        files: 1,
        bytes: inst_clean.len() as u64,
        redactions: inst_red,
        note: None,
    });

    // ── 日志与崩溃报告 ──
    let all_logs = crate::logs::list(dir);
    let game_logs: Vec<_> = all_logs
        .iter()
        .filter(|l| l.kind == "game")
        .take(MAX_LOG_FILES)
        .cloned()
        .collect();
    let crash_logs: Vec<_> = all_logs
        .iter()
        .filter(|l| l.kind == "crash")
        .take(MAX_CRASH_FILES)
        .cloned()
        .collect();

    let mut log_redactions = 0u32;
    let mut log_bytes = 0u64;
    let mut log_files = 0u32;
    if include_logs {
        for l in &game_logs {
            match crate::logs::read(dir, &l.id, Some(2000)) {
                Ok(t) => {
                    let (text, red) = clean(&t.text, game, &username, MAX_LOG_BYTES);
                    log_redactions += red;
                    log_bytes += text.len() as u64;
                    log_files += 1;
                    entries.push(text_entry(&format!("logs/{}", entry_name(&l.name)), &text));
                }
                Err(e) => warnings.push(format!("日志 {} 读不出来：{}", l.name, human_error(&e))),
            }
        }
    }
    sections.push(BundleSection {
        key: "logs".into(),
        label: "游戏日志".into(),
        files: log_files,
        bytes: log_bytes,
        redactions: log_redactions,
        note: if !include_logs {
            Some("按调用方要求没有收集日志".into())
        } else if game_logs.is_empty() {
            Some("这个实例还没有游戏日志".into())
        } else {
            None
        },
    });

    let mut crash_redactions = 0u32;
    let mut crash_bytes = 0u64;
    let mut crash_files = 0u32;
    for c in &crash_logs {
        match crate::logs::read(dir, &c.id, Some(4000)) {
            Ok(t) => {
                let (text, red) = clean(&t.text, game, &username, MAX_LOG_BYTES);
                crash_redactions += red;
                crash_bytes += text.len() as u64;
                crash_files += 1;
                entries.push(text_entry(&format!("crash/{}", entry_name(&c.name)), &text));
            }
            Err(e) => warnings.push(format!("崩溃报告 {} 读不出来：{}", c.name, human_error(&e))),
        }
    }
    sections.push(BundleSection {
        key: "crash".into(),
        label: "崩溃报告".into(),
        files: crash_files,
        bytes: crash_bytes,
        redactions: crash_redactions,
        note: if crash_logs.is_empty() {
            Some("没有崩溃报告（没崩过，或者崩在启动器层面）".into())
        } else {
            None
        },
    });

    // ── Mod 清单 ──
    let mut mod_text = String::new();
    mod_text.push_str("文件名\t启用\t大小\t名称\t版本\t加载器\n");
    for m in &mods.mods {
        mod_text.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\n",
            m.file_name,
            if m.enabled { "是" } else { "否" },
            m.size_bytes,
            m.ids.first().map(|i| i.mod_id.clone()).unwrap_or_default(),
            m.ids.first().map(|i| i.version.clone()).unwrap_or_default(),
            m.ids.first().map(|i| i.loader.clone()).unwrap_or_default(),
        ));
    }
    let (mod_clean, mod_red) = clean(&mod_text, game, &username, MAX_TEXT_BYTES);
    entries.push(text_entry("mods.txt", &mod_clean));
    sections.push(BundleSection {
        key: "mods".into(),
        label: "Mod 清单".into(),
        files: 1,
        bytes: mod_clean.len() as u64,
        redactions: mod_red,
        note: if mods.mods.is_empty() {
            Some("这个实例没有 Mod".into())
        } else {
            None
        },
    });

    // ── 报告 + 机器可读摘要 ──
    let report = build_report(
        inst,
        Some(&verify),
        java.as_ref(),
        Some(&mods),
        &game_logs,
        &crash_logs,
    );
    let (report_clean, report_red) = clean(&report, game, &username, MAX_TEXT_BYTES);
    let json = build_json(inst, Some(&verify), java.as_ref(), Some(&mods));
    let json_text = serde_json::to_string_pretty(&json).unwrap_or_else(|_| "{}".into());
    let (json_clean, json_red) = clean(&json_text, game, &username, MAX_TEXT_BYTES);
    entries.push(text_entry("report.txt", &report_clean));
    entries.push(text_entry("bundle.json", &json_clean));
    sections.insert(
        0,
        BundleSection {
            key: "report".into(),
            label: "诊断报告".into(),
            files: 2,
            bytes: (report_clean.len() + json_clean.len()) as u64,
            redactions: report_red + json_red,
            note: None,
        },
    );

    (entries, sections, warnings)
}

fn target_of(game: &Path, inst: &crate::instance::Instance) -> (String, PathBuf) {
    let file_name = format!("astral-diag-{}-{}.zip", inst.id, crate::time::now_stamp());
    let path = game.join("diagnostics").join(&file_name);
    (file_name, path)
}

/// 预览：只读，告诉玩家这个包里会有什么、多大。
pub fn plan(
    game: &Path,
    instance_id: &str,
    stats: &dyn Fn(&str) -> crate::instance::PlayStats,
) -> Result<BundlePlan, String> {
    let inst = crate::instance::find(game, instance_id, stats)
        .ok_or_else(|| "INSTANCE_NOT_FOUND|实例不存在".to_string())?;
    let (file_name, target) = target_of(game, &inst);
    let (entries, sections, warnings) = collect(game, &inst, stats, true);
    let estimated: u64 = entries.iter().map(|e| e.data.len() as u64).sum();
    let exists = target.is_file();
    let mut issues = warnings;
    let mut blocking = false;
    if estimated > MAX_BUNDLE_BYTES {
        issues.push(format!(
            "诊断包太大（约 {}），超过了 {} 的上限",
            human_bytes(estimated),
            human_bytes(MAX_BUNDLE_BYTES)
        ));
        blocking = true;
    }
    if exists {
        issues.push("同名诊断包已存在，导出会覆盖它".into());
    }
    Ok(BundlePlan {
        instance_id: inst.id.clone(),
        instance_name: inst.name.clone(),
        file_name,
        target_path: target.to_string_lossy().to_string(),
        exists,
        sections,
        estimated_bytes: estimated,
        free_bytes: crate::world_backup::available_space(&target),
        issues,
        blocking,
        collected_at: crate::time::now_iso(),
    })
}

/// 真的出包。写到 `<game>/diagnostics/`，先 `.part` 再改名。
pub fn export(
    game: &Path,
    instance_id: &str,
    include_logs: bool,
    stats: &dyn Fn(&str) -> crate::instance::PlayStats,
) -> Result<BundleResult, String> {
    let inst = crate::instance::find(game, instance_id, stats)
        .ok_or_else(|| "INSTANCE_NOT_FOUND|实例不存在".to_string())?;
    let (_file_name, target) = target_of(game, &inst);
    let (entries, sections, warnings) = collect(game, &inst, stats, include_logs);
    let total: u64 = entries.iter().map(|e| e.data.len() as u64).sum();
    if total > MAX_BUNDLE_BYTES {
        return Err(format!(
            "INVALID_VALUE|诊断包太大（约 {}），请先清掉旧日志再试",
            human_bytes(total)
        ));
    }
    if let (Some(free), true) = (crate::world_backup::available_space(&target), total > 0) {
        if free < total.saturating_add(4 * 1024 * 1024) {
            return Err(format!(
                "DISK_FULL|磁盘剩余空间不足（需要约 {}，可用 {}）",
                human_bytes(total),
                human_bytes(free)
            ));
        }
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("PERMISSION|创建目录失败|{e}"))?;
    }
    let bytes = write_zip(&target, &entries)?;
    let redactions = sections.iter().map(|s| s.redactions).sum();
    Ok(BundleResult {
        path: target.to_string_lossy().to_string(),
        bytes,
        sections,
        redactions,
        warnings,
    })
}

fn write_zip(target: &Path, entries: &[Entry]) -> Result<u64, String> {
    let part = target.with_extension("zip.part");
    let file = std::fs::File::create(&part).map_err(|e| format!("PERMISSION|创建文件失败|{e}"))?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let res = (|| -> Result<(), String> {
        for e in entries {
            zip.start_file(e.name.as_str(), opts)
                .map_err(|err| format!("INTERNAL|写入诊断包失败|{err}"))?;
            zip.write_all(&e.data)
                .map_err(|err| format!("INTERNAL|写入诊断包失败|{err}"))?;
        }
        zip.finish()
            .map_err(|err| format!("INTERNAL|收尾失败|{err}"))?;
        Ok(())
    })();
    if let Err(e) = res {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    std::fs::rename(&part, target).map_err(|e| format!("PERMISSION|保存失败|{e}"))?;
    Ok(std::fs::metadata(target).map(|m| m.len()).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("astral-support-{}-{tag}", crate::time::now_nanos()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn entry_names_are_safe() {
        assert_eq!(
            entry_name("2024-01-01_12.00.00.txt"),
            "2024-01-01_12.00.00.txt"
        );
        assert_eq!(entry_name("../etc/passwd"), "_etc_passwd");
        assert_eq!(entry_name("a:b*c?.txt"), "a_b_c_.txt");
        assert_eq!(entry_name("   "), "unnamed.txt");
        assert_eq!(entry_name("..."), "unnamed.txt");
    }

    #[test]
    fn scrub_replaces_game_dir_and_username() {
        let game = Path::new("D:\\Games\\.minecraft");
        let out = scrub(
            "打开 D:\\Games\\.minecraft\\logs\\latest.log 看看",
            game,
            "Steve",
        );
        assert!(out.contains("<game>"));
        assert!(!out.contains("D:\\Games"));
        let out2 = scrub("玩家 Steve 启动了游戏", game, "Steve");
        assert_eq!(out2, "玩家 <user> 启动了游戏");
    }

    #[test]
    fn short_username_is_left_alone() {
        // 两个字符的名字太容易撞上正常单词，不能乱替换
        let out = scrub("玩家 ab 启动了游戏", Path::new("C:\\m"), "ab");
        assert_eq!(out, "玩家 ab 启动了游戏");
    }

    #[test]
    fn clean_keeps_tail_when_too_long() {
        let body = "x".repeat(1000) + "TAIL";
        let (out, _) = clean(&body, Path::new("C:\\m"), "steve", 64);
        assert!(out.starts_with("……（前面省略"));
        // 脱敏按行处理，每行都会补一个换行，所以尾部要忽略那个换行再看
        assert!(out.trim_end().ends_with("TAIL"), "got: {out:?}");
        assert!(out.len() < 200);
    }

    #[test]
    fn clean_scrubs_game_dir_before_redacting() {
        // 顺序写反会漏：redact 先把用户名换成 <user>，整条路径就对不上了
        let game = Path::new("C:\\Users\\ASUS\\AppData\\Roaming\\.minecraft");
        let line = "日志在 C:\\Users\\ASUS\\AppData\\Roaming\\.minecraft\\logs\\latest.log";
        let (out, _) = clean(line, game, "Player", 4096);
        assert!(out.contains("<game>"), "got: {out}");
        assert!(!out.contains("ASUS"), "got: {out}");
    }

    #[test]
    fn clean_does_not_split_utf8() {
        let body = "中文".repeat(100);
        let (out, _) = clean(&body, Path::new("C:\\m"), "steve", 61);
        // 只要不 panic 且是合法 UTF-8 就行（String 天然保证）
        assert!(out.contains("中文"));
    }

    #[test]
    fn human_bytes_reads_naturally() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(2048), "2.0 KB");
        assert_eq!(human_bytes(3 * 1024 * 1024), "3.0 MB");
    }

    #[test]
    fn writes_a_readable_zip() {
        let d = tmp("zip");
        let target = d.join("diag.zip");
        let entries = vec![
            text_entry("report.txt", "你好"),
            text_entry("logs/latest.log", "line1\nline2\n"),
        ];
        let bytes = write_zip(&target, &entries).unwrap();
        assert!(bytes > 0);
        assert!(!target.with_extension("zip.part").exists());

        let f = std::fs::File::open(&target).unwrap();
        let mut zip = zip::ZipArchive::new(f).unwrap();
        assert_eq!(zip.len(), 2);
        let mut names: Vec<String> = (0..zip.len())
            .map(|i| {
                zip.by_index(i)
                    .unwrap()
                    .name()
                    .map(|n| n.to_string())
                    .unwrap_or_default()
            })
            .collect();
        names.sort();
        assert_eq!(names, vec!["logs/latest.log", "report.txt"]);
        std::fs::remove_dir_all(&d).ok();
    }
}
