//! 崩溃报告解析。
//!
//! Minecraft 崩溃时会在 `crash-reports/` 下留一份文本报告，格式固定：
//!
//! ```text
//! ---- Minecraft Crash Report ----
//! // 一句俏皮话
//!
//! Time: 2026-10-26 12:00:00
//! Description: Rendering overlay
//!
//! java.lang.NullPointerException: Cannot invoke ...
//!     at net.minecraft.client.renderer.GameRenderer.render(GameRenderer.java:900)
//! ...
//! -- System Details --
//!     Minecraft Version: 1.20.1
//!     Java Version: 17.0.8, Microsoft
//! ```
//!
//! `logs::classify_text` 用关键词猜问题类型，够用但不够细。这里把报告本身
//! **按它自己的格式**拆开：什么时候崩的、崩在干什么、抛了什么异常、
//! 被谁引起的、系统详情、报告里点名的可疑模组。
//!
//! 原则：只读文件里**写着的东西**。报告没写"可疑模组"就不猜是哪个模组，
//! 只在 `inferredPackages` 里列出栈帧里出现的非原版包名，并标明这是推断。

use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

/// 报告最多读这么多。正常报告几十 KB，大的整合包报告上百 KB。
const MAX_BYTES: u64 = 2 * 1024 * 1024;
/// "Caused by" 链最多留几层
const MAX_CAUSE: usize = 8;
/// 栈帧最多留几行
const MAX_FRAMES: usize = 12;
/// 可疑模组最多留几个
const MAX_SUSPECTS: usize = 12;
/// 返回给前端的报告原文上限（已脱敏），够看清前后文就行
const MAX_TEXT: usize = 256 * 1024;

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct CrashReport {
    /// 文件里有没有 `---- Minecraft Crash Report ----` 这行
    pub is_crash_report: bool,
    /// `Time:` 那行的值，原样
    pub time: Option<String>,
    /// `Description:` —— 崩的时候游戏在干什么（"Rendering overlay" 之类）
    pub description: Option<String>,
    /// 顶层异常，例如 `java.lang.NullPointerException: Cannot invoke ...`
    pub exception: Option<String>,
    /// `Caused by:` 链，由外到内
    pub cause_chain: Vec<String>,
    /// 顶层异常的前几行栈帧（已去掉行首缩进）
    pub frames: Vec<String>,
    /// 报告里所有 `-- 小节名 --` 的标题
    pub sections: Vec<String>,
    /// `-- System Details --` 里的键值
    pub system: BTreeMap<String, String>,
    /// 报告自己点名的可疑模组（没有就是空）
    pub suspects: Vec<String>,
    /// suspects 从哪来：report（报告写了）/ inferred（从栈帧推断）/ none
    pub suspects_source: String,
    /// 栈帧里出现的非原版包名（推断，仅供参考）
    pub inferred_packages: Vec<String>,
    /// 报告一共多少行
    pub lines: u32,
    /// 因为文件太大只读了后面一部分
    pub truncated: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CrashView {
    pub log_id: String,
    pub log_name: String,
    pub report: CrashReport,
    /// 已脱敏的报告原文（截断到 256 KiB）
    pub text: String,
    /// 按关键词规则得出的结论（和「诊断」用的是同一套规则）
    pub findings: Vec<crate::logs::Finding>,
    /// 依据说明，让玩家知道这不是猜的
    pub basis: String,
}

/// 原版/框架自己的包名，出现在栈帧里不算"可疑模组"
const VANILLA_PREFIXES: [&str; 12] = [
    "java.",
    "javax.",
    "jdk.",
    "sun.",
    "net.minecraft",
    "com.mojang",
    "org.lwjgl",
    "org.apache",
    "org.slf4j",
    "com.google",
    "it.unimi",
    "oshi.",
];

fn is_vanilla(pkg: &str) -> bool {
    VANILLA_PREFIXES.iter().any(|p| pkg.starts_with(p))
}

/// 从栈帧行里取包名：`at net.fabricmc.loader.impl.Foo.bar(...)` → `net.fabricmc.loader.impl`
fn package_of(frame: &str) -> Option<String> {
    let rest = frame.trim().strip_prefix("at ")?;
    // 去掉可能的 "java.base/" 模块前缀
    let rest = match rest.find('/') {
        Some(i) if !rest[..i].contains('.') => &rest[i + 1..],
        _ => rest,
    };
    let full = rest.split('(').next()?.trim();
    let mut parts: Vec<&str> = full.split('.').collect();
    // 末尾两段是「方法名」和「类名」，包名在它们前面
    if parts.len() < 3 {
        return None;
    }
    parts.pop();
    parts.pop();
    Some(parts.join("."))
}

/// 解析报告正文。不做 IO，可单测。
pub fn parse(text: &str, truncated: bool) -> CrashReport {
    let mut r = CrashReport {
        truncated,
        suspects_source: "none".into(),
        ..Default::default()
    };

    let mut lines = text.lines();
    let mut total = 0u32;
    for line in text.lines() {
        total += 1;
        let t = line.trim();
        if t.starts_with("---- Minecraft Crash Report ----") {
            r.is_crash_report = true;
        }
    }
    r.lines = total;

    // 第一遍：头部字段、异常、栈帧
    let mut in_frames = false;
    let mut system_mode = false;
    let mut suspects_mode = false;

    for raw in lines.by_ref() {
        let t = raw.trim();
        if t.is_empty() {
            // 空行结束可疑模组小节
            if suspects_mode {
                suspects_mode = false;
            }
            continue;
        }
        if let Some(rest) = t.strip_prefix("Time:") {
            r.time = Some(rest.trim().to_string());
            continue;
        }
        if let Some(rest) = t.strip_prefix("Description:") {
            r.description = Some(rest.trim().to_string());
            continue;
        }
        // 小节标题：`-- System Details --`
        if t.starts_with("--") && t.ends_with("--") && t.len() > 4 {
            let name = t.trim_matches('-').trim().to_string();
            system_mode = name.eq_ignore_ascii_case("System Details");
            if !name.is_empty() {
                r.sections.push(name);
            }
            in_frames = false;
            continue;
        }
        if let Some(rest) = t.strip_prefix("Caused by:") {
            let v = rest.trim().to_string();
            if !v.is_empty() && r.cause_chain.len() < MAX_CAUSE {
                r.cause_chain.push(v);
            }
            in_frames = true;
            continue;
        }
        if t.starts_with("Suspected Mods:") {
            let rest = t.trim_start_matches("Suspected Mods:").trim();
            if !rest.is_empty() && !rest.eq_ignore_ascii_case("None") {
                for s in rest.split(',') {
                    let s = s.trim();
                    if !s.is_empty() && r.suspects.len() < MAX_SUSPECTS {
                        r.suspects.push(s.to_string());
                    }
                }
                r.suspects_source = "report".into();
            }
            suspects_mode = true;
            continue;
        }
        if suspects_mode && r.suspects.len() < MAX_SUSPECTS {
            // `Suspected Mods:` 后面跟多行的情况
            let v = t.trim_start_matches(['\t', ' ']).trim();
            if !v.is_empty() && !v.contains(": ") {
                r.suspects.push(v.to_string());
                r.suspects_source = "report".into();
                continue;
            }
        }
        // 顶层异常：形如 `java.lang.XxxException: msg`，且还没抓到
        if r.exception.is_none()
            && !t.starts_with("at ")
            && !t.starts_with("Caused by:")
            && t.contains("Exception")
            && t.contains('.')
        {
            r.exception = Some(t.to_string());
            in_frames = true;
            continue;
        }
        if t.starts_with("at ") {
            if in_frames && r.frames.len() < MAX_FRAMES {
                r.frames.push(t.to_string());
            }
            continue;
        }
        // System Details 的键值（行首有制表符）
        if system_mode && raw.starts_with('\t') {
            if let Some((k, v)) = t.split_once(':') {
                let k = k.trim();
                let v = v.trim();
                if !k.is_empty() && !v.is_empty() {
                    r.system.insert(k.to_string(), v.to_string());
                }
            }
        }
    }

    // 推断：栈帧里出现的非原版包名
    let mut pkgs: Vec<String> = Vec::new();
    for f in &r.frames {
        if let Some(p) = package_of(f) {
            if !is_vanilla(&p) && !pkgs.iter().any(|x| p.starts_with(x.as_str())) {
                pkgs.push(p);
            }
        }
    }
    pkgs.truncate(8);
    r.inferred_packages = pkgs;

    r
}

/// 取实例最新的崩溃报告（没有就 None）
pub fn newest(instance_dir: &Path) -> Option<crate::logs::LogFile> {
    crate::logs::list(instance_dir)
        .into_iter()
        .find(|f| f.kind == "crash")
}

/// 读并解析一份崩溃报告。`log_id` 为空时取最新的。
pub fn read(instance_dir: &Path, log_id: Option<&str>) -> Result<CrashView, String> {
    let picked = match log_id {
        Some(id) => crate::logs::list(instance_dir)
            .into_iter()
            .find(|f| f.id == id)
            .ok_or_else(|| "INVALID_VALUE|找不到这份日志".to_string())?,
        None => newest(instance_dir).ok_or_else(|| {
            "NOT_FOUND|这个实例还没有崩溃报告（没崩过，或者崩在启动器层面）".to_string()
        })?,
    };

    let (path, _name) = crate::logs::resolve_path(instance_dir, &picked.id)
        .ok_or_else(|| "INVALID_PATH|日志文件不在了".to_string())?;
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let (raw, truncated) = if size > MAX_BYTES {
        use std::io::{Read, Seek, SeekFrom};
        let mut f = std::fs::File::open(&path).map_err(|e| format!("PERMISSION|读取失败|{e}"))?;
        f.seek(SeekFrom::Start(size - MAX_BYTES))
            .map_err(|e| format!("PERMISSION|定位失败|{e}"))?;
        let mut s = String::new();
        f.read_to_string(&mut s)
            .map_err(|e| format!("PERMISSION|读取失败|{e}"))?;
        (s, true)
    } else {
        (
            std::fs::read_to_string(&path).map_err(|e| format!("PERMISSION|读取失败|{e}"))?,
            false,
        )
    };

    let report = parse(&raw, truncated);
    let (clean, redactions) = crate::logs::redact(&raw);
    let findings = crate::logs::classify_text(&clean);
    let text = if clean.len() > MAX_TEXT {
        // 截断要落在字符边界上，不然中文会被切坏
        let mut end = MAX_TEXT;
        while end > 0 && !clean.is_char_boundary(end) {
            end -= 1;
        }
        clean[..end].to_string()
    } else {
        clean
    };

    Ok(CrashView {
        log_id: picked.id.clone(),
        log_name: picked.name.clone(),
        report,
        text,
        findings,
        basis: format!(
            "按崩溃报告自身格式解析（{} 行），并按本地规则匹配关键词；已脱敏 {} 处，未上传任何内容",
            picked_lines(&raw),
            redactions
        ),
    })
}

fn picked_lines(text: &str) -> u32 {
    text.lines().count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "---- Minecraft Crash Report ----\n\
// Don't be sad, have a hug!\n\
\n\
Time: 2026-10-26 12:00:00\n\
Description: Rendering overlay\n\
\n\
java.lang.NullPointerException: Cannot invoke \"String.length()\" because \"s\" is null\n\
\tat net.minecraft.client.renderer.GameRenderer.render(GameRenderer.java:900)\n\
\tat net.minecraft.client.Minecraft.runTick(Minecraft.java:1000)\n\
\tat com.example.mymod.RenderHook.onRender(RenderHook.java:42)\n\
\n\
-- System Details --\n\
\tMinecraft Version: 1.20.1\n\
\tJava Version: 17.0.8, Microsoft\n\
\tOperating System: Windows 11\n\
\n\
-- Affected level --\n\
\tLevel name: world\n\
\n\
Caused by: java.lang.IllegalStateException: bad state\n\
\tat com.example.mymod.Core.init(Core.java:7)\n";

    #[test]
    fn parses_header_fields() {
        let r = parse(SAMPLE, false);
        assert!(r.is_crash_report);
        assert_eq!(r.time.as_deref(), Some("2026-10-26 12:00:00"));
        assert_eq!(r.description.as_deref(), Some("Rendering overlay"));
        assert_eq!(r.lines, 21);
    }

    #[test]
    fn parses_exception_and_frames() {
        let r = parse(SAMPLE, false);
        assert_eq!(
            r.exception.as_deref(),
            Some("java.lang.NullPointerException: Cannot invoke \"String.length()\" because \"s\" is null")
        );
        assert!(r.frames.len() >= 2);
        assert!(r.frames[0].starts_with("at net.minecraft"));
    }

    #[test]
    fn parses_cause_chain_and_sections() {
        let r = parse(SAMPLE, false);
        assert_eq!(
            r.cause_chain,
            vec!["java.lang.IllegalStateException: bad state".to_string()]
        );
        assert!(r.sections.iter().any(|s| s == "System Details"));
        assert!(r.sections.iter().any(|s| s == "Affected level"));
    }

    #[test]
    fn parses_system_details() {
        let r = parse(SAMPLE, false);
        assert_eq!(
            r.system.get("Minecraft Version").map(|s| s.as_str()),
            Some("1.20.1")
        );
        assert_eq!(
            r.system.get("Java Version").map(|s| s.as_str()),
            Some("17.0.8, Microsoft")
        );
    }

    #[test]
    fn infers_foreign_packages_but_flags_them() {
        let r = parse(SAMPLE, false);
        assert_eq!(r.suspects_source, "none");
        assert!(r.inferred_packages.iter().any(|p| p == "com.example.mymod"));
        assert!(!r
            .inferred_packages
            .iter()
            .any(|p| p.starts_with("net.minecraft")));
    }

    #[test]
    fn reads_explicit_suspects() {
        let t = "---- Minecraft Crash Report ----\n\
Description: Ticking entity\n\
\n\
Suspected Mods: sodium, iris\n\
\n\
java.lang.RuntimeException: boom\n";
        let r = parse(t, false);
        assert_eq!(r.suspects, vec!["sodium".to_string(), "iris".to_string()]);
        assert_eq!(r.suspects_source, "report");
    }

    #[test]
    fn ignores_suspects_when_report_says_none() {
        let t = "---- Minecraft Crash Report ----\nSuspected Mods: None\n";
        let r = parse(t, false);
        assert!(r.suspects.is_empty());
        assert_eq!(r.suspects_source, "none");
    }

    #[test]
    fn plain_log_is_not_a_crash_report() {
        let r = parse("[12:00:00] [main/INFO]: Loading 42 mods\n", false);
        assert!(!r.is_crash_report);
        assert!(r.time.is_none());
        assert!(r.exception.is_none());
    }
}
