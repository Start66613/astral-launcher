//! 游戏截图浏览（只读 + 删除）。
//!
//! 截图在 `screenshots/` 下，Minecraft 自己存 PNG。这里做三件事：
//! 列出（带尺寸）、读成 data URL 给前端显示、删除（需确认）。
//!
//! 为什么返回 base64 而不是用 tauri 的 asset 协议：asset 协议要在
//! `tauri.conf.json` 里静态声明允许的目录，而游戏目录是玩家可配的，
//! 静态 scope 要么写死要么放得太宽。走 IPC 一次读一张，配合
//! `img-src data:`（CSP 已允许）就够用，也不用动安全配置。
//!
//! 尺寸是从 PNG 的 IHDR 头里直接读的，不解码像素 —— 列 200 张截图
//! 也不会因为解码而卡住。

use serde::Serialize;
use std::path::{Path, PathBuf};

/// 单张图片读取上限。MC 截图一般是 1–4 MB，8 MB 足够覆盖 4K。
const MAX_IMAGE: u64 = 8 * 1024 * 1024;
const MAX_NAME: usize = 200;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Screenshot {
    /// 文件名（不含目录），前端回传时用它定位
    pub file_name: String,
    /// 完整路径，给"在文件夹中显示"用
    pub path: String,
    pub size_bytes: u64,
    pub modified_at: String,
    /// PNG 里读到的像素宽（读不出就是 null）
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// 补充说明，例如"不是 PNG，读不出尺寸"
    pub note: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ScreenshotData {
    pub file_name: String,
    /// `image/png` 或 `image/jpeg`
    pub mime: String,
    pub size_bytes: u64,
    /// 可以直接塞进 `<img src>` 的 data URL
    pub data_url: String,
}

fn is_image(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".png") || lower.ends_with(".jpg") || lower.ends_with(".jpeg")
}

/// 只允许纯文件名，挡掉 `..`、盘符、路径分隔符
fn safe_name(name: &str) -> Result<&str, String> {
    if name.is_empty() {
        return Err("INVALID_NAME|文件名不能为空".into());
    }
    if name.chars().count() > MAX_NAME {
        return Err("INVALID_NAME|文件名太长".into());
    }
    if name.contains('/') || name.contains('\\') || name.contains(':') {
        return Err("INVALID_NAME|文件名里不能有路径分隔符".into());
    }
    if name == "." || name == ".." || name.starts_with('.') {
        return Err("INVALID_NAME|文件名不合法".into());
    }
    Ok(name)
}

/// 从 PNG 的 IHDR 里读宽高。不是 PNG 或头被截断就返回 None。
pub fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    const SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if bytes.len() < 24 || bytes[..8] != SIG {
        return None;
    }
    // 8..12 是第一个 chunk 的长度，12..16 必须是 IHDR
    if &bytes[12..16] != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    if w == 0 || h == 0 || w > 100_000 || h > 100_000 {
        return None;
    }
    Some((w, h))
}

/// 标准 base64（带 `=` 补齐），自己写是为了不新增依赖
pub fn base64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

fn mime_of(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        "image/jpeg"
    } else {
        "image/png"
    }
}

fn read_head(path: &Path, n: usize) -> Vec<u8> {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut buf = vec![0u8; n];
    let mut got = 0usize;
    while got < n {
        match f.read(&mut buf[got..]) {
            Ok(0) => break,
            Ok(k) => got += k,
            Err(_) => break,
        }
    }
    buf.truncate(got);
    buf
}

/// 列出截图目录里的图片（按修改时间倒序）
pub fn list(dir: &Path) -> Vec<Screenshot> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in rd.flatten() {
        let p = e.path();
        let Ok(meta) = std::fs::symlink_metadata(&p) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        if !is_image(&name) {
            continue;
        }
        let head = read_head(&p, 24);
        let (width, height, note) = match png_size(&head) {
            Some((w, h)) => (Some(w), Some(h), None),
            None => {
                if name.to_ascii_lowercase().ends_with(".png") {
                    (
                        None,
                        None,
                        Some("PNG 头读不出来（文件可能已损坏）".to_string()),
                    )
                } else {
                    (None, None, Some("不是 PNG，读不出尺寸".to_string()))
                }
            }
        };
        out.push(Screenshot {
            file_name: name,
            path: p.to_string_lossy().to_string(),
            size_bytes: meta.len(),
            modified_at: meta
                .modified()
                .map(crate::time::system_time_to_iso)
                .unwrap_or_else(|_| crate::time::now_iso()),
            width,
            height,
            note,
        });
    }
    out.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));
    out
}

/// 读一张截图，返回可以直接显示的 data URL
pub fn read(dir: &Path, file_name: &str) -> Result<ScreenshotData, String> {
    let name = safe_name(file_name)?;
    if !is_image(name) {
        return Err("INVALID_VALUE|只能读 PNG 或 JPEG 图片".into());
    }
    let p: PathBuf = dir.join(name);
    let meta = std::fs::metadata(&p).map_err(|e| format!("NOT_FOUND|找不到这张截图|{e}"))?;
    if !meta.is_file() {
        return Err("INVALID_VALUE|不是文件".into());
    }
    if crate::net::is_reparse_point(&p) {
        return Err("INVALID_PATH|这是一个符号链接，已拒绝读取".into());
    }
    if meta.len() > MAX_IMAGE {
        return Err(format!(
            "INVALID_VALUE|图片太大（{} MB），请用系统看图工具打开",
            meta.len() / 1024 / 1024
        ));
    }
    let bytes = std::fs::read(&p).map_err(|e| format!("PERMISSION|读取截图失败|{e}"))?;
    let mime = mime_of(&bytes);
    let data_url = format!("data:{mime};base64,{}", base64_encode(&bytes));
    Ok(ScreenshotData {
        file_name: name.to_string(),
        mime: mime.to_string(),
        size_bytes: meta.len(),
        data_url,
    })
}

/// 删除一张截图（需确认）
pub fn delete(dir: &Path, file_name: &str, confirm: bool) -> Result<u64, String> {
    if !confirm {
        return Err("UNSUPPORTED|删除截图需要明确确认".into());
    }
    let name = safe_name(file_name)?;
    if !is_image(name) {
        return Err("INVALID_VALUE|只能删 PNG 或 JPEG 图片".into());
    }
    let p = dir.join(name);
    let meta = std::fs::metadata(&p).map_err(|e| format!("NOT_FOUND|找不到这张截图|{e}"))?;
    if !meta.is_file() {
        return Err("INVALID_VALUE|不是文件".into());
    }
    if crate::net::is_reparse_point(&p) {
        return Err("INVALID_PATH|这是一个符号链接，已拒绝删除".into());
    }
    let bytes = meta.len();
    std::fs::remove_file(&p).map_err(|e| format!("PERMISSION|删除截图失败|{e}"))?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "astral-shots-{tag}-{:032x}",
            crate::time::now_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 造一个只有头部的合法 PNG（够读尺寸，不是能显示的完整图）
    fn fake_png(w: u32, h: u32) -> Vec<u8> {
        let mut v = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        v.extend_from_slice(&13u32.to_be_bytes());
        v.extend_from_slice(b"IHDR");
        v.extend_from_slice(&w.to_be_bytes());
        v.extend_from_slice(&h.to_be_bytes());
        v.extend_from_slice(&[8, 2, 0, 0, 0]);
        v.extend_from_slice(&[0, 0, 0, 0]);
        v
    }

    #[test]
    fn encodes_base64_with_padding() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn reads_png_dimensions() {
        assert_eq!(png_size(&fake_png(1920, 1080)), Some((1920, 1080)));
        assert_eq!(png_size(&fake_png(256, 150)), Some((256, 150)));
        assert_eq!(png_size(b"not a png at all"), None);
        assert_eq!(png_size(&fake_png(0, 1080)), None);
    }

    #[test]
    fn lists_only_images_newest_first() {
        let d = tmp("list");
        std::fs::write(d.join("a.png"), fake_png(1920, 1080)).unwrap();
        std::fs::write(d.join("b.jpg"), [0xff, 0xd8, 0xff, 0x00]).unwrap();
        std::fs::write(d.join("notes.txt"), b"x").unwrap();
        let list = list(&d);
        assert_eq!(list.len(), 2);
        let png = list.iter().find(|s| s.file_name == "a.png").unwrap();
        assert_eq!(png.width, Some(1920));
        assert_eq!(png.height, Some(1080));
        assert!(png.note.is_none());
        let jpg = list.iter().find(|s| s.file_name == "b.jpg").unwrap();
        assert!(jpg.width.is_none() && jpg.note.is_some());
    }

    #[test]
    fn reads_a_screenshot_as_data_url() {
        let d = tmp("read");
        let png = fake_png(800, 600);
        std::fs::write(d.join("shot.png"), &png).unwrap();
        let data = read(&d, "shot.png").unwrap();
        assert_eq!(data.mime, "image/png");
        assert!(data.data_url.starts_with("data:image/png;base64,"));
        let b64 = data.data_url.split(',').nth(1).unwrap();
        assert_eq!(base64_encode(&png), b64);
        assert_eq!(data.size_bytes, png.len() as u64);
    }

    #[test]
    fn rejects_path_traversal_and_other_types() {
        let d = tmp("bad");
        assert!(read(&d, "../secret.png").is_err());
        assert!(read(&d, "a/b.png").is_err());
        assert!(read(&d, "..").is_err());
        std::fs::write(d.join("notes.txt"), b"x").unwrap();
        assert!(read(&d, "notes.txt").is_err());
        assert!(read(&d, "missing.png").is_err());
    }

    #[test]
    fn delete_needs_confirm() {
        let d = tmp("del");
        std::fs::write(d.join("x.png"), fake_png(10, 10)).unwrap();
        assert!(delete(&d, "x.png", false).is_err());
        assert!(d.join("x.png").is_file());
        let bytes = delete(&d, "x.png", true).unwrap();
        assert!(bytes > 0);
        assert!(!d.join("x.png").exists());
    }
}
