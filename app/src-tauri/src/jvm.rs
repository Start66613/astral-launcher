//! 实例的 Java 运行参数：内存与自定义 JVM 参数。
//!
//! 之前 `update_instance` 对 `memoryMb` 和 `jvmArgs` **一个字符都不校验**：
//! 前端传 0 会生成 `-Xmx0M`，传 999999 会让 Java 直接拒绝启动，参数里再写
//! 一个 `-Xmx` 会和我们自己生成的那个打架（谁在后谁生效，玩家完全看不懂）。
//! 这些错误都要等到点启动、Java 报一串英文才暴露。
//!
//! 这个模块把「什么算能用」写在一处，写配置时校验，启动前预检也复用同一套
//! 判断，两边不会各说各话。

use serde::Serialize;

/// Java 能起来的最低内存。再低连 JVM 初始化都过不去。
pub const MIN_MEMORY_MB: u32 = 512;

/// 绝对上限。普通整合包用不到这么多，给多了 GC 停顿反而更长。
pub const MAX_MEMORY_MB: u32 = 32768;

/// 最多允许的自定义参数条数。
pub const MAX_ARGS: usize = 32;

/// 单条参数的最大长度。
pub const MAX_ARG_LEN: usize = 512;

/// 一条问题。
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct JvmIssue {
    /// `memory` 或 `args`
    pub field: String,
    /// `error` 挡住启动，`warn` 只提示
    pub severity: String,
    pub message: String,
}

/// 检查结果。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct JvmCheck {
    /// 实例里设置的值（null = 没设过，用默认）
    pub memory_mb: Option<u32>,
    /// 实际会传给 Java 的 -Xmx 值（没设就是 null，由启动器默认决定）
    pub effective_mb: Option<u32>,
    /// 去掉空项后的参数
    pub args: Vec<String>,
    /// 这台机器的物理内存（MB）
    pub system_total_mb: u64,
    /// 这台机器上说得通的内存上限
    pub max_mb: u32,
    pub issues: Vec<JvmIssue>,
    /// 没有 error 级问题
    pub ok: bool,
}

fn issue(field: &str, severity: &str, message: impl Into<String>) -> JvmIssue {
    JvmIssue {
        field: field.to_string(),
        severity: severity.to_string(),
        message: message.into(),
    }
}

/// 这台机器的物理内存（MB）。
pub fn system_memory_mb() -> u64 {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    sys.total_memory() / (1024 * 1024)
}

/// 这台机器上说得通的内存上限：物理内存的四分之三，按 512 MB 取整。
///
/// 不给满是因为 Windows 自己还要用；四分之三已经足够激进。
pub fn max_mb_for(total_mb: u64) -> u32 {
    if total_mb == 0 {
        return MAX_MEMORY_MB;
    }
    let three_quarters = total_mb / 4 * 3;
    let rounded = (three_quarters / 512) * 512;
    rounded.clamp(MIN_MEMORY_MB as u64, MAX_MEMORY_MB as u64) as u32
}

/// 去掉首尾空白与空项。玩家从别处复制参数常常带换行和多余空格。
pub fn normalize_args(args: &[String]) -> Vec<String> {
    args.iter()
        .map(|a| a.trim())
        .filter(|a| !a.is_empty())
        .map(|a| a.to_string())
        .collect()
}

/// 纯函数版本，方便测试。`total_mb` 传 0 表示不知道系统内存（跳过与物理内存的比较）。
pub fn check(memory_mb: Option<u32>, args: &[String], total_mb: u64) -> JvmCheck {
    let mut issues = Vec::new();
    let max_mb = max_mb_for(total_mb);
    let args = normalize_args(args);

    if let Some(mb) = memory_mb {
        if mb == 0 {
            issues.push(issue(
                "memory",
                "error",
                "内存不能是 0，Java 起不来。建议设成 2048 MB 以上。",
            ));
        } else if mb < MIN_MEMORY_MB {
            issues.push(issue(
                "memory",
                "error",
                format!(
                    "内存 {mb} MB 太少，Java 起不来。至少要 {MIN_MEMORY_MB} MB，建议 2048 MB 以上。"
                ),
            ));
        } else if mb > MAX_MEMORY_MB {
            issues.push(issue(
                "memory",
                "error",
                format!("内存 {mb} MB 超过上限 {MAX_MEMORY_MB} MB。"),
            ));
        } else if total_mb > 0 && u64::from(mb) > total_mb {
            issues.push(issue(
                "memory",
                "error",
                format!("内存 {mb} MB 比这台机器的物理内存（{total_mb} MB）还大，启动会失败。"),
            ));
        } else if total_mb > 0 && u64::from(mb) > u64::from(max_mb) {
            issues.push(issue(
                "memory",
                "warn",
                format!(
                    "内存 {mb} MB 超过这台机器建议的上限 {max_mb} MB。给 Java 太多，\
                     系统会开始用虚拟内存，反而更卡。"
                ),
            ));
        }
    }

    if args.len() > MAX_ARGS {
        issues.push(issue(
            "args",
            "error",
            format!("JVM 参数最多 {MAX_ARGS} 条，现在是 {} 条。", args.len()),
        ));
    }

    for raw in args.iter() {
        let arg = raw.as_str();
        if arg.len() > MAX_ARG_LEN {
            issues.push(issue(
                "args",
                "error",
                format!("参数太长（超过 {MAX_ARG_LEN} 个字符）：{}…", head(arg, 24)),
            ));
            continue;
        }
        if arg.contains('\n') || arg.contains('\r') {
            issues.push(issue(
                "args",
                "error",
                format!("参数里不能有换行：{}", head(arg, 24)),
            ));
            continue;
        }
        if arg.starts_with("-Xmx") || arg.starts_with("-Xms") || arg.starts_with("-Xss") {
            issues.push(issue(
                "args",
                "error",
                format!(
                    "内存请在上面「内存」里设置，不要写进 JVM 参数：{arg}。\
                     两边都写的话后生效的那个说了算，很难排查。"
                ),
            ));
            continue;
        }
        if arg.matches('"').count() % 2 == 1 {
            issues.push(issue("args", "error", format!("引号没配对：{arg}")));
            continue;
        }
        if let Some(path) = arg.strip_prefix("-javaagent:") {
            let path = path.split('=').next().unwrap_or(path);
            if !path.is_empty() && !std::path::Path::new(path).exists() {
                issues.push(issue(
                    "args",
                    "warn",
                    format!("找不到 {path}。这条参数指的文件不在，加上它游戏会启动失败。"),
                ));
            }
        }
    }

    let ok = !issues.iter().any(|i| i.severity == "error");
    JvmCheck {
        memory_mb,
        effective_mb: memory_mb,
        args,
        system_total_mb: total_mb,
        max_mb,
        issues,
        ok,
    }
}

/// 校验并返回规范化后的参数。有 error 就返回编码错误。
pub fn validate(
    memory_mb: Option<u32>,
    args: &[String],
    total_mb: u64,
) -> Result<Vec<String>, String> {
    let result = check(memory_mb, args, total_mb);
    if let Some(first) = result.issues.iter().find(|i| i.severity == "error") {
        let field = if first.field == "memory" {
            "内存"
        } else {
            "JVM 参数"
        };
        return Err(format!(
            "INVALID_VALUE|{field}设置有问题：{}",
            first.message
        ));
    }
    Ok(result.args)
}

fn head(text: &str, n: usize) -> String {
    text.chars().take(n).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn accepts_sane_memory() {
        let c = check(Some(4096), &[], 16384);
        assert!(c.ok);
        assert!(c.issues.is_empty());
        assert_eq!(c.effective_mb, Some(4096));
    }

    #[test]
    fn rejects_zero_and_tiny_memory() {
        let zero = check(Some(0), &[], 16384);
        assert!(!zero.ok);
        assert!(zero.issues[0].message.contains("不能是 0"));

        let tiny = check(Some(256), &[], 16384);
        assert!(!tiny.ok);
        assert!(tiny.issues[0].message.contains("太少"));
    }

    #[test]
    fn rejects_memory_bigger_than_the_machine() {
        // 20000 MB 没超过绝对上限，但比这台机器的 16 GB 物理内存还大
        let c = check(Some(20000), &[], 16384);
        assert!(!c.ok);
        assert!(c.issues.iter().any(|i| i.message.contains("物理内存")));

        // 超过绝对上限时先报上限
        let c = check(Some(65536), &[], 65536);
        assert!(!c.ok);
        assert!(c.issues.iter().any(|i| i.message.contains("上限")));
    }

    #[test]
    fn warns_above_the_suggested_ceiling() {
        // 8 GB 机器：建议上限 6144
        let c = check(Some(7168), &[], 8192);
        assert!(c.ok, "超过建议上限只是提醒，不挡启动");
        assert!(c.issues.iter().any(|i| i.severity == "warn"));
        assert_eq!(c.max_mb, 6144);
    }

    #[test]
    fn unknown_system_memory_skips_the_comparison() {
        let c = check(Some(4096), &[], 0);
        assert!(c.ok);
        assert_eq!(c.max_mb, MAX_MEMORY_MB);
    }

    #[test]
    fn rejects_memory_flags_in_custom_args() {
        let c = check(None, &s(&["-Xmx8G"]), 16384);
        assert!(!c.ok);
        assert!(c.issues[0].message.contains("内存请在上面"));

        let c = check(None, &s(&["-Xms512M"]), 16384);
        assert!(!c.ok);
    }

    #[test]
    fn rejects_newlines_and_unbalanced_quotes() {
        assert!(!check(None, &s(&["-Dfoo=a\nb"]), 0).ok);
        assert!(!check(None, &s(&["-Dfoo=\"bar"]), 0).ok);
        assert!(check(None, &s(&["-Dfoo=\"bar\""]), 0).ok);
    }

    #[test]
    fn rejects_too_many_and_too_long_args() {
        let many: Vec<String> = (0..40).map(|i| format!("-Dk{i}=v")).collect();
        assert!(!check(None, &many, 0).ok);

        let long = format!("-Dk={}", "x".repeat(MAX_ARG_LEN + 1));
        assert!(!check(None, &s(&[long.as_str()]), 0).ok);
    }

    #[test]
    fn warns_about_missing_javaagent() {
        let c = check(None, &s(&["-javaagent:C:/nope/agent.jar"]), 0);
        assert!(c.ok, "找不到文件只提醒，不挡启动");
        assert_eq!(c.issues.len(), 1);
        assert_eq!(c.issues[0].severity, "warn");
    }

    #[test]
    fn normalizes_whitespace_and_empty_args() {
        let c = check(None, &s(&["  -XX:+UseG1GC  ", "", "   ", "-Dfoo=1"]), 0);
        assert_eq!(c.args, s(&["-XX:+UseG1GC", "-Dfoo=1"]));
        assert!(c.ok);
    }

    #[test]
    fn validate_returns_the_first_error() {
        let e = validate(Some(0), &s(&["-Xmx1G"]), 16384).unwrap_err();
        assert!(e.starts_with("INVALID_VALUE|内存设置有问题"));
        assert!(e.contains("不能是 0"));

        let e = validate(None, &s(&["-Xmx1G"]), 16384).unwrap_err();
        assert!(e.starts_with("INVALID_VALUE|JVM 参数设置有问题"));
    }

    #[test]
    fn max_memory_tracks_the_machine() {
        assert_eq!(max_mb_for(8192), 6144);
        assert_eq!(max_mb_for(16384), 12288);
        // 很小的机器也不会低于下限
        assert_eq!(max_mb_for(512), MIN_MEMORY_MB);
    }
}
