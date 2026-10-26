//! 实例封面图。
//!
//! 玩家想给自己的整合包实例配一张图。以前 `instance.json` 里的 `cover`
//! 是**任意字符串**，前端传什么就存什么 —— 存绝对路径能泄露本机目录结构，
//! 存 `..` 能指到别处。而且前端没有 asset 协议，压根显示不出来。
//!
//! 现在改成：图片复制进实例目录，只存一个固定的文件名，
//! 读的时候走 IPC 返回 data URL。规则和截图那套一致。

use serde::Serialize;
use std::path::{Path, PathBuf};

/// 单张上限。封面是给人看的缩略图，8 MB 已经很宽裕。
const MAX_COVER: u64 = 8 * 1024 * 1024;
/// 允许的扩展名（也是落盘时用的名字）
const ALLOWED: [(&str, &str); 4] = [
    ("image/png", ".png"),
    ("image/jpeg", ".jpg"),
    ("image/webp", ".webp"),
    ("image/gif", ".gif"),
];

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CoverInfo {
    /// 存进 instance.json 的文件名，例如 `.astral-cover.png`
    pub file_name: String,
    pub mime: String,
    pub size_bytes: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// 可直接塞进 `<img src>`
    pub data_url: String,
}

/// 判断是不是我们管理的封面文件名。只认 `.astral-cover.<ext>`，
/// 别的一律不认（包括绝对路径、`..`、别的文件名）。
pub fn is_managed_name(name: &str) -> bool {
    let Some(ext) = name.strip_prefix(".astral-cover.") else {
        return false;
    };
    ALLOWED
        .iter()
        .any(|(_, e)| e.trim_start_matches('.') == ext)
}

/// 把 instance.json 里可能存在的旧值收敛成安全值：
/// 不是我们管理的文件名就当没有（宁可没封面，也不要显示一个来路不明的路径）
pub fn sanitize(cover: Option<String>) -> Option<String> {
    cover.filter(|c| is_managed_name(c))
}

fn detect(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        return Some(("image/png", ".png"));
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some(("image/jpeg", ".jpg"));
    }
    if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some(("image/webp", ".webp"));
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some(("image/gif", ".gif"));
    }
    None
}

/// PNG 的宽高（IHDR）
fn png_size(b: &[u8]) -> Option<(u32, u32)> {
    if b.len() < 24 || &b[12..16] != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes([b[16], b[17], b[18], b[19]]);
    let h = u32::from_be_bytes([b[20], b[21], b[22], b[23]]);
    if w == 0 || h == 0 || w > 100_000 || h > 100_000 {
        return None;
    }
    Some((w, h))
}

/// JPEG 的宽高：扫到 SOFn 段读出来
fn jpeg_size(b: &[u8]) -> Option<(u32, u32)> {
    let mut i = 2usize;
    while i + 9 < b.len() {
        if b[i] != 0xff {
            i += 1;
            continue;
        }
        let marker = b[i + 1];
        // SOF0..SOF15 里除去 DHT(c4) DAC(cc) RSTn(d0-d7)
        if (0xc0..=0xcf).contains(&marker) && marker != 0xc4 && marker != 0xc8 && marker != 0xcc {
            let h = u16::from_be_bytes([b[i + 5], b[i + 6]]) as u32;
            let w = u16::from_be_bytes([b[i + 7], b[i + 8]]) as u32;
            if w == 0 || h == 0 {
                return None;
            }
            return Some((w, h));
        }
        let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
        if len < 2 {
            return None;
        }
        i += 2 + len;
    }
    None
}

fn size_of(mime: &str, bytes: &[u8]) -> Option<(u32, u32)> {
    match mime {
        "image/png" => png_size(bytes),
        "image/jpeg" => jpeg_size(bytes),
        _ => None,
    }
}

fn path_of(instance_dir: &Path, name: &str) -> PathBuf {
    instance_dir.join(name)
}

/// 设置封面：把 `source` 复制进实例目录，返回落盘的文件名
pub fn set(instance_dir: &Path, source: &Path) -> Result<String, String> {
    let meta = std::fs::metadata(source).map_err(|e| format!("NOT_FOUND|找不到这张图片|{e}"))?;
    if !meta.is_file() {
        return Err("INVALID_VALUE|请选择一张图片文件".into());
    }
    if crate::net::is_reparse_point(source) {
        return Err("INVALID_PATH|这是一个符号链接，已拒绝读取".into());
    }
    if meta.len() > MAX_COVER {
        return Err(format!(
            "INVALID_VALUE|图片太大（{} MB），请用小于 {} MB 的图",
            meta.len() / 1024 / 1024,
            MAX_COVER / 1024 / 1024
        ));
    }
    let bytes = std::fs::read(source).map_err(|e| format!("PERMISSION|读取图片失败|{e}"))?;
    let (mime, ext) = detect(&bytes)
        .ok_or_else(|| "INVALID_VALUE|只支持 PNG / JPEG / WebP / GIF 图片".to_string())?;
    let _ = mime;

    let name = format!(".astral-cover{ext}");
    // 换格式时先把旧封面删掉，避免留两个
    for (_, e) in ALLOWED {
        if e != ext {
            let _ = std::fs::remove_file(path_of(instance_dir, &format!(".astral-cover{e}")));
        }
    }
    let dest = path_of(instance_dir, &name);
    let tmp = dest.with_extension(format!("{}.part", ext.trim_start_matches('.')));
    std::fs::write(&tmp, &bytes).map_err(|e| format!("PERMISSION|写入封面失败|{e}"))?;
    if let Err(e) = std::fs::rename(&tmp, &dest) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("PERMISSION|保存封面失败|{e}"));
    }
    Ok(name)
}

/// 删掉封面文件（不管 instance.json 里写的是哪个）
pub fn clear(instance_dir: &Path) -> bool {
    let mut any = false;
    for (_, e) in ALLOWED {
        if std::fs::remove_file(path_of(instance_dir, &format!(".astral-cover{e}"))).is_ok() {
            any = true;
        }
    }
    any
}

/// 读出封面并编码成 data URL；没有封面就返回 None
pub fn read(instance_dir: &Path, cover: Option<&str>) -> Result<Option<CoverInfo>, String> {
    let Some(name) = cover.filter(|c| is_managed_name(c)) else {
        return Ok(None);
    };
    let path = path_of(instance_dir, name);
    let meta = match std::fs::metadata(&path) {
        Ok(m) if m.is_file() => m,
        _ => return Ok(None),
    };
    if meta.len() > MAX_COVER {
        return Err("INVALID_VALUE|封面文件异常大，已跳过".into());
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("PERMISSION|读取封面失败|{e}"))?;
    let (mime, _) = detect(&bytes).ok_or_else(|| "INVALID_VALUE|封面文件已损坏".to_string())?;
    let (width, height) = match size_of(mime, &bytes) {
        Some((w, h)) => (Some(w), Some(h)),
        None => (None, None),
    };
    Ok(Some(CoverInfo {
        file_name: name.to_string(),
        mime: mime.to_string(),
        size_bytes: meta.len(),
        width,
        height,
        data_url: format!(
            "data:{mime};base64,{}",
            crate::screenshots::base64_encode(&bytes)
        ),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "astral-cover-{tag}-{:032x}",
            crate::time::now_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut v = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        v.extend_from_slice(&13u32.to_be_bytes());
        v.extend_from_slice(b"IHDR");
        v.extend_from_slice(&w.to_be_bytes());
        v.extend_from_slice(&h.to_be_bytes());
        v.extend_from_slice(&[8, 2, 0, 0, 0, 0, 0, 0, 0]);
        v
    }

    fn jpeg(w: u16, h: u16) -> Vec<u8> {
        let mut v = vec![0xff, 0xd8];
        // APP0 段，随便填
        v.extend_from_slice(&[0xff, 0xe0, 0x00, 0x04, 0x00, 0x00]);
        // SOF0
        v.extend_from_slice(&[0xff, 0xc0, 0x00, 0x11, 0x08]);
        v.extend_from_slice(&h.to_be_bytes());
        v.extend_from_slice(&w.to_be_bytes());
        v.extend_from_slice(&[3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1]);
        v
    }

    #[test]
    fn only_managed_names_pass() {
        assert!(is_managed_name(".astral-cover.png"));
        assert!(is_managed_name(".astral-cover.jpg"));
        assert!(is_managed_name(".astral-cover.webp"));
        assert!(!is_managed_name("cover.png"));
        assert!(!is_managed_name("/etc/passwd"));
        assert!(!is_managed_name("..\\x.png"));
        assert!(!is_managed_name(".astral-cover.exe"));
        assert_eq!(sanitize(Some("C:\\secret.png".into())), None);
        assert_eq!(
            sanitize(Some(".astral-cover.png".into())).as_deref(),
            Some(".astral-cover.png")
        );
    }

    #[test]
    fn detects_image_types() {
        assert_eq!(detect(&png(4, 4)).map(|x| x.0), Some("image/png"));
        assert_eq!(detect(&jpeg(4, 4)).map(|x| x.0), Some("image/jpeg"));
        assert_eq!(detect(b"GIF89a....").map(|x| x.0), Some("image/gif"));
        assert_eq!(detect(b"not an image"), None);
    }

    #[test]
    fn reads_dimensions() {
        assert_eq!(png_size(&png(1920, 1080)), Some((1920, 1080)));
        assert_eq!(jpeg_size(&jpeg(800, 600)), Some((800, 600)));
        assert_eq!(jpeg_size(b"not a jpeg"), None);
    }

    #[test]
    fn sets_and_reads_a_cover() {
        let d = tmp("set");
        let src = d.join("src.png");
        std::fs::write(&src, png(512, 288)).unwrap();
        let name = set(&d, &src).unwrap();
        assert_eq!(name, ".astral-cover.png");
        let info = read(&d, Some(&name)).unwrap().unwrap();
        assert_eq!(info.width, Some(512));
        assert_eq!(info.height, Some(288));
        assert!(info.data_url.starts_with("data:image/png;base64,"));
    }

    #[test]
    fn replacing_removes_the_old_extension() {
        let d = tmp("replace");
        let p = d.join("a.png");
        let j = d.join("b.jpg");
        std::fs::write(&p, png(10, 10)).unwrap();
        std::fs::write(&j, jpeg(10, 10)).unwrap();
        assert_eq!(set(&d, &p).unwrap(), ".astral-cover.png");
        assert_eq!(set(&d, &j).unwrap(), ".astral-cover.jpg");
        assert!(!d.join(".astral-cover.png").exists());
        assert!(d.join(".astral-cover.jpg").exists());
    }

    #[test]
    fn rejects_non_images_and_missing() {
        let d = tmp("bad");
        let t = d.join("a.txt");
        std::fs::write(&t, b"hello").unwrap();
        assert!(set(&d, &t).is_err());
        assert!(set(&d, &d.join("nope.png")).is_err());
    }

    #[test]
    fn clears_and_reads_missing() {
        let d = tmp("clear");
        let p = d.join("a.png");
        std::fs::write(&p, png(10, 10)).unwrap();
        let name = set(&d, &p).unwrap();
        assert!(clear(&d));
        assert!(read(&d, Some(&name)).unwrap().is_none());
        assert!(!clear(&d));
        assert!(read(&d, None).unwrap().is_none());
    }
}
