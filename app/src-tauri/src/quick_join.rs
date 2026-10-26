//! 一键进入服务器：启动游戏时直接把服务器地址塞给 Minecraft。
//!
//! 两种方式，取决于游戏版本：
//!
//! - **1.20 及以后**：`--quickPlayMultiplayer <host:port>`，官方参数，
//!   进游戏直接连，连不上会停在多人游戏界面并提示。
//! - **1.19 及以前**：`--server <host> --port <port>`，老参数，效果一样。
//!
//! 这里只负责"算出该加哪些参数"和"把地址校验干净"，不碰进程。
//! 真正启动仍在 `core::spawn_with`。

use crate::servers::ServerEntry;

/// 1.20 起才有 `--quickPlayMultiplayer`。
const QUICK_PLAY_FROM: (u32, u32, u32) = (1, 20, 0);

pub const DEFAULT_PORT: u16 = 25565;

/// 解析出来的进入方式。
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct JoinTarget {
    pub server_id: Option<String>,
    pub name: String,
    pub host: String,
    pub port: u16,
    /// 展示用：`host:port`（端口是默认值时只显示 host）
    pub address: String,
    /// `quickPlay` 或 `legacy`
    pub mode: String,
    /// 要追加到游戏命令行的参数
    pub args: Vec<String>,
}

/// 只解析 `1.20.1` 这种前缀，`1.20.1-forge-47.2.0` 也能吃。
fn version_tuple(version: &str) -> Option<(u32, u32, u32)> {
    let head: String = version
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    if head.is_empty() {
        return None;
    }
    let mut it = head.split('.');
    let a: u32 = it.next()?.parse().ok()?;
    let b: u32 = it.next().unwrap_or("0").parse().unwrap_or(0);
    let c: u32 = it.next().unwrap_or("0").parse().unwrap_or(0);
    Some((a, b, c))
}

/// 这个游戏版本支持 `--quickPlayMultiplayer` 吗。
///
/// 版本号读不出来时返回 `false`，走老参数——老参数从 1.6 就有，
/// 覆盖面比新参数广，猜错时它是更安全的一边。
pub fn supports_quick_play(mc_version: &str) -> bool {
    match version_tuple(mc_version) {
        Some(v) => v >= QUICK_PLAY_FROM,
        None => false,
    }
}

/// 校验主机名。允许域名、IPv4、IPv6 字面量，但不许空白与控制字符。
pub fn validate_host(host: &str) -> Result<String, String> {
    let h = host.trim();
    if h.is_empty() {
        return Err("INVALID_VALUE|服务器地址不能为空".into());
    }
    if h.len() > 253 {
        return Err("INVALID_VALUE|服务器地址太长了".into());
    }
    if h.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("INVALID_VALUE|服务器地址里不能有空格或换行".into());
    }
    if h.contains('/') || h.contains('\\') {
        return Err("INVALID_VALUE|服务器地址里不能有斜杠".into());
    }
    Ok(h.to_string())
}

/// 校验端口。
pub fn validate_port(port: u16) -> Result<u16, String> {
    if port == 0 {
        return Err("INVALID_VALUE|端口必须在 1-65535 之间".into());
    }
    Ok(port)
}

/// 展示用的地址。
pub fn display_address(host: &str, port: u16) -> String {
    if port == DEFAULT_PORT {
        host.to_string()
    } else {
        format!("{host}:{port}")
    }
}

/// 算出要追加的游戏参数。
pub fn args_for(mc_version: &str, host: &str, port: u16) -> Vec<String> {
    if supports_quick_play(mc_version) {
        vec![
            "--quickPlayMultiplayer".to_string(),
            format!("{host}:{port}"),
        ]
    } else {
        vec![
            "--server".to_string(),
            host.to_string(),
            "--port".to_string(),
            port.to_string(),
        ]
    }
}

/// 从"一个地址字符串"（`mc.example.com`、`mc.example.com:25566`、`[::1]:25565`）
/// 拼出进入目标。
pub fn target_from_address(
    mc_version: &str,
    raw: &str,
    name: Option<&str>,
) -> Result<JoinTarget, String> {
    let (host, port) = crate::server::split_address(raw);
    let host = validate_host(&host)?;
    let port = validate_port(port.unwrap_or(DEFAULT_PORT))?;
    let mode = if supports_quick_play(mc_version) {
        "quickPlay"
    } else {
        "legacy"
    };
    Ok(JoinTarget {
        server_id: None,
        name: name
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| display_address(&host, port)),
        address: display_address(&host, port),
        args: args_for(mc_version, &host, port),
        host,
        port,
        mode: mode.to_string(),
    })
}

/// 从收藏列表里的一条拼出进入目标。
pub fn target_from_entry(mc_version: &str, entry: &ServerEntry) -> Result<JoinTarget, String> {
    let host = validate_host(&entry.host)?;
    let port = validate_port(entry.port)?;
    let mode = if supports_quick_play(mc_version) {
        "quickPlay"
    } else {
        "legacy"
    };
    let name = {
        let n = entry.name.trim();
        if n.is_empty() {
            display_address(&host, port)
        } else {
            n.to_string()
        }
    };
    Ok(JoinTarget {
        server_id: Some(entry.id.clone()),
        name,
        address: display_address(&host, port),
        args: args_for(mc_version, &host, port),
        host,
        port,
        mode: mode.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_versions() {
        assert_eq!(version_tuple("1.20.1"), Some((1, 20, 1)));
        assert_eq!(version_tuple("1.7.10"), Some((1, 7, 10)));
        assert_eq!(version_tuple("1.20"), Some((1, 20, 0)));
        assert_eq!(version_tuple("1.20.1-forge-47.2.0"), Some((1, 20, 1)));
        assert_eq!(version_tuple("garbage"), None);
    }

    #[test]
    fn picks_quick_play_only_on_new_versions() {
        assert!(supports_quick_play("1.20"));
        assert!(supports_quick_play("1.21.4"));
        assert!(!supports_quick_play("1.19.4"));
        assert!(!supports_quick_play("1.12.2"));
        // 读不出来时走老参数，老参数覆盖面更广
        assert!(!supports_quick_play("garbage"));
    }

    #[test]
    fn builds_args_for_both_modes() {
        assert_eq!(
            args_for("1.21", "mc.example.com", 25566),
            vec![
                "--quickPlayMultiplayer".to_string(),
                "mc.example.com:25566".to_string()
            ]
        );
        assert_eq!(
            args_for("1.19.4", "mc.example.com", 25566),
            vec![
                "--server".to_string(),
                "mc.example.com".to_string(),
                "--port".to_string(),
                "25566".to_string()
            ]
        );
    }

    #[test]
    fn rejects_bad_hosts_and_ports() {
        assert!(validate_host("").is_err());
        assert!(validate_host("   ").is_err());
        assert!(validate_host("mc example.com").is_err());
        assert!(validate_host("mc\nexample.com").is_err());
        assert!(validate_host("a/b").is_err());
        assert!(validate_host(&"a".repeat(300)).is_err());
        assert_eq!(validate_host(" mc.example.com ").unwrap(), "mc.example.com");
        assert!(validate_port(0).is_err());
        assert_eq!(validate_port(25565).unwrap(), 25565);
    }

    #[test]
    fn target_parses_addresses() {
        let t = target_from_address("1.21", "mc.example.com:25566", None).unwrap();
        assert_eq!(t.host, "mc.example.com");
        assert_eq!(t.port, 25566);
        assert_eq!(t.address, "mc.example.com:25566");
        assert_eq!(t.name, "mc.example.com:25566");
        assert_eq!(t.mode, "quickPlay");

        // 默认端口不显示出来
        let t = target_from_address("1.20.1", "mc.example.com", Some("我的服")).unwrap();
        assert_eq!(t.port, DEFAULT_PORT);
        assert_eq!(t.address, "mc.example.com");
        assert_eq!(t.name, "我的服");

        // IPv6 字面量
        let t = target_from_address("1.21", "[::1]:25565", None).unwrap();
        assert_eq!(t.host, "::1");
        assert_eq!(t.port, 25565);
    }

    #[test]
    fn target_rejects_junk() {
        assert!(target_from_address("1.21", "", None).is_err());
        assert!(target_from_address("1.21", "  ", None).is_err());
        assert!(target_from_address("1.21", "a b:25565", None).is_err());
    }
}
