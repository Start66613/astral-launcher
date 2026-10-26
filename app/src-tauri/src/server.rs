//! 服务器状态查询（Minecraft Server List Ping）。
//!
//! 玩家填一个地址，想知道"这个服现在能不能进、有几个人、是什么版本"。
//! 这里直接说 Minecraft 的服务器列表协议（1.7+ 的 modern ping，老服退回 legacy），
//! 不经过任何第三方网站，也不会把地址传给任何人。
//!
//! 设计取舍：
//! - **联网失败不算错误**。地址写错、服务器没开、防火墙拦了，都是玩家会遇到的
//!   正常情况。这时返回 `online=false` 加一句原因，前端直接显示"离线"就行，
//!   不用把 `invoke` 包在 try/catch 里区分。
//! - **只有参数本身不合法才返回 Err**（空地址、端口 0），那是调用方的 bug。
//! - 延迟量的是"握手 + 状态查询"整个来回，不是单独的 ping 包 —— 多数服务器
//!   回完状态就断开，再要一个 pong 容易白等一次超时。

use serde::Serialize;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

/// 不写端口时的默认值。
pub const DEFAULT_PORT: u16 = 25565;
/// 默认超时。玩家点一下最多等 4 秒，再长就像卡死了。
pub const DEFAULT_TIMEOUT_MS: u64 = 4_000;
/// 地址最长多少字符（DNS 域名上限 253）。
pub const MAX_HOST_LEN: usize = 253;
/// 状态响应最大字节数。真实响应带 favicon 也就几十 KB，1 MiB 是防坏服务器。
pub const MAX_RESPONSE: usize = 1024 * 1024;

/// 一次查询的结果。**永远返回结构体**，联网失败时 `online=false`。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
    /// 规范化之后的地址（去掉了首尾空格和末尾的点）
    pub host: String,
    pub port: u16,
    /// 是否成功拿到状态
    pub online: bool,
    /// 握手 + 状态查询耗时（毫秒）
    pub latency_ms: u64,
    pub version_name: Option<String>,
    pub version_protocol: Option<i32>,
    pub players_online: Option<u32>,
    pub players_max: Option<u32>,
    /// 在线玩家名（服务器只给一部分，通常是前几个）
    pub sample: Vec<String>,
    /// 去掉 `§` 颜色代码之后的纯文本，可以直接显示
    pub motd: String,
    /// 原始 MOTD（带颜色代码），想自己渲染颜色时用
    pub motd_raw: String,
    /// 服务器图标，`data:image/png;base64,...`；没有就是 None
    pub favicon: Option<String>,
    /// 是不是走的老协议（1.6 及更早）
    pub legacy: bool,
    /// 离线原因，`online=false` 时一定有值
    pub error: Option<String>,
}

/// 把 `example.com:25566`、`[::1]:25565` 这种写法拆成主机 + 端口。
///
/// 玩家习惯直接粘贴带端口的地址，不该让他再单独填一个框。
pub fn split_address(input: &str) -> (String, Option<u16>) {
    let s = input.trim();
    if let Some(rest) = s.strip_prefix('[') {
        // IPv6 字面量：[::1] 或 [::1]:25565
        if let Some(end) = rest.find(']') {
            let host = &rest[..end];
            let tail = rest[end + 1..].trim_start_matches(':');
            let port = tail.parse::<u16>().ok();
            return (host.to_string(), port);
        }
        return (s.to_string(), None);
    }
    // 只有恰好一个冒号才当成 host:port；多冒号说明是裸 IPv6，不能乱拆
    if s.matches(':').count() == 1 {
        let (host, tail) = s.split_once(':').unwrap();
        if let Ok(port) = tail.parse::<u16>() {
            return (host.to_string(), Some(port));
        }
    }
    (s.to_string(), None)
}

fn write_varint(out: &mut Vec<u8>, value: i32) {
    let mut v = value as u32;
    loop {
        let mut byte = (v & 0x7F) as u8;
        v >>= 7;
        if v != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if v == 0 {
            break;
        }
    }
}

fn read_varint(r: &mut impl Read) -> std::io::Result<i32> {
    let mut value = 0i32;
    let mut shift = 0u32;
    loop {
        let mut b = [0u8; 1];
        r.read_exact(&mut b)?;
        value |= ((b[0] & 0x7F) as i32) << shift;
        if b[0] & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 35 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "VarInt 过长",
            ));
        }
    }
    Ok(value)
}

fn write_string(out: &mut Vec<u8>, s: &str) {
    write_varint(out, s.len() as i32);
    out.extend_from_slice(s.as_bytes());
}

fn read_string(r: &mut impl Read) -> std::io::Result<String> {
    let len = read_varint(r)?;
    if len < 0 || len as usize > MAX_RESPONSE {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "字符串长度异常",
        ));
    }
    let mut buf = vec![0u8; len as usize];
    r.read_exact(&mut buf)?;
    String::from_utf8(buf)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "不是合法 UTF-8"))
}

/// 给包体加上 VarInt 长度前缀。
fn frame(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 5);
    write_varint(&mut out, payload.len() as i32);
    out.extend_from_slice(payload);
    out
}

fn handshake_packet(host: &str, port: u16) -> Vec<u8> {
    let mut body = Vec::new();
    write_varint(&mut body, 0x00); // packet id: handshake
    write_varint(&mut body, 767); // 协议版本，服务器一般不校验，写个新版本即可
    write_string(&mut body, host);
    body.extend_from_slice(&port.to_be_bytes());
    write_varint(&mut body, 1); // next state: status
    frame(&body)
}

fn status_request_packet() -> Vec<u8> {
    let mut body = Vec::new();
    write_varint(&mut body, 0x00);
    frame(&body)
}

/// 把聊天组件（字符串 / 对象 / 数组）拍平成纯文本。
pub fn flatten_chat(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(items) => items.iter().map(flatten_chat).collect(),
        serde_json::Value::Object(map) => {
            let mut out = String::new();
            if let Some(serde_json::Value::String(t)) = map.get("text") {
                out.push_str(t);
            }
            if let Some(extra) = map.get("extra") {
                out.push_str(&flatten_chat(extra));
            }
            out
        }
        _ => String::new(),
    }
}

/// 去掉 `§a` 这类颜色/格式代码。
pub fn strip_codes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '§' {
            chars.next(); // 丢掉后面的格式字符
        } else {
            out.push(c);
        }
    }
    out
}

/// modern ping 解析出来的字段，避免函数签名里堆一长串元组。
pub struct ParsedStatus {
    pub version_name: Option<String>,
    pub version_protocol: Option<i32>,
    pub players_online: Option<u32>,
    pub players_max: Option<u32>,
    pub sample: Vec<String>,
    pub motd_raw: String,
    pub favicon: Option<String>,
}

/// 解析 modern ping 返回的那段 JSON。
pub fn parse_status(json: &serde_json::Value) -> ParsedStatus {
    let version_name = json
        .get("version")
        .and_then(|v| v.get("name"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let version_protocol = json
        .get("version")
        .and_then(|v| v.get("protocol"))
        .and_then(|v| v.as_i64())
        .map(|v| v as i32);
    let players = json.get("players");
    let players_online = players
        .and_then(|p| p.get("online"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);
    let players_max = players
        .and_then(|p| p.get("max"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);
    let sample = players
        .and_then(|p| p.get("sample"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|e| e.get("name").and_then(|n| n.as_str()))
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let motd_raw = json
        .get("description")
        .map(flatten_chat)
        .unwrap_or_default();
    let favicon = json
        .get("favicon")
        .and_then(|v| v.as_str())
        .filter(|s| s.starts_with("data:image/") && s.len() <= 256 * 1024)
        .map(|s| s.to_string());
    ParsedStatus {
        version_name,
        version_protocol,
        players_online,
        players_max,
        sample,
        motd_raw,
        favicon,
    }
}

/// 解析老协议（1.6 及更早）返回的 `§1\0协议\0版本\0MOTD\0在线\0上限`。
pub fn parse_legacy(bytes: &[u8]) -> Option<(i32, String, String, u32, u32)> {
    // 0xFF + u16 长度 + UTF-16BE
    if bytes.len() < 3 || bytes[0] != 0xFF {
        return None;
    }
    let len = u16::from_be_bytes([bytes[1], bytes[2]]) as usize;
    let body = bytes.get(3..3 + len * 2)?;
    let units: Vec<u16> = body
        .chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect();
    let text = String::from_utf16(&units).ok()?;
    let mut parts = text.split('\0');
    let head = parts.next()?;
    if !head.starts_with("§1") {
        return None;
    }
    let protocol = parts.next()?.trim().parse::<i32>().unwrap_or(0);
    let version = parts.next()?.to_string();
    let motd = parts.next()?.to_string();
    let online = parts.next()?.trim().parse::<u32>().unwrap_or(0);
    let max = parts.next()?.trim().parse::<u32>().unwrap_or(0);
    Some((protocol, version, motd, online, max))
}

fn offline(host: &str, port: u16, started: Instant, error: String) -> ServerStatus {
    ServerStatus {
        host: host.to_string(),
        port,
        online: false,
        latency_ms: started.elapsed().as_millis() as u64,
        version_name: None,
        version_protocol: None,
        players_online: None,
        players_max: None,
        sample: Vec::new(),
        motd: String::new(),
        motd_raw: String::new(),
        favicon: None,
        legacy: false,
        error: Some(error),
    }
}

/// 参数不合法时的兜底：批量刷新里某个地址写错了，不该让整批失败。
pub fn offline_from_error(host: &str, port: u16, error: String) -> ServerStatus {
    // 错误串是 `CODE|消息|细节` 格式，给玩家看的时候只要消息那部分
    let message = error
        .split('|')
        .nth(1)
        .map(|s| s.to_string())
        .unwrap_or(error);
    ServerStatus {
        host: host.to_string(),
        port,
        online: false,
        latency_ms: 0,
        version_name: None,
        version_protocol: None,
        players_online: None,
        players_max: None,
        sample: Vec::new(),
        motd: String::new(),
        motd_raw: String::new(),
        favicon: None,
        legacy: false,
        error: Some(message),
    }
}

fn connect(host: &str, port: u16, timeout: Duration) -> std::io::Result<TcpStream> {
    let addrs = (host, port).to_socket_addrs()?;
    let mut last = std::io::Error::new(std::io::ErrorKind::NotFound, "解析不到地址");
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, timeout) {
            Ok(s) => return Ok(s),
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn try_modern(stream: &mut TcpStream, host: &str, port: u16) -> std::io::Result<serde_json::Value> {
    stream.write_all(&handshake_packet(host, port))?;
    stream.write_all(&status_request_packet())?;
    stream.flush()?;

    let len = read_varint(stream)?;
    if len <= 0 || len as usize > MAX_RESPONSE {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "响应长度异常",
        ));
    }
    let mut buf = vec![0u8; len as usize];
    stream.read_exact(&mut buf)?;
    let mut cur = std::io::Cursor::new(buf);
    let packet_id = read_varint(&mut cur)?;
    if packet_id != 0x00 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "不是状态响应",
        ));
    }
    let text = read_string(&mut cur)?;
    serde_json::from_str(&text)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))
}

fn try_legacy(stream: &mut TcpStream) -> std::io::Result<(i32, String, String, u32, u32)> {
    stream.write_all(&[0xFE, 0x01])?;
    stream.flush()?;
    let mut head = [0u8; 3];
    stream.read_exact(&mut head)?;
    if head[0] != 0xFF {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "不是 legacy 响应",
        ));
    }
    let len = u16::from_be_bytes([head[1], head[2]]) as usize;
    let mut body = vec![0u8; len * 2];
    stream.read_exact(&mut body)?;
    let mut all = Vec::with_capacity(3 + body.len());
    all.extend_from_slice(&head);
    all.extend_from_slice(&body);
    parse_legacy(&all)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "legacy 响应解析失败"))
}

/// 查询一个服务器。`host` 可以带端口（`example.com:25566`）。
///
/// 联网失败不会返回 Err，而是 `online=false` + `error`。
pub fn ping(
    host: &str,
    port: Option<u16>,
    timeout_ms: Option<u64>,
) -> Result<ServerStatus, String> {
    let (raw_host, inline_port) = split_address(host);
    let host = raw_host.trim().trim_end_matches('.').to_string();
    if host.is_empty() {
        return Err("INVALID_VALUE|服务器地址不能为空".to_string());
    }
    if host.chars().count() > MAX_HOST_LEN
        || host
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '/' || c == '\\')
    {
        return Err("INVALID_VALUE|服务器地址不合法".to_string());
    }
    let port = port.or(inline_port).unwrap_or(DEFAULT_PORT);
    if port == 0 {
        return Err("INVALID_VALUE|端口必须在 1-65535 之间".to_string());
    }
    let timeout =
        Duration::from_millis(timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS).clamp(500, 30_000));

    let started = Instant::now();
    let mut stream = match connect(&host, port, timeout) {
        Ok(s) => s,
        Err(e) => return Ok(offline(&host, port, started, format!("连不上服务器：{e}"))),
    };
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));

    match try_modern(&mut stream, &host, port) {
        Ok(json) => {
            let latency = started.elapsed().as_millis() as u64;
            let p = parse_status(&json);
            Ok(ServerStatus {
                host,
                port,
                online: true,
                latency_ms: latency,
                version_name: p.version_name,
                version_protocol: p.version_protocol,
                players_online: p.players_online,
                players_max: p.players_max,
                sample: p.sample,
                motd: strip_codes(&p.motd_raw),
                motd_raw: p.motd_raw,
                favicon: p.favicon,
                legacy: false,
                error: None,
            })
        }
        Err(modern_err) => {
            // modern 失败：可能真是 1.6 老服，退回去试一次
            let started2 = Instant::now();
            match try_legacy(&mut stream) {
                Ok((protocol, version, motd, online, max)) => Ok(ServerStatus {
                    host,
                    port,
                    online: true,
                    latency_ms: started.elapsed().as_millis() as u64,
                    version_name: Some(version),
                    version_protocol: Some(protocol),
                    players_online: Some(online),
                    players_max: Some(max),
                    sample: Vec::new(),
                    motd: strip_codes(&motd),
                    motd_raw: motd,
                    favicon: None,
                    legacy: true,
                    error: None,
                }),
                Err(_) => Ok(offline(
                    &host,
                    port,
                    started2,
                    format!("服务器没有正常回应：{modern_err}"),
                )),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt_varint(value: i32) {
        let mut buf = Vec::new();
        write_varint(&mut buf, value);
        let mut cur = std::io::Cursor::new(buf);
        assert_eq!(read_varint(&mut cur).unwrap(), value);
    }

    #[test]
    fn varint_roundtrip() {
        for v in [0, 1, 127, 128, 255, 25565, 767, 2097151, 2147483647] {
            rt_varint(v);
        }
        // 127 只要 1 字节，128 要 2 字节
        let mut b = Vec::new();
        write_varint(&mut b, 127);
        assert_eq!(b, vec![0x7F]);
        b.clear();
        write_varint(&mut b, 128);
        assert_eq!(b, vec![0x80, 0x01]);
    }

    #[test]
    fn string_roundtrip() {
        let mut buf = Vec::new();
        write_string(&mut buf, "mc.example.com");
        let mut cur = std::io::Cursor::new(buf);
        assert_eq!(read_string(&mut cur).unwrap(), "mc.example.com");
        // 中文按 UTF-8 字节数算长度
        let mut buf2 = Vec::new();
        write_string(&mut buf2, "你好");
        assert_eq!(buf2[0], 6);
    }

    #[test]
    fn flattens_chat_components() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"text":"Hello ","extra":[{"text":"World","extra":[{"text":"!"}]}]}"#,
        )
        .unwrap();
        assert_eq!(flatten_chat(&v), "Hello World!");
        let s = serde_json::json!("plain");
        assert_eq!(flatten_chat(&s), "plain");
        let arr = serde_json::json!([{"text":"a"},{"text":"b"}]);
        assert_eq!(flatten_chat(&arr), "ab");
    }

    #[test]
    fn strips_formatting_codes() {
        assert_eq!(strip_codes("§aHello §lWorld§r"), "Hello World");
        assert_eq!(strip_codes("no codes"), "no codes");
        // 结尾单独的 § 不能 panic
        assert_eq!(strip_codes("tail§"), "tail");
    }

    #[test]
    fn parses_status_json() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{
                "version": {"name": "1.20.1", "protocol": 763},
                "players": {"max": 100, "online": 7, "sample": [{"name":"Alice"},{"name":"Bob"}]},
                "description": {"text": "§6Welcome"},
                "favicon": "data:image/png;base64,AAAA"
            }"#,
        )
        .unwrap();
        let p = parse_status(&json);
        assert_eq!(p.version_name.as_deref(), Some("1.20.1"));
        assert_eq!(p.version_protocol, Some(763));
        assert_eq!(p.players_online, Some(7));
        assert_eq!(p.players_max, Some(100));
        assert_eq!(p.sample, vec!["Alice".to_string(), "Bob".to_string()]);
        assert_eq!(p.motd_raw, "§6Welcome");
        assert_eq!(p.favicon.as_deref(), Some("data:image/png;base64,AAAA"));
    }

    #[test]
    fn rejects_bad_favicon_and_missing_fields() {
        let json: serde_json::Value =
            serde_json::from_str(r#"{"favicon":"http://evil/x.png"}"#).unwrap();
        let p = parse_status(&json);
        assert!(p.version_name.is_none() && p.version_protocol.is_none());
        assert!(p.players_online.is_none() && p.players_max.is_none());
        assert!(p.sample.is_empty() && p.motd_raw.is_empty());
        // 不是 data:image/ 的图标不能透传给前端
        assert!(p.favicon.is_none());
    }

    #[test]
    fn splits_host_and_port() {
        assert_eq!(
            split_address("play.example.com"),
            ("play.example.com".to_string(), None)
        );
        assert_eq!(
            split_address("play.example.com:25566"),
            ("play.example.com".to_string(), Some(25566))
        );
        assert_eq!(
            split_address("  spaced.example.com  "),
            ("spaced.example.com".to_string(), None)
        );
        assert_eq!(
            split_address("[::1]:25565"),
            ("::1".to_string(), Some(25565))
        );
        assert_eq!(split_address("[::1]"), ("::1".to_string(), None));
        // 裸 IPv6 不能当成 host:port 拆
        assert_eq!(split_address("::1"), ("::1".to_string(), None));
        // 冒号后面不是数字就不拆
        assert_eq!(
            split_address("weird:thing"),
            ("weird:thing".to_string(), None)
        );
    }

    #[test]
    fn parses_legacy_response() {
        let text = "§1\u{0}74\u{0}1.6.4\u{0}A Minecraft Server\u{0}3\u{0}20";
        let units: Vec<u16> = text.encode_utf16().collect();
        let mut bytes = vec![0xFF];
        bytes.extend_from_slice(&(units.len() as u16).to_be_bytes());
        for u in &units {
            bytes.extend_from_slice(&u.to_be_bytes());
        }
        let (protocol, version, motd, online, max) = parse_legacy(&bytes).unwrap();
        assert_eq!(protocol, 74);
        assert_eq!(version, "1.6.4");
        assert_eq!(motd, "A Minecraft Server");
        assert_eq!(online, 3);
        assert_eq!(max, 20);
        // 不是 0xFF 开头直接拒
        assert!(parse_legacy(&[0x00, 0x00, 0x00]).is_none());
    }

    #[test]
    fn rejects_invalid_address() {
        assert!(ping("", None, None).is_err());
        assert!(ping("   ", None, None).is_err());
        assert!(ping("bad host", None, None).is_err());
        assert!(ping("a/b", None, None).is_err());
        assert!(ping("example.com", Some(0), None).is_err());
    }
}
