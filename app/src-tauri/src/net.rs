//! 下载与路径安全（契约 V2 §13）。
//!
//! 三条规则：
//! 1. 只允许白名单主机，且必须是 https；
//! 2. 只写允许目录之内（canonicalize 后做前缀检查，防 `..` 与 junction 逃逸）；
//! 3. 先落 `.part`，校验后再原子替换，失败清理临时文件。

use std::io::Read;
use std::path::{Path, PathBuf};

/// 允许的下载来源。新增来源前先确认它是否会被重定向到别处。
pub const ALLOWED_HOSTS: &[&str] = &[
    // Modrinth
    "cdn.modrinth.com",
    "api.modrinth.com",
    // Mojang
    "piston-data.mojang.com",
    "piston-metadata.mojang.com",
    "launchermeta.mojang.com",
    "launcher.mojang.com",
    "resources.download.minecraft.net",
    "libraries.minecraft.net",
    // 加载器
    "meta.fabricmc.net",
    "maven.fabricmc.net",
    "maven.minecraftforge.net",
    "files.minecraftforge.net",
    "maven.neoforged.net",
    "meta.quiltmc.org",
    "maven.quiltmc.org",
    // 国内镜像
    "bmclapi2.bangbang93.com",
];

fn blocked(reason: &str) -> String {
    format!("BLOCKED_URL|下载地址被拒绝|{reason}")
}

/// 校验下载地址；返回小写主机名。
///
/// 只接受 `https://`，拒绝 userinfo、IP 字面量、localhost、非白名单主机。
pub fn check_url(raw: &str) -> Result<String, String> {
    let s = raw.trim();
    let rest = s
        .strip_prefix("https://")
        .ok_or_else(|| blocked("只允许 https 地址"))?;

    let host_port = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host_port.contains('@') {
        return Err(blocked("地址不得包含用户信息"));
    }

    let host = host_port
        .split(':')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if host.is_empty() {
        return Err(blocked("缺少主机名"));
    }
    if host.parse::<std::net::IpAddr>().is_ok() || host == "localhost" || host.ends_with(".local") {
        return Err(blocked("不允许本机或内网地址"));
    }
    if !ALLOWED_HOSTS.iter().any(|h| host == *h) {
        return Err(blocked(&format!("不在允许的来源列表内：{host}")));
    }
    Ok(host)
}

/// 目标必须落在 `root` 之内；返回 canonical 后的完整路径。
///
/// 不存在的父目录会被创建；目标文件本身可以不存在。
pub fn ensure_within(root: &Path, target: &Path) -> Result<PathBuf, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("INVALID_PATH|根目录不可用|{e}"))?;

    let name = target
        .file_name()
        .ok_or_else(|| "INVALID_PATH|目标文件名无效".to_string())?;
    let parent = target
        .parent()
        .ok_or_else(|| "INVALID_PATH|目标目录无效".to_string())?;

    std::fs::create_dir_all(parent).map_err(|e| format!("PERMISSION|创建目录失败|{e}"))?;
    let parent = parent
        .canonicalize()
        .map_err(|e| format!("INVALID_PATH|目录不可解析|{e}"))?;

    if !parent.starts_with(&root) {
        return Err("INVALID_PATH|目标超出允许目录".into());
    }
    Ok(parent.join(name))
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// 文件的 SHA1（小写十六进制）
pub fn sha1_file(path: &Path) -> Result<String, String> {
    use sha1::{Digest, Sha1};
    let mut f = std::fs::File::open(path).map_err(|e| format!("INVALID_PATH|文件不可读|{e}"))?;
    let mut hasher = Sha1::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| format!("INVALID_PATH|读取失败|{e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex(&hasher.finalize()))
}

/// 原子下载：先写 `<dest>.part`，校验后替换。
///
/// 失败或校验不通过时删除 `.part`，不留下半个文件。
pub fn download_atomic(
    url: &str,
    dest: &Path,
    expected_sha1: Option<&str>,
    mut on_progress: impl FnMut(u32, u32, u32, u32),
) -> Result<u64, String> {
    check_url(url)?;

    let part = PathBuf::from(format!("{}.part", dest.to_string_lossy()));
    let _ = std::fs::remove_file(&part);

    let mut single = portablemc::download::single(url, part.clone());
    let res = single.download(BridgeHandler(&mut on_progress));
    if let Err(e) = res {
        let _ = std::fs::remove_file(&part);
        return Err(format!("NETWORK|下载失败|{e:?}"));
    }

    if let Some(want) = expected_sha1 {
        let got = sha1_file(&part)?;
        if !got.eq_ignore_ascii_case(want) {
            let _ = std::fs::remove_file(&part);
            return Err(format!("HASH_MISMATCH|校验失败|期望 {want}，实际 {got}"));
        }
    }

    let size = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    std::fs::rename(&part, dest).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        format!("PERMISSION|替换文件失败|{e}")
    })?;
    Ok(size)
}

/// 文件的 SHA512（小写十六进制）
pub fn sha512_file(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha512};
    let mut f = std::fs::File::open(path).map_err(|e| format!("INVALID_PATH|文件不可读|{e}"))?;
    let mut hasher = Sha512::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| format!("INVALID_PATH|读取失败|{e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex(&hasher.finalize()))
}

/// 拉取 JSON（临时文件，不缓存），用于 Modrinth API
pub fn fetch_json(url: &str) -> Result<serde_json::Value, String> {
    check_url(url)?;
    let tmp = std::env::temp_dir().join(format!(
        "astral-api-{:x}.json",
        crate::time::now_unix_ms()
    ));
    let _ = std::fs::remove_file(&tmp);
    let mut single = portablemc::download::single(url, tmp.clone());
    let res = single.download(());
    if let Err(e) = res {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("NETWORK|请求失败|{e:?}"));
    }
    let text = std::fs::read_to_string(&tmp).map_err(|e| format!("INTERNAL|读取响应失败|{e}"))?;
    let _ = std::fs::remove_file(&tmp);
    serde_json::from_str(&text).map_err(|e| format!("INTERNAL|响应不是合法 JSON|{e}"))
}

struct BridgeHandler<'a>(&'a mut dyn FnMut(u32, u32, u32, u32));

impl portablemc::download::Handler for BridgeHandler<'_> {
    fn on_progress(&mut self, count: u32, total_count: u32, size: u32, total_size: u32) {
        (self.0)(count, total_count, size, total_size);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_known_hosts() {
        assert_eq!(
            check_url("https://cdn.modrinth.com/data/x/y.jar").unwrap(),
            "cdn.modrinth.com"
        );
        assert!(check_url("https://bmclapi2.bangbang93.com/maven/a.jar").is_ok());
    }

    #[test]
    fn rejects_unsafe_urls() {
        assert!(check_url("http://cdn.modrinth.com/a.jar").is_err());
        assert!(check_url("https://evil.example.com/a.jar").is_err());
        assert!(check_url("https://127.0.0.1/a.jar").is_err());
        assert!(check_url("https://localhost/a.jar").is_err());
        assert!(check_url("https://user@cdn.modrinth.com/a.jar").is_err());
        assert!(check_url("file:///C:/a.jar").is_err());
        assert!(check_url("https://sub.cdn.modrinth.com/a.jar").is_err());
    }
}
