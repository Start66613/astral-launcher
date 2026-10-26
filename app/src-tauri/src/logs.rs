//! 日志读取与诊断预览（契约 V2 §10）。
//!
//! 只读本地日志，不联网、不上传。`prepare_diagnosis` 只做**脱敏预览**，
//! 真正的上传必须由用户在界面上确认后才发生。

use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LogFile {
    /// 稳定 id，由相对路径的哈希派生
    pub id: String,
    pub name: String,
    /// game | crash | launcher
    pub kind: String,
    pub size_bytes: u64,
    pub modified_at: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LogText {
    pub id: String,
    pub name: String,
    pub text: String,
    pub lines: u32,
    /// 是否因为 limit 只返回了尾部
    pub truncated: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosisPreview {
    pub text: String,
    pub redactions: u32,
    pub bytes: u64,
    pub truncated: bool,
    pub warnings: Vec<String>,
}

fn id_of(rel: &str) -> String {
    use sha1::{Digest, Sha1};
    let mut h = Sha1::new();
    h.update(rel.as_bytes());
    let digest = h.finalize();
    let hex: String = digest.iter().take(4).map(|b| format!("{b:02x}")).collect();
    format!("log_{hex}")
}

fn mtime_iso(p: &Path) -> String {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .map(crate::time::system_time_to_iso)
        .unwrap_or_else(|_| crate::time::now_iso())
}

fn collect(dir: &Path, kind: &str, rel_prefix: &str, out: &mut Vec<(LogFile, PathBuf)>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if !p.is_file() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        let lower = name.to_lowercase();
        if !(lower.ends_with(".log") || lower.ends_with(".txt") || lower.ends_with(".gz")) {
            continue;
        }
        // .gz 不能直接按文本读，暂不列出
        if lower.ends_with(".gz") {
            continue;
        }
        let rel = format!("{rel_prefix}{name}");
        let size = e.metadata().map(|m| m.len()).unwrap_or(0);
        out.push((
            LogFile {
                id: id_of(&rel),
                name,
                kind: kind.to_string(),
                size_bytes: size,
                modified_at: mtime_iso(&p),
            },
            p,
        ));
    }
}

/// 列出实例可读的日志：logs/*.log 与 crash-reports/*.txt
pub fn list(instance_dir: &Path) -> Vec<LogFile> {
    let mut entries: Vec<(LogFile, PathBuf)> = Vec::new();
    collect(&instance_dir.join("logs"), "game", "logs/", &mut entries);
    collect(
        &instance_dir.join("crash-reports"),
        "crash",
        "crash-reports/",
        &mut entries,
    );
    entries.sort_by(|a, b| b.0.modified_at.cmp(&a.0.modified_at));
    entries.into_iter().map(|(f, _)| f).collect()
}

fn resolve(instance_dir: &Path, id: &str) -> Option<PathBuf> {
    let mut entries: Vec<(LogFile, PathBuf)> = Vec::new();
    collect(&instance_dir.join("logs"), "game", "logs/", &mut entries);
    collect(
        &instance_dir.join("crash-reports"),
        "crash",
        "crash-reports/",
        &mut entries,
    );
    entries
        .into_iter()
        .find(|(f, _)| f.id == id)
        .map(|(_, p)| p)
}

/// 读日志尾部；`limit` 为行数上限（默认 400，最大 2000）
pub fn read(instance_dir: &Path, id: &str, limit: Option<u32>) -> Result<LogText, String> {
    let path = resolve(instance_dir, id).ok_or_else(|| "INVALID_PATH|日志不存在".to_string())?;
    let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    // 超大日志只读尾部 4 MiB，避免一次读进内存
    const MAX_READ: u64 = 4 * 1024 * 1024;
    let text = if bytes > MAX_READ {
        use std::io::{Read, Seek, SeekFrom};
        let mut f = std::fs::File::open(&path).map_err(|e| format!("PERMISSION|读取失败|{e}"))?;
        f.seek(SeekFrom::Start(bytes - MAX_READ))
            .map_err(|e| format!("PERMISSION|定位失败|{e}"))?;
        let mut buf = String::new();
        f.read_to_string(&mut buf)
            .map_err(|e| format!("PERMISSION|读取失败|{e}"))?;
        buf
    } else {
        std::fs::read_to_string(&path).map_err(|e| format!("PERMISSION|读取失败|{e}"))?
    };

    let limit = limit.unwrap_or(400).clamp(1, 2000) as usize;
    let all: Vec<&str> = text.lines().collect();
    let truncated = all.len() > limit;
    let tail: Vec<&str> = if truncated {
        all[all.len() - limit..].to_vec()
    } else {
        all.clone()
    };

    Ok(LogText {
        id: id.to_string(),
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        text: tail.join("\n"),
        lines: tail.len() as u32,
        truncated,
    })
}

/// 本地脱敏：token、邮箱、IPv4、Windows 用户目录
pub fn redact(input: &str) -> (String, u32) {
    let mut count = 0u32;
    let mut out = String::with_capacity(input.len());

    for line in input.lines() {
        let mut l = line.to_string();
        let lower = l.to_lowercase();

        // 常见的凭证关键字：整行只保留键名
        for key in [
            "access_token",
            "accessToken",
            "refresh_token",
            "authorization",
            "client_secret",
            "sessiontoken",
            "xuid",
        ] {
            if lower.contains(&key.to_lowercase()) {
                if let Some(pos) = l.to_lowercase().find(&key.to_lowercase()) {
                    let head = &l[..pos];
                    if !head.contains('[') {
                        l = format!("{head}{key}=<redacted>");
                        count += 1;
                    }
                }
                break;
            }
        }

        // 邮箱
        if let Some(at) = l.find('@') {
            let bytes = l.as_bytes();
            let mut start = at;
            while start > 0 && !bytes[start - 1].is_ascii_whitespace() {
                start -= 1;
            }
            let mut end = at + 1;
            while end < bytes.len() && !bytes[end].is_ascii_whitespace() {
                end += 1;
            }
            if end > at + 1 && l[start..end].contains('.') {
                l.replace_range(start..end, "<email>");
                count += 1;
            }
        }

        // Windows 用户目录
        if let Some(pos) = l.find("\\Users\\") {
            let rest = &l[pos + 7..];
            if let Some(end) = rest.find('\\') {
                if end > 0 {
                    let name = rest[..end].to_string();
                    if name != "<user>" {
                        l = l.replacen(&name, "<user>", 1);
                        count += 1;
                    }
                }
            }
        }

        // IPv4：先收集区间，再倒序替换，避免边借边改
        let mut ip_ranges: Vec<(usize, usize)> = Vec::new();
        {
            let bytes = l.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                if bytes[i].is_ascii_digit() {
                    let start = i;
                    let mut dots = 0;
                    let mut j = i;
                    while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b'.') {
                        if bytes[j] == b'.' {
                            dots += 1;
                        }
                        j += 1;
                    }
                    if dots == 3 && j - start >= 7 {
                        ip_ranges.push((start, j));
                    }
                    i = j;
                } else {
                    i += 1;
                }
            }
        }
        for (start, end) in ip_ranges.into_iter().rev() {
            l.replace_range(start..end, "<ip>");
            count += 1;
        }

        out.push_str(&l);
        out.push('\n');
    }

    (out, count)
}

/// 诊断预览：读日志尾部 + 脱敏，不上传
/// 结构化诊断结果
#[derive(serde::Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosisReport {
    pub log_id: String,
    pub log_name: String,
    pub lines: u32,
    pub findings: Vec<Finding>,
    /// 全部脱敏文本（供"查看原文"用，已裁剪）
    pub text: String,
    /// 一句话结论（没有发现问题时是 null，不硬编一个"正常"）
    pub summary: Option<String>,
    /// 命中的最高严重度：error / warn / none
    pub worst: String,
    /// 建议优先做的事（来自命中的规则，去重）
    pub next_steps: Vec<String>,
    /// 说明诊断依据，让玩家知道这不是猜的
    pub basis: String,
    /// 本地处理，未上传
    pub local_only: bool,
}

/// 对实例日志做结构化诊断（读 → 脱敏 → 规则匹配）
pub fn diagnose(
    instance_dir: &Path,
    log_id: Option<&str>,
    max_lines: u32,
) -> Result<DiagnosisReport, String> {
    let files = list(instance_dir);
    let picked = match log_id {
        Some(id) => files
            .iter()
            .find(|f| f.id == id)
            .ok_or_else(|| "INVALID_VALUE|日志不存在".to_string())?,
        None => files
            .iter()
            .find(|f| f.kind == "game")
            .or_else(|| files.first())
            .ok_or_else(|| "INVALID_VALUE|该实例还没有日志".to_string())?,
    };
    let text = read(instance_dir, &picked.id, Some(max_lines))?;
    let (clean, _redactions) = redact(&text.text);
    let findings = classify_text(&clean);
    let worst = if findings.iter().any(|f| f.severity == "error") {
        "error".to_string()
    } else if findings.iter().any(|f| f.severity == "warn") {
        "warn".to_string()
    } else {
        "none".to_string()
    };
    let summary = if findings.is_empty() {
        None
    } else {
        let labels: Vec<&str> = findings.iter().take(3).map(|f| f.label).collect();
        Some(format!(
            "日志里有 {} 处可疑，最可能是：{}",
            findings.len(),
            labels.join("、")
        ))
    };
    let mut next_steps: Vec<String> = Vec::new();
    for f in &findings {
        let s = f.hint.to_string();
        if !next_steps.contains(&s) {
            next_steps.push(s);
        }
    }
    next_steps.truncate(4);
    Ok(DiagnosisReport {
        log_id: picked.id.clone(),
        log_name: picked.name.clone(),
        lines: text.lines,
        findings,
        text: clean,
        summary,
        worst,
        next_steps,
        basis: format!(
            "按 {} 条本地规则匹配日志原文（已脱敏 {} 处），未上传任何内容",
            RULES.len(),
            _redactions
        ),
        local_only: true,
    })
}
/// 结构化诊断的一条结论
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub id: &'static str,
    pub label: &'static str,
    /// info | warn | error
    pub severity: &'static str,
    /// 命中的日志行（已脱敏，最多 3 条）
    pub evidence: Vec<String>,
    /// 可能的原因（基于规则，不是猜测）
    pub cause: &'static str,
    /// 给玩家的下一步建议
    pub hint: &'static str,
    /// high | medium | low —— 这条结论有多可信
    pub confidence: &'static str,
    /// 这类问题通常由什么引起，方便玩家自己判断
    pub category: &'static str,
}

struct Rule {
    id: &'static str,
    label: &'static str,
    severity: &'static str,
    cause: &'static str,
    hint: &'static str,
    confidence: &'static str,
    category: &'static str,
    patterns: &'static [&'static str],
}

const RULES: &[Rule] = &[
    Rule {
        id: "java-version",
        label: "Java 版本不匹配",
        severity: "error",
        cause: "游戏用了一个版本不对的 Java：类文件版本高于运行时支持的范围",
        hint: "换用该版本要求的 Java（实例设置里可指定 javaPath）",
        confidence: "high",
        category: "java",
        patterns: &[
            "UnsupportedClassVersionError",
            "requires Java",
            "class file version",
        ],
    },
    Rule {
        id: "missing-class",
        label: "缺少类或依赖库",
        severity: "error",
        cause: "依赖库缺失、下载不完整，或加载器装到了错误的版本上",
        hint: "用「修复实例」补全依赖库；仍失败请检查加载器是否装对",
        confidence: "high",
        category: "files",
        patterns: &[
            "NoClassDefFoundError",
            "ClassNotFoundException",
            "Could not find or load main class",
            "java.lang.NoSuchMethodError",
            "java.lang.NoSuchFieldError",
        ],
    },
    Rule {
        id: "mod-conflict",
        label: "模组冲突或重复",
        severity: "error",
        cause: "同一个模组装了两份，或两个模组改同一处逻辑",
        hint: "移除重复/不兼容的模组；看日志里被点名的 mod id",
        confidence: "high",
        category: "mods",
        patterns: &[
            "Duplicate mods",
            "Incompatible mod set",
            "ModResolutionException",
            "Duplicate entry",
            "already registered",
            "Duplicate mod",
        ],
    },
    Rule {
        id: "duplicate-mod-id",
        label: "模组 ID 重复",
        severity: "error",
        cause: "mods 目录里存在同一个 modId 的两个 jar（常见于手动更新后没删旧版）",
        hint: "打开 mods 目录，删掉旧版本 jar；「批量更新」会自动处理这种情况",
        confidence: "high",
        category: "mods",
        patterns: &[
            "Duplicate mod id",
            "duplicate modid",
            "DuplicateModId",
            "Found duplicate mod",
        ],
    },
    Rule {
        id: "out-of-memory",
        label: "内存不足",
        severity: "error",
        cause: "分配的堆内存不够，或大型整合包 + 光影同时加载",
        hint: "调大实例内存（建议 2–4 GB），或减少大型整合包",
        confidence: "high",
        category: "memory",
        patterns: &["OutOfMemoryError", "Java heap space", "GC overhead limit"],
    },
    Rule {
        id: "graphics",
        label: "显卡/窗口初始化失败",
        severity: "error",
        cause: "显卡驱动过旧、笔记本用了核显，或 OpenGL 版本不够",
        hint: "更新显卡驱动；笔记本请确认用独显运行；必要时关闭光影",
        confidence: "high",
        category: "graphics",
        patterns: &[
            "GLFW error",
            "Failed to create window",
            "Pixel format not accelerated",
            "OpenGL",
            "A fatal error has been detected",
            "EXCEPTION_ACCESS_VIOLATION",
        ],
    },
    Rule {
        id: "loader-mismatch",
        label: "加载器版本不兼容",
        severity: "error",
        cause: "加载器版本与游戏版本不匹配，或加载器文件被另一个版本覆盖",
        hint: "在实例设置里重装对应版本的加载器；不要混用不同加载器",
        confidence: "medium",
        category: "loader",
        patterns: &[
            "LoaderException",
            "Unsupported class file major version",
            "fabric-loader",
            "Mixin apply failed",
            "MixinTransformerError",
            "Could not find valid Forge",
            "Missing or unsupported mandatory dependencies",
        ],
    },
    Rule {
        id: "config-corrupt",
        label: "配置文件损坏",
        severity: "warn",
        cause: "配置文件写入中断或被手动改坏，常见于强退游戏后",
        hint: "删掉日志里点名的那个配置文件（通常在 config/ 下），游戏会重新生成",
        confidence: "medium",
        category: "config",
        patterns: &[
            "JsonSyntaxException",
            "MalformedJsonException",
            "com.google.gson.stream.MalformedJsonException",
            "Couldn't parse",
            "Failed to load config",
        ],
    },
    Rule {
        id: "download",
        label: "下载或校验失败",
        severity: "warn",
        cause: "网络中断、镜像不稳定，或本地文件与哈希对不上",
        hint: "检查网络/镜像设置后重试；同目录可能留有 .part 临时文件",
        confidence: "high",
        category: "network",
        patterns: &[
            "Failed to download",
            "Couldn't download",
            "Checksum",
            "HASH_MISMATCH",
            "Connection reset",
            "Connect timed out",
        ],
    },
    Rule {
        id: "auth",
        label: "登录凭据问题",
        severity: "warn",
        cause: "登录令牌过期、账号名不合法，或正版验证没配好",
        hint: "重新输入用户名；正版登录需先配置微软应用",
        confidence: "medium",
        category: "account",
        patterns: &["Invalid credentials", "Authentication", "Failed to login"],
    },
    Rule {
        id: "permission",
        label: "文件权限不足",
        severity: "error",
        cause: "游戏目录在受保护位置（如 Program Files），或文件被其他程序占用",
        hint: "把游戏目录换到普通文件夹（如 D:\\Games），并关掉占用文件的程序",
        confidence: "medium",
        category: "filesystem",
        patterns: &[
            "Access is denied",
            "Permission denied",
            "java.io.IOException: Access",
            "The process cannot access the file",
        ],
    },
    Rule {
        id: "disk-full",
        label: "磁盘空间不足",
        severity: "error",
        cause: "游戏目录所在磁盘写满，下载或解压中途失败",
        hint: "清理磁盘空间后重试；「存储」页面可以看到各目录占用",
        confidence: "high",
        category: "filesystem",
        patterns: &["No space left on device", "not enough space", "DISK_FULL"],
    },
];

/// 对纯文本日志做规则匹配（可单测，不读文件）
pub fn classify_text(text: &str) -> Vec<Finding> {
    let mut out: Vec<Finding> = Vec::new();
    for rule in RULES {
        let mut evidence: Vec<String> = Vec::new();
        for line in text.lines() {
            if evidence.len() >= 3 {
                break;
            }
            let lower = line.to_ascii_lowercase();
            if rule
                .patterns
                .iter()
                .any(|p| lower.contains(&p.to_ascii_lowercase()))
            {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if trimmed.len() > 240 {
                    evidence.push(trimmed.chars().take(240).collect());
                } else {
                    evidence.push(trimmed.to_string());
                }
            }
        }
        if !evidence.is_empty() {
            out.push(Finding {
                id: rule.id,
                label: rule.label,
                severity: rule.severity,
                evidence,
                cause: rule.cause,
                hint: rule.hint,
                confidence: rule.confidence,
                category: rule.category,
            });
        }
    }
    // 错误优先展示，同级按证据条数多的在前
    out.sort_by(|a, b| {
        let rank = |s: &str| match s {
            "error" => 0,
            "warn" => 1,
            _ => 2,
        };
        rank(a.severity)
            .cmp(&rank(b.severity))
            .then(b.evidence.len().cmp(&a.evidence.len()))
    });
    out
}

pub fn prepare(
    instance_dir: &Path,
    log_ids: &[String],
    max_lines: u32,
) -> Result<DiagnosisPreview, String> {
    if log_ids.is_empty() {
        return Err("INVALID_VALUE|至少选择一个日志".into());
    }
    let mut merged = String::new();
    for id in log_ids.iter().take(5) {
        let t = read(instance_dir, id, Some(max_lines))?;
        merged.push_str(&format!("===== {} =====\n{}\n", t.name, t.text));
    }
    let (text, redactions) = redact(&merged);
    let truncated =
        text.lines().count() as u32 >= max_lines.saturating_mul(log_ids.len().min(5) as u32);
    let mut warnings = Vec::new();
    if redactions > 0 {
        warnings.push(format!("已在本机脱敏 {redactions} 处敏感信息"));
    }
    if text.len() > 200_000 {
        warnings.push("内容较多，建议只上传选中片段".into());
    }
    Ok(DiagnosisPreview {
        bytes: text.len() as u64,
        text,
        redactions,
        truncated,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_failures() {
        let text = "\
java.lang.UnsupportedClassVersionError: 65.0
Caused by: java.lang.NoClassDefFoundError: net/minecraft/client/Minecraft
java.lang.OutOfMemoryError: Java heap space
GLFW error 65542: WGL: The driver does not appear to support OpenGL
Duplicate mods found: sodium
";
        let found = classify_text(text);
        let ids: Vec<&str> = found.iter().map(|f| f.id).collect();
        assert!(ids.contains(&"java-version"));
        assert!(ids.contains(&"missing-class"));
        assert!(ids.contains(&"out-of-memory"));
        assert!(ids.contains(&"graphics"));
        assert!(ids.contains(&"mod-conflict"));
        assert!(found.iter().all(|f| f.evidence.len() <= 3));
    }

    #[test]
    fn findings_carry_cause_and_confidence() {
        let text = "\
java.lang.UnsupportedClassVersionError: net/minecraft/client/Minecraft has been compiled by a more recent version
java.lang.OutOfMemoryError: Java heap space
";
        let found = classify_text(text);
        let java = found.iter().find(|f| f.id == "java-version").unwrap();
        assert_eq!(java.confidence, "high");
        assert_eq!(java.category, "java");
        assert!(!java.cause.is_empty());
        // error 排在 warn 前面
        assert_eq!(found.first().map(|f| f.severity), Some("error"));
    }

    #[test]
    fn new_rules_cover_permission_and_duplicate_mod() {
        let text = "\
java.io.IOException: Access is denied
Found duplicate mod id: sodium
java.lang.ExceptionInInitializerError: Mixin apply failed
";
        let ids: Vec<&str> = classify_text(text).iter().map(|f| f.id).collect();
        assert!(ids.contains(&"permission"));
        assert!(ids.contains(&"duplicate-mod-id"));
        assert!(ids.contains(&"loader-mismatch"));
    }

    #[test]
    fn no_findings_on_clean_log() {
        let clean = "[12:00:00] [main/INFO]: Loading Minecraft 1.20.1\n";
        assert!(classify_text(clean).is_empty());
    }
    #[test]
    fn redacts_email_and_ip() {
        let (out, n) = redact("contact me at steve@example.com from 192.168.1.20");
        assert!(out.contains("<email>"));
        assert!(out.contains("<ip>"));
        assert!(n >= 2);
    }

    #[test]
    fn redacts_token_line() {
        let (out, _) = redact("Authorization: Bearer abcdef123456");
        assert!(out.contains("<redacted>"));
    }

    #[test]
    fn redacts_user_dir() {
        let (out, _) = redact("C:\\Users\\alice\\AppData\\Roaming\\.minecraft");
        assert!(out.contains("<user>"));
    }
}
