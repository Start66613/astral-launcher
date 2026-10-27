//! 实例的「游戏参数」：追加在游戏主类之后的那一串。
//!
//! 和 `jvm` 模块分工：`jvm` 管 `-Xmx`、`-XX:` 这些给 Java 的；这里管给
//! Minecraft 自己的，比如 `--width 1920 --height 1080`、`-Dfml.queryResult=confirm`。
//!
//! 之前实例只有 `jvmArgs`，想改窗口大小、跳过 Forge 的首次确认对话框，玩家
//! 只能自己去翻文档然后发现启动器根本没地方填。
//!
//! 但有几种参数**不能让玩家填**：用户名、UUID、accessToken、游戏目录、资源
//! 目录这些是启动器负责拼的，玩家再写一遍会直接顶掉认证信息，游戏要么进不去
//! 要么变成离线状态。这些一律拒绝，并说清楚为什么。

use serde::Serialize;

/// 最多允许的自定义游戏参数条数。
pub const MAX_ARGS: usize = 32;

/// 单条参数的最大长度。
pub const MAX_ARG_LEN: usize = 512;

/// 启动器自己负责、玩家不许覆盖的参数。
///
/// 这些拼错了不是"启动失败"，而是认证信息被顶掉、游戏进了离线模式却看不出来，
/// 所以宁可在这里拦住。
///
/// 注意 `--width` / `--height` **不在**这里：启动器没给游戏设分辨率
/// （`portablemc` 的 `set_resolution` 我们没调），改窗口大小正是玩家最想
/// 自己配的一项，拦掉就没意义了。
const RESERVED: [&str; 12] = [
    "--username",
    "--uuid",
    "--accessToken",
    "--clientId",
    "--xuid",
    "--userType",
    "--userProperties",
    "--gameDir",
    "--assetsDir",
    "--assetIndex",
    "--version",
    "--versionType",
];

/// 一条问题。
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ArgIssue {
    /// `error` 挡住启动，`warn` 只提示
    pub severity: String,
    pub message: String,
}

/// 检查结果。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ArgsCheck {
    /// 去掉空项后的参数
    pub args: Vec<String>,
    pub issues: Vec<ArgIssue>,
    /// 没有 error 级问题
    pub ok: bool,
}

fn issue(severity: &str, message: impl Into<String>) -> ArgIssue {
    ArgIssue {
        severity: severity.to_string(),
        message: message.into(),
    }
}

/// 去掉首尾空白与空项。
pub fn normalize(args: &[String]) -> Vec<String> {
    args.iter()
        .map(|a| a.trim())
        .filter(|a| !a.is_empty())
        .map(|a| a.to_string())
        .collect()
}

/// 参数名部分（`--width=1920` 取 `--width`）。
fn flag_of(arg: &str) -> &str {
    match arg.find('=') {
        Some(i) => &arg[..i],
        None => arg,
    }
}

/// 纯函数检查，方便测试。
pub fn check(args: &[String]) -> ArgsCheck {
    let args = normalize(args);
    let mut issues = Vec::new();

    if args.len() > MAX_ARGS {
        issues.push(issue(
            "error",
            format!("游戏参数最多 {MAX_ARGS} 条，现在有 {} 条。", args.len()),
        ));
    }

    for arg in &args {
        if arg.chars().count() > MAX_ARG_LEN {
            issues.push(issue(
                "error",
                format!("有一条参数太长（超过 {MAX_ARG_LEN} 个字符），请检查是不是贴错了。"),
            ));
            continue;
        }
        if arg.contains('\n') || arg.contains('\r') {
            issues.push(issue(
                "error",
                "参数里不能有换行。一行一条，或者用空格分开。".to_string(),
            ));
            continue;
        }
        let flag = flag_of(arg);
        if RESERVED.contains(&flag) {
            issues.push(issue(
                "error",
                format!("「{flag}」由启动器负责填，写在这里会把登录信息顶掉，游戏可能进离线模式。"),
            ));
        }
    }

    // 只提醒，不拦：玩家确实可能想固定进某个服，那和"一键进服"同时用就是后者生效
    if args.iter().any(|a| flag_of(a) == "--server") {
        issues.push(issue(
            "warn",
            "你自己写了 --server，用「一键进服」启动时这两个会重复，以一键进服的为准。".to_string(),
        ));
    }

    let ok = !issues.iter().any(|i| i.severity == "error");
    ArgsCheck { args, issues, ok }
}

/// 校验并返回清洗后的参数；有 error 时返回错误。
pub fn validate(args: &[String]) -> Result<Vec<String>, String> {
    let checked = check(args);
    if let Some(bad) = checked.issues.iter().find(|i| i.severity == "error") {
        return Err(format!("INVALID_VALUE|游戏参数有问题：{}", bad.message));
    }
    Ok(checked.args)
}

/// 每个实例一份，存在实例目录里（和 `.astral-ledger.json` 一类旁挂文件）。
///
/// 不塞进 `instance.json` 是因为那份是实例的"身份"（名字、版本、加载器），
/// 参数属于可随时改的偏好，分开放两边互不影响。
pub const FILE: &str = "game-args.json";

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Stored {
    #[serde(default)]
    args: Vec<String>,
}

/// 读某个实例自己配的游戏参数。文件不存在或读不出来都当"没配过"。
pub fn load(instance_dir: &std::path::Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(instance_dir.join(FILE)) else {
        return Vec::new();
    };
    serde_json::from_str::<Stored>(&text)
        .map(|s| normalize(&s.args))
        .unwrap_or_default()
}

/// 写参数。先写 `.part` 再改名，避免半截 JSON。
pub fn save(instance_dir: &std::path::Path, args: &[String]) -> Result<Vec<String>, String> {
    let args = validate(args)?;
    let text = serde_json::to_string_pretty(&Stored { args: args.clone() })
        .map_err(|e| format!("INTERNAL|序列化失败|{e}"))?;
    let path = instance_dir.join(FILE);
    let tmp = path.with_extension("json.part");
    std::fs::write(&tmp, text).map_err(|e| format!("PERMISSION|写入参数失败|{e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("PERMISSION|保存参数失败|{e}")
    })?;
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_normal_args() {
        let args = vec![
            "--width".to_string(),
            "1920".to_string(),
            "--height=1080".to_string(),
        ];
        let c = check(&args);
        assert!(c.ok);
        assert_eq!(c.args.len(), 3);
        assert!(c.issues.is_empty());
    }

    #[test]
    fn rejects_reserved_flags() {
        let c = check(&["--username".into(), "Steve".into()]);
        assert!(!c.ok);
        assert!(c.issues.iter().any(|i| i.message.contains("--username")));

        // 带等号的写法也要拦住
        let c2 = check(&["--accessToken=abc".into()]);
        assert!(!c2.ok);

        let c3 = check(&["--gameDir".into(), "/tmp".into()]);
        assert!(!c3.ok);
    }

    #[test]
    fn rejects_newlines_and_long_args() {
        assert!(!check(&["a\nb".into()]).ok);
        assert!(!check(&["x".repeat(MAX_ARG_LEN + 1)]).ok);
    }

    #[test]
    fn rejects_too_many_args() {
        let many: Vec<String> = (0..MAX_ARGS + 1).map(|i| format!("--a{i}")).collect();
        assert!(!check(&many).ok);
        let just_enough: Vec<String> = (0..MAX_ARGS).map(|i| format!("--a{i}")).collect();
        assert!(check(&just_enough).ok);
    }

    #[test]
    fn warns_about_server_but_does_not_block() {
        let c = check(&["--server".into(), "play.example.com".into()]);
        assert!(c.ok);
        assert!(c.issues.iter().any(|i| i.severity == "warn"));
    }

    #[test]
    fn normalizes_whitespace_and_empty_args() {
        let c = check(&["  --width  ".into(), "".into(), "  ".into()]);
        assert_eq!(c.args, vec!["--width"]);
    }

    #[test]
    fn validate_returns_first_error() {
        let e = validate(&["--uuid".into(), "x".into()]).unwrap_err();
        assert!(e.starts_with("INVALID_VALUE|"));
        assert!(validate(&["--width".into(), "800".into()]).is_ok());
    }

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d =
            std::env::temp_dir().join(format!("astral-ga-{tag}-{:x}", crate::time::now_nanos()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn saves_and_loads_per_instance() {
        let d = tmp("ok");
        assert!(load(&d).is_empty());
        let saved = save(&d, &["--width".into(), "1920".into()]).unwrap();
        assert_eq!(saved.len(), 2);
        assert_eq!(load(&d), vec!["--width", "1920"]);
        // 再改一次，旧值要被顶掉而不是叠加
        save(&d, &["--height".into(), "1080".into()]).unwrap();
        assert_eq!(load(&d), vec!["--height", "1080"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn save_refuses_reserved_flags_and_writes_nothing() {
        let d = tmp("bad");
        let e = save(&d, &["--accessToken".into(), "abc".into()]).unwrap_err();
        assert!(e.starts_with("INVALID_VALUE|"));
        // 校验失败不能留下半截文件
        assert!(!d.join(FILE).exists());
        assert!(load(&d).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn broken_file_reads_as_empty() {
        let d = tmp("broken");
        std::fs::write(d.join(FILE), b"{not json").unwrap();
        assert!(load(&d).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}
