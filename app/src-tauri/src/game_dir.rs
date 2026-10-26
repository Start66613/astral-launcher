//! 游戏目录（`.minecraft`）识别与切换预检。
//!
//! 之前 `set_config` 对 `game_dir` **一个字符都不校验**：玩家手抖打错一个字母，
//! 启动器就会指到一个空目录，界面上所有实例、存档、Mod 全"消失"——看起来像
//! 数据丢了，实际只是路径错了。从 PCL / HMCL / 官方启动器搬过来的玩家也常常
//! 不知道该填哪个目录。
//!
//! 这里做两件事：
//! 1. `validate` —— 写配置前的硬校验（绝对路径、不是文件、能创建能写）。
//! 2. `inspect` —— 切换前的预检，告诉玩家这个目录里到底有什么、切过去会看到什么。

use serde::Serialize;
use std::path::{Path, PathBuf};

/// 判断"像个 Minecraft 目录"的依据：这些名字里有任意一个就基本能确认。
const MARKERS: [&str; 6] = [
    "versions",
    "libraries",
    "assets",
    "instances",
    "saves",
    "mods",
];

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GameDirReport {
    pub path: String,
    /// 目录已经存在
    pub exists: bool,
    /// 是目录而不是文件
    pub is_dir: bool,
    /// 能往里写（不存在时看能不能创建）
    pub writable: bool,
    /// 看得出来是 Minecraft 目录
    pub looks_like_minecraft: bool,
    /// 能扫到的版本 id（最多列 50 个）
    pub versions: Vec<String>,
    pub version_count: usize,
    pub instance_count: usize,
    pub save_count: usize,
    pub mod_count: usize,
    /// 有没有共享的 libraries / assets（缺了要重新下载几百 MB）
    pub has_libraries: bool,
    pub has_assets: bool,
    /// 这个目录当前占多大；扫不动时是 null
    pub size_bytes: Option<u64>,
    /// 所在磁盘剩余空间
    pub free_bytes: Option<u64>,
    /// 是不是当前正在用的目录
    pub same_as_current: bool,
    /// 当前正在用的目录
    pub current: String,
    /// 给玩家看的提示（不阻塞）
    pub notes: Vec<String>,
    /// 阻塞性问题：有值就不该让玩家切过去
    pub blocking: Vec<String>,
}

/// 写配置前的硬校验。只拦真正会出事的情况，不啰嗦。
pub fn validate(raw: &str) -> Result<PathBuf, String> {
    let text = raw.trim();
    if text.is_empty() {
        return Err("INVALID_VALUE|游戏目录不能为空".into());
    }
    let p = PathBuf::from(text);
    if !p.is_absolute() {
        return Err("INVALID_VALUE|游戏目录要写完整路径，比如 D:\\Games\\.minecraft".into());
    }
    if p.exists() && !p.is_dir() {
        return Err("INVALID_VALUE|这个路径是一个文件，不是目录".into());
    }
    // 盘符根目录单独当地址用会到处乱建文件，明确拒绝
    if p.parent().is_none() {
        return Err("INVALID_VALUE|不能把整个磁盘根目录当游戏目录".into());
    }
    Ok(p)
}

/// 能不能在目录里写文件（不存在就先建）。
fn writable(dir: &Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(format!(".astral-write-{}", crate::time::now_nanos()));
    match std::fs::write(&probe, b"ok") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

fn count_entries(dir: &Path, ext: Option<&str>) -> usize {
    let Ok(read) = std::fs::read_dir(dir) else {
        return 0;
    };
    read.flatten()
        .filter(|e| {
            let p = e.path();
            match ext {
                None => p.is_dir(),
                Some(x) => p.is_file() && p.extension().map(|v| v == x).unwrap_or(false),
            }
        })
        .count()
}

/// 扫一个候选目录，给玩家一份"切过去会看到什么"。
pub fn inspect(raw: &str, current: &Path) -> Result<GameDirReport, String> {
    let p = validate(raw)?;
    let exists = p.exists();
    let is_dir = if exists { p.is_dir() } else { true };
    let writable = is_dir && writable(&p);

    let mut blocking = Vec::new();
    if !is_dir {
        blocking.push("这个路径是一个文件，不是目录".to_string());
    } else if !writable {
        blocking.push("这个目录写不进去（可能是权限问题或磁盘只读）".to_string());
    }

    let mut notes = Vec::new();
    if !exists {
        notes.push("这个目录还不存在，切换后会新建一个空目录".to_string());
    }

    let versions: Vec<String> = if is_dir {
        let mut v: Vec<String> = std::fs::read_dir(p.join("versions"))
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.path().is_dir())
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    } else {
        Vec::new()
    };
    let version_count = versions.len();
    let versions: Vec<String> = versions.into_iter().take(50).collect();

    let has_libraries = p.join("libraries").is_dir();
    let has_assets = p.join("assets").is_dir();
    let instance_count = count_entries(&p.join("instances"), None);
    let save_count = count_entries(&p.join("saves"), None);
    let mod_count = count_entries(&p.join("mods"), Some("jar"));

    let looks_like_minecraft = MARKERS.iter().any(|m| p.join(m).exists());
    if is_dir && exists && !looks_like_minecraft {
        notes.push(
            "这个目录里没看到 versions / libraries 这些文件夹，可能不是 Minecraft 目录".to_string(),
        );
    }
    if looks_like_minecraft && !has_assets {
        notes.push("缺 assets 文件夹，启动时可能要重新下载资源文件".to_string());
    }

    let same_as_current = match (p.canonicalize(), current.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => p == current,
    };

    // 目录大小只对已经存在、且看起来像 Minecraft 的目录扫（避免扫整个 D 盘）
    let size_bytes = if exists && is_dir && looks_like_minecraft {
        Some(crate::world_backup::dir_stats(&p).0)
    } else {
        None
    };
    let free_bytes = if exists {
        crate::world_backup::available_space(&p)
    } else {
        // 目录还不存在时，问它的父目录所在磁盘
        p.parent().and_then(crate::world_backup::available_space)
    };

    if let Some(free) = free_bytes {
        if free < 2 * 1024 * 1024 * 1024 {
            notes.push(format!(
                "这个盘只剩 {} MB，装不下多少东西",
                free / 1024 / 1024
            ));
        }
    }

    Ok(GameDirReport {
        path: p.to_string_lossy().to_string(),
        exists,
        is_dir,
        writable,
        looks_like_minecraft,
        versions,
        version_count,
        instance_count,
        save_count,
        mod_count,
        has_libraries,
        has_assets,
        size_bytes,
        free_bytes,
        same_as_current,
        current: current.to_string_lossy().to_string(),
        notes,
        blocking,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("astral-gamedir-{tag}-{}", crate::time::now_nanos()));
        let _ = std::fs::create_dir_all(&d);
        d
    }

    #[test]
    fn rejects_empty_and_relative_paths() {
        assert!(validate("").is_err());
        assert!(validate("   ").is_err());
        assert!(validate(".minecraft").is_err());
        assert!(validate("D:Games").is_err());
    }

    #[test]
    fn rejects_a_file() {
        let d = tmp("file");
        let f = d.join("not-a-dir.txt");
        std::fs::write(&f, b"x").unwrap();
        let e = validate(&f.to_string_lossy()).unwrap_err();
        assert!(e.starts_with("INVALID_VALUE|"), "实际：{e}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn rejects_a_drive_root() {
        let e = validate("D:\\").unwrap_err();
        assert!(e.contains("根目录"), "实际：{e}");
    }

    #[test]
    fn accepts_a_normal_directory() {
        let d = tmp("ok");
        let p = validate(&d.to_string_lossy()).expect("应当通过");
        assert!(p.is_absolute());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn recognizes_a_minecraft_dir() {
        let d = tmp("mc");
        std::fs::create_dir_all(d.join("versions").join("1.20.1")).unwrap();
        std::fs::create_dir_all(d.join("libraries")).unwrap();
        std::fs::create_dir_all(d.join("saves").join("World")).unwrap();
        let r = inspect(&d.to_string_lossy(), Path::new("Z:\\nowhere")).expect("inspect");
        assert!(r.looks_like_minecraft);
        assert_eq!(r.version_count, 1);
        assert_eq!(r.versions, vec!["1.20.1".to_string()]);
        assert_eq!(r.save_count, 1);
        assert!(r.has_libraries);
        assert!(!r.has_assets);
        assert!(r.writable);
        assert!(r.blocking.is_empty());
        // 缺 assets 要有提示，但不是阻塞
        assert!(r.notes.iter().any(|n| n.contains("assets")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn reports_a_missing_dir_as_creatable() {
        let d = tmp("missing");
        let target = d.join("brand-new");
        let r = inspect(&target.to_string_lossy(), &d).expect("inspect");
        assert!(!r.exists);
        assert!(r.writable, "不存在时应当能创建");
        assert!(!r.looks_like_minecraft);
        assert!(r.blocking.is_empty());
        assert!(r.notes.iter().any(|n| n.contains("新建")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn detects_the_current_directory() {
        let d = tmp("same");
        let r = inspect(&d.to_string_lossy(), &d).expect("inspect");
        assert!(r.same_as_current);
        let other = d.join("other");
        std::fs::create_dir_all(&other).unwrap();
        let r2 = inspect(&other.to_string_lossy(), &d).expect("inspect");
        assert!(!r2.same_as_current);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_plain_dir_is_not_minecraft() {
        let d = tmp("plain");
        let r = inspect(&d.to_string_lossy(), Path::new("Z:\\nowhere")).expect("inspect");
        assert!(!r.looks_like_minecraft);
        assert!(r
            .notes
            .iter()
            .any(|n| n.contains("可能不是 Minecraft 目录")));
        let _ = std::fs::remove_dir_all(&d);
    }
}
