//! 磁盘占用分析：钱花在哪个文件夹上了。
//!
//! 玩家的硬盘不是无限的。一个整合包实例动辄几 GB，`assets` 又单独几 GB，
//! 缓存、备份、回收站里还各躺着一堆看不见的东西。玩家只会看到「C 盘红了」，
//! 不知道该删什么。
//!
//! 这里**只读**：把游戏目录按几大类量一遍，再按实例细分，告诉界面
//! 「这几个 GB 花在哪」。真正的清理动作仍然走各自已有的命令
//! （`cache_clear` / `purge_trash` / `prune_mod_update_backups` …），
//! 每个都有自己的确认要求，不在这个模块里偷偷删东西。

use serde::Serialize;
use std::path::{Path, PathBuf};

/// 一次最多进多少层目录。存档、资源包里不会有更深的必要结构，
/// 再深基本就是解压炸弹或者自己套娃，不再往里数。
const MAX_DEPTH: u32 = 8;
/// 一次最多看多少个文件。到上限就标 `truncated`，让界面知道数字是下限。
const MAX_FILES: u64 = 400_000;

/// 一个文件夹的占用
#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct DirUsage {
    /// 相对游戏目录的路径，`""` 表示根
    pub path: String,
    /// 界面显示用的名字
    pub label: String,
    /// 这一类是什么（见 [`CATEGORIES`]）
    pub kind: String,
    pub bytes: u64,
    pub files: u64,
    /// 文件夹在不在
    pub exists: bool,
    /// 数到上限了，`bytes` 是下限
    pub truncated: bool,
    /// 删这里会不会影响玩家（给界面决定要不要放「清理」按钮）
    pub safe_to_clean: bool,
    /// 一句话说明这是干什么的
    pub note: String,
}

/// 一个实例的占用
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InstanceUsage {
    pub id: String,
    pub name: String,
    pub game_version: String,
    pub bytes: u64,
    pub files: u64,
    /// 实例里各部分占多少（mods / saves / resourcepacks …）
    pub parts: Vec<DirUsage>,
    /// 里面最大的几个文件，帮玩家找「谁这么大」
    pub largest: Vec<FileUsage>,
}

/// 单个文件
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FileUsage {
    /// 相对实例目录
    pub path: String,
    pub bytes: u64,
}

/// 整体报告
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DiskReport {
    pub game_dir: String,
    /// 游戏目录总共多少（各分类之和，不含未知零散文件）
    pub total_bytes: u64,
    /// 分类占用，按大小倒序
    pub categories: Vec<DirUsage>,
    /// 每个实例的占用，按大小倒序
    pub instances: Vec<InstanceUsage>,
    /// 所在磁盘的剩余空间
    pub free_bytes: Option<u64>,
    /// 所在磁盘的总容量
    pub total_disk_bytes: Option<u64>,
    /// 值得提醒玩家的话（不是错误，是建议）
    pub hints: Vec<String>,
    /// 数到上限了，报告不完整
    pub truncated: bool,
}

/// 分类定义：`(目录, 界面名字, kind, 能不能安全清, 说明)`
///
/// `safe_to_clean` 指的是「用对应命令清理不会丢玩家数据」：
/// 缓存和下载残留可以清；`assets`/`libraries` 清了游戏要重下但不会丢档；
/// `saves`/`mods` 绝对不能在这里自动清。
const CATEGORIES: [(&str, &str, &str, bool, &str); 8] = [
    (
        "assets",
        "游戏资源",
        "assets",
        false,
        "游戏自己的音效、语言、贴图，删了游戏会重新下",
    ),
    (
        "libraries",
        "游戏库文件",
        "libraries",
        false,
        "游戏运行需要的库，删了会重新下",
    ),
    (
        "versions",
        "游戏版本",
        "versions",
        false,
        "各版本的 jar 与 json，删了对应版本要重装",
    ),
    (
        "instances",
        "实例",
        "instances",
        false,
        "每个实例的 mods、存档、配置都在这里",
    ),
    (
        "cache",
        "下载缓存",
        "cache",
        true,
        "已下载文件的副本，清理不会丢东西，只会让下次重下",
    ),
    (
        "jvm",
        "托管的 Java",
        "java",
        true,
        "启动器自己装的 Java 运行时",
    ),
    (
        "logs",
        "日志",
        "logs",
        true,
        "启动器日志，清理后历史问题就查不到了",
    ),
    (
        "meta",
        "元数据缓存",
        "meta",
        true,
        "版本清单、Modrinth 信息的缓存，清理后断网就看不到了",
    ),
];

/// 实例内部各部分
const INSTANCE_PARTS: [(&str, &str, &str, bool, &str); 8] = [
    ("mods", "Mod", "mods", false, "装了的 Mod"),
    ("saves", "存档", "saves", false, "世界存档，删了就真没了"),
    ("config", "配置", "config", false, "Mod 的配置文件"),
    ("resourcepacks", "资源包", "resourcepacks", false, "资源包"),
    ("shaderpacks", "光影包", "shaderpacks", false, "光影包"),
    (
        "screenshots",
        "截图",
        "screenshots",
        true,
        "游戏截图，删了只是丢图",
    ),
    ("logs", "日志", "logs", true, "这个实例的游戏日志"),
    (
        ".astral-backup",
        "Mod 更新备份",
        "backups",
        true,
        "Mod 批量更新前的旧文件备份",
    ),
];

/// 量一个目录：`(字节数, 文件数, 是否到上限)`
///
/// 只统计普通文件；符号链接与重解析点不跟进去（否则可能绕圈或跳到别处）。
pub fn measure(dir: &Path) -> (u64, u64, bool) {
    let mut bytes = 0u64;
    let mut files = 0u64;
    let mut truncated = false;
    let mut stack: Vec<(PathBuf, u32)> = vec![(dir.to_path_buf(), 0)];
    while let Some((d, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            if files >= MAX_FILES {
                truncated = true;
                break;
            }
            let p = e.path();
            let Ok(md) = std::fs::symlink_metadata(&p) else {
                continue;
            };
            // 符号链接 / Windows 重解析点一律不跟
            if md.file_type().is_symlink() || crate::net::is_reparse_point(&p) {
                continue;
            }
            if md.is_dir() {
                if depth < MAX_DEPTH {
                    stack.push((p, depth + 1));
                }
            } else if md.is_file() {
                bytes = bytes.saturating_add(md.len());
                files += 1;
            }
        }
        if truncated {
            break;
        }
    }
    (bytes, files, truncated)
}

/// 量一个目录，同时找出里面最大的 `top` 个文件
pub fn measure_with_largest(dir: &Path, top: usize) -> (u64, u64, Vec<FileUsage>) {
    let mut bytes = 0u64;
    let mut files = 0u64;
    let mut biggest: Vec<FileUsage> = Vec::new();
    let mut stack: Vec<(PathBuf, u32)> = vec![(dir.to_path_buf(), 0)];
    while let Some((d, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            if files >= MAX_FILES {
                break;
            }
            let p = e.path();
            let Ok(md) = std::fs::symlink_metadata(&p) else {
                continue;
            };
            if md.file_type().is_symlink() || crate::net::is_reparse_point(&p) {
                continue;
            }
            if md.is_dir() {
                if depth < MAX_DEPTH {
                    stack.push((p, depth + 1));
                }
            } else if md.is_file() {
                bytes = bytes.saturating_add(md.len());
                files += 1;
                if top > 0 {
                    let rel = p
                        .strip_prefix(dir)
                        .map(|r| r.to_string_lossy().replace('\\', "/"))
                        .unwrap_or_else(|_| p.to_string_lossy().to_string());
                    biggest.push(FileUsage {
                        path: rel,
                        bytes: md.len(),
                    });
                    if biggest.len() > top * 4 {
                        biggest.sort_by_key(|f| std::cmp::Reverse(f.bytes));
                        biggest.truncate(top);
                    }
                }
            }
        }
    }
    biggest.sort_by_key(|f| std::cmp::Reverse(f.bytes));
    biggest.truncate(top);
    (bytes, files, biggest)
}

fn usage_of(root: &Path, rel: &str, label: &str, kind: &str, clean: bool, note: &str) -> DirUsage {
    let p = if rel.is_empty() {
        root.to_path_buf()
    } else {
        root.join(rel)
    };
    let exists = p.is_dir();
    let (bytes, files, truncated) = if exists { measure(&p) } else { (0, 0, false) };
    DirUsage {
        path: rel.to_string(),
        label: label.to_string(),
        kind: kind.to_string(),
        bytes,
        files,
        exists,
        truncated,
        safe_to_clean: clean,
        note: note.to_string(),
    }
}

/// 磁盘剩余空间：`(可用, 总容量)`
pub fn disk_space(path: &Path) -> (Option<u64>, Option<u64>) {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let canon = |p: &Path| -> PathBuf {
        let c = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        let s = c.to_string_lossy().to_string();
        match s.strip_prefix(r"\\?\") {
            Some(rest) => PathBuf::from(rest),
            None => PathBuf::from(s),
        }
    };
    let target = canon(path);
    let mut best: Option<(usize, u64, u64)> = None;
    for d in disks.list() {
        let mp = canon(d.mount_point());
        if target.starts_with(&mp) {
            let depth = mp.components().count();
            if best.map(|(bd, _, _)| depth >= bd).unwrap_or(true) {
                best = Some((depth, d.available_space(), d.total_space()));
            }
        }
    }
    match best {
        Some((_, free, total)) => (Some(free), Some(total)),
        None => (None, None),
    }
}

fn mb(bytes: u64) -> String {
    let m = bytes as f64 / (1024.0 * 1024.0);
    if m >= 1024.0 {
        format!("{:.1} GB", m / 1024.0)
    } else {
        format!("{:.0} MB", m)
    }
}

/// 扫一遍游戏目录
///
/// `instances` 是调用方给的实例清单（id、显示名、版本、目录），
/// 这里不自己去发现实例 —— 发现逻辑在 `instance::discover`，
/// 让它保持唯一来源，避免两边对「哪些算实例」的判断不一致。
pub fn report(game: &Path, instances: &[(String, String, String, PathBuf)]) -> DiskReport {
    let mut categories: Vec<DirUsage> = CATEGORIES
        .iter()
        .map(|(rel, label, kind, clean, note)| usage_of(game, rel, label, kind, *clean, note))
        .collect();
    categories.sort_by_key(|c| std::cmp::Reverse(c.bytes));

    let mut per_instance: Vec<InstanceUsage> = Vec::new();
    for (id, name, version, dir) in instances {
        if !dir.is_dir() {
            continue;
        }
        let (bytes, files, largest) = measure_with_largest(dir, 5);
        let parts: Vec<DirUsage> = INSTANCE_PARTS
            .iter()
            .map(|(rel, label, kind, clean, note)| usage_of(dir, rel, label, kind, *clean, note))
            .filter(|u| u.exists)
            .collect();
        per_instance.push(InstanceUsage {
            id: id.clone(),
            name: name.clone(),
            game_version: version.clone(),
            bytes,
            files,
            parts,
            largest,
        });
    }
    per_instance.sort_by_key(|i| std::cmp::Reverse(i.bytes));

    let total_bytes = categories.iter().map(|c| c.bytes).sum::<u64>();
    let (free_bytes, total_disk_bytes) = disk_space(game);

    // 值得提醒的几句：只在真的值得说的时候说，不是每次都念一遍
    let mut hints = Vec::new();
    if let Some(free) = free_bytes {
        if free < 2 * 1024 * 1024 * 1024 {
            hints.push(format!("磁盘只剩 {}，装新版本或整合包可能失败", mb(free)));
        }
    }
    let cache = categories.iter().find(|c| c.kind == "cache");
    if let Some(c) = cache {
        if c.bytes > 1024 * 1024 * 1024 {
            hints.push(format!(
                "下载缓存占 {}，清理不会丢东西（下次重下会慢一点）",
                mb(c.bytes)
            ));
        }
    }
    if let Some(biggest) = per_instance.first() {
        if biggest.bytes > 4 * 1024 * 1024 * 1024 {
            hints.push(format!(
                "「{}」占了 {}，可以看看里面最大的文件是不是旧的整合包",
                biggest.name,
                mb(biggest.bytes)
            ));
        }
    }
    let backups: u64 = per_instance
        .iter()
        .flat_map(|i| i.parts.iter())
        .filter(|p| p.kind == "backups")
        .map(|p| p.bytes)
        .sum();
    if backups > 512 * 1024 * 1024 {
        hints.push(format!(
            "Mod 更新备份占 {}，确认没问题后可以清掉",
            mb(backups)
        ));
    }

    let truncated = categories.iter().any(|c| c.truncated);
    DiskReport {
        game_dir: game.to_string_lossy().to_string(),
        total_bytes,
        categories,
        instances: per_instance,
        free_bytes,
        total_disk_bytes,
        hints,
        truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("astral-disk-{}-{}", tag, crate::time::now_nanos()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write(p: &Path, bytes: usize) {
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(p, vec![0u8; bytes]).unwrap();
    }

    #[test]
    fn measures_nested_files() {
        let d = tmp("nested");
        write(&d.join("a.txt"), 100);
        write(&d.join("sub/b.txt"), 250);
        write(&d.join("sub/deep/c.txt"), 50);
        let (bytes, files, truncated) = measure(&d);
        assert_eq!(bytes, 400);
        assert_eq!(files, 3);
        assert!(!truncated);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn missing_dir_is_zero() {
        let d = tmp("missing");
        let (bytes, files, truncated) = measure(&d.join("nope"));
        assert_eq!(bytes, 0);
        assert_eq!(files, 0);
        assert!(!truncated);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn finds_the_largest_files() {
        let d = tmp("largest");
        write(&d.join("small.bin"), 10);
        write(&d.join("mods/big.jar"), 900);
        write(&d.join("mods/mid.jar"), 400);
        let (bytes, files, largest) = measure_with_largest(&d, 2);
        assert_eq!(bytes, 1310);
        assert_eq!(files, 3);
        assert_eq!(largest.len(), 2);
        assert_eq!(largest[0].path, "mods/big.jar");
        assert_eq!(largest[0].bytes, 900);
        assert_eq!(largest[1].path, "mods/mid.jar");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn report_ranks_categories_and_instances() {
        let game = tmp("report");
        write(&game.join("assets/objects/a"), 300);
        write(&game.join("libraries/b.jar"), 100);
        let inst1 = game.join("instances/one");
        write(&inst1.join("mods/x.jar"), 500);
        write(&inst1.join("saves/w/level.dat"), 20);
        let inst2 = game.join("instances/two");
        write(&inst2.join("mods/y.jar"), 50);

        let list = vec![
            (
                "one".to_string(),
                "第一个".to_string(),
                "1.20.1".to_string(),
                inst1.clone(),
            ),
            (
                "two".to_string(),
                "第二个".to_string(),
                "1.21.1".to_string(),
                inst2.clone(),
            ),
        ];
        let r = report(&game, &list);
        assert_eq!(r.instances.len(), 2);
        assert_eq!(r.instances[0].id, "one", "大的排前面");
        assert_eq!(r.instances[0].bytes, 520);
        assert_eq!(r.instances[1].bytes, 50);
        // 分类里 instances 应该最大（520 + 50 = 570）
        let inst_cat = r.categories.iter().find(|c| c.kind == "instances").unwrap();
        assert_eq!(inst_cat.bytes, 570);
        assert_eq!(r.categories[0].kind, "instances");
        // 各部分只列存在的
        let kinds: Vec<&str> = r.instances[0]
            .parts
            .iter()
            .map(|p| p.kind.as_str())
            .collect();
        assert!(kinds.contains(&"mods"));
        assert!(kinds.contains(&"saves"));
        assert!(!kinds.contains(&"shaderpacks"), "没建过的目录不列出来");
        let _ = std::fs::remove_dir_all(&game);
    }

    #[test]
    fn hints_mention_big_cache() {
        let game = tmp("hint");
        write(
            &game.join("cache/objects/aa/big"),
            2 * 1024 * 1024 * 1024 + 1,
        );
        let r = report(&game, &[]);
        assert!(
            r.hints.iter().any(|h| h.contains("下载缓存")),
            "缓存超过 1 GB 应该提醒"
        );
        let _ = std::fs::remove_dir_all(&game);
    }

    #[test]
    fn safe_to_clean_flags_are_honest() {
        let game = tmp("safe");
        let r = report(&game, &[]);
        let by = |kind: &str| r.categories.iter().find(|c| c.kind == kind).unwrap();
        assert!(by("cache").safe_to_clean);
        assert!(by("meta").safe_to_clean);
        assert!(!by("instances").safe_to_clean, "实例目录不能标成可清");
        assert!(!by("assets").safe_to_clean);
        let _ = std::fs::remove_dir_all(&game);
    }
}
