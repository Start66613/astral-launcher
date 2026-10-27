//! 存档图标与存档文件夹。
//!
//! 游戏会给每个存档在目录里放一张 `icon.png`（64×64，玩家可以自己换）。
//! `list_target_worlds` 里只给了 `hasIcon` 一个布尔，前端要显示还得自己想办法
//! 拿图 —— 而游戏目录是玩家可配的，静态 asset scope 要么写死要么放得太宽。
//!
//! 所以这里跟 `screenshots` / `mod_icon` 一样走 IPC：读成 data URL，
//! `img-src data:` 已经允许，不动安全配置。
//!
//! 另外提供「在文件管理器里打开存档文件夹」，玩家想自己换图标、看 region
//! 文件、拷存档都方便 —— 这是 PCL/HMCL 都有、我们一直缺的。

use serde::Serialize;
use std::path::Path;

/// 图标读取上限。游戏自己写的是 64×64，玩家换的图最多也就几十 KB。
const MAX_ICON: u64 = 2 * 1024 * 1024;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WorldIcon {
    /// 可以直接塞进 `<img src>` 的 data URL
    pub data_url: String,
    pub size_bytes: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// 读某个存档的图标。没有图标返回 `Ok(None)`（不是错误 —— 大部分存档都有，
/// 但新建的、被删过的没有）。
pub fn read_icon(world_dir: &Path) -> Result<Option<WorldIcon>, String> {
    let path = world_dir.join("icon.png");
    let meta = match std::fs::metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("PERMISSION|读不到存档图标|{e}")),
    };
    if !meta.is_file() {
        return Ok(None);
    }
    if meta.len() > MAX_ICON {
        return Err(format!(
            "INVALID_VALUE|存档图标异常大（{} KB），已拒绝读取",
            meta.len() / 1024
        ));
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("PERMISSION|读不到存档图标|{e}"))?;
    // 游戏写的都是 PNG；玩家换的图如果扩展名是 png 但内容不是，读尺寸会失败，
    // 但图还是能显示（浏览器按魔数认），所以不拦。
    let size = crate::screenshots::png_size(&bytes);
    Ok(Some(WorldIcon {
        data_url: format!(
            "data:image/png;base64,{}",
            crate::screenshots::base64_encode(&bytes)
        ),
        size_bytes: meta.len(),
        width: size.map(|s| s.0),
        height: size.map(|s| s.1),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d =
            std::env::temp_dir().join(format!("astral-wicon-{tag}-{}", crate::time::now_nanos()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 一个最小的合法 PNG 头（只有 IHDR，够读尺寸）
    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut b = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        b.extend_from_slice(&13u32.to_be_bytes());
        b.extend_from_slice(b"IHDR");
        b.extend_from_slice(&w.to_be_bytes());
        b.extend_from_slice(&h.to_be_bytes());
        b.extend_from_slice(&[8, 2, 0, 0, 0]);
        b
    }

    #[test]
    fn reads_an_icon_as_data_url() {
        let d = tmp("ok");
        let bytes = png(64, 64);
        std::fs::write(d.join("icon.png"), &bytes).unwrap();
        let icon = read_icon(&d).unwrap().unwrap();
        assert!(icon.data_url.starts_with("data:image/png;base64,"));
        assert_eq!(icon.width, Some(64));
        assert_eq!(icon.height, Some(64));
        assert_eq!(icon.size_bytes, bytes.len() as u64);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn missing_icon_is_not_an_error() {
        let d = tmp("none");
        assert!(read_icon(&d).unwrap().is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn non_png_still_reads_but_has_no_size() {
        let d = tmp("junk");
        std::fs::write(d.join("icon.png"), b"definitely not a png").unwrap();
        let icon = read_icon(&d).unwrap().unwrap();
        assert!(icon.width.is_none());
        assert!(icon.data_url.starts_with("data:image/png;base64,"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn refuses_an_absurdly_large_icon() {
        let d = tmp("big");
        std::fs::write(d.join("icon.png"), vec![0u8; (MAX_ICON + 1) as usize]).unwrap();
        let err = read_icon(&d).unwrap_err();
        assert!(err.starts_with("INVALID_VALUE|"), "{err}");
        let _ = std::fs::remove_dir_all(&d);
    }
}
