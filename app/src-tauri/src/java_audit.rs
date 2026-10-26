//! Java 运行环境审计（C4）。
//!
//! 只读为主：把「每个实例需要的 Java」「记录的路径还能不能用」
//! 「启动器自己下载的 JDK 占了多少空间、还有没有人用」分开报，
//! 不合成一个 healthy 布尔。删除托管 JDK 是独立的破坏性操作，
//! 必须显式确认、只允许删 `<game>/jvm/adoptium-<major>`。

use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::instance::{Instance, PlayStats};
use crate::java::JavaRuntime;

/// 解析 `1.20.1` / `1.20` / `1.21.4-forge-...` 这类版本号的前三段
fn parse_mc(version: &str) -> Option<(u32, u32, u32)> {
    let head: String = version
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

/// 读不到版本 JSON 时，按 Minecraft 版本推算需要的 Java 主版本。
///
/// 这是**兜底**：优先用版本 JSON 里的 `javaVersion.majorVersion`，
/// 只有那里读不到才用这张表。
pub fn expected_major(mc_version: &str) -> Option<u32> {
    let v = parse_mc(mc_version)?;
    Some(if v >= (1, 20, 5) {
        21
    } else if v >= (1, 17, 0) {
        17
    } else {
        8
    })
}

/// 说明为什么推荐这个版本（给界面显示，不含猜测）
pub fn reason_for(mc_version: &str) -> String {
    match parse_mc(mc_version) {
        Some(v) if v >= (1, 20, 5) => "1.20.5 起官方要求 Java 21".to_string(),
        Some(v) if v >= (1, 18, 0) => "1.18 ~ 1.20.4 官方要求 Java 17".to_string(),
        Some(v) if v >= (1, 17, 0) => "1.17 最低 Java 16，推荐 17".to_string(),
        Some(_) => "1.16.5 及更早版本使用 Java 8".to_string(),
        None => "无法识别版本号，按 Java 17 处理".to_string(),
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct JavaCheck {
    pub instance_id: String,
    pub instance_name: String,
    pub game_version: String,
    /// 实例里记录的 Java 路径（没记录就是 null，表示自动选择）
    pub java_path: Option<String>,
    /// instance = 玩家指定；auto = 启动器自动挑
    pub source: String,
    pub exists: bool,
    pub version: Option<String>,
    pub major: Option<u32>,
    pub required: Option<u32>,
    /// 为什么需要这个主版本（给界面直接显示，不含猜测）
    pub requirement: String,
    pub compatible: bool,
    /// 具体问题（没有就是 null），不合成布尔
    pub problem: Option<String>,
    pub suggestion: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ManagedInfo {
    pub major: u32,
    pub dir: String,
    pub bytes: u64,
    pub java_path: Option<String>,
    pub exists: bool,
    /// 是否还有实例在用它
    pub referenced_by: Vec<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct JavaAudit {
    pub instances: Vec<JavaCheck>,
    pub managed: Vec<ManagedInfo>,
    /// 没有被任何实例引用的托管 JDK 主版本
    pub unused: Vec<u32>,
    /// 有问题的实例 id
    pub broken: Vec<String>,
    /// 系统里发现但不由启动器管理的 Java（只读展示，不允许删除）
    pub external: Vec<String>,
    pub checked_at: String,
}

fn key_of(p: &str) -> String {
    p.replace('/', "\\").to_ascii_lowercase()
}

/// 主版本是否满足要求：
/// - 要求 8 时必须正好 8（Java 17 跑不了 1.12）
/// - 要求 >= 17 时更高版本通常可用（17 的要求用 21 跑一般没问题）
pub fn satisfies(major: u32, required: u32) -> bool {
    if required <= 8 {
        major == required
    } else {
        major >= required
    }
}

fn check_one(
    game: &Path,
    inst: &Instance,
    runtimes: &[JavaRuntime],
) -> JavaCheck {
    let required = crate::java::required_major(game, &inst.game_version)
        .or_else(|| expected_major(&inst.game_version));

    let mut out = JavaCheck {
        instance_id: inst.id.clone(),
        instance_name: inst.name.clone(),
        game_version: inst.game_version.clone(),
        java_path: inst.java_path.clone(),
        source: if inst.java_path.is_some() {
            "instance".into()
        } else {
            "auto".into()
        },
        exists: false,
        version: None,
        major: None,
        required,
        requirement: reason_for(&inst.game_version),
        compatible: false,
        problem: None,
        suggestion: None,
    };

    match inst.java_path.as_deref() {
        Some(p) => {
            let path = Path::new(p);
            out.exists = path.is_file();
            if !out.exists {
                out.problem = Some("实例记录的 Java 路径已经不存在（可能被移动或删除）".into());
                out.suggestion = Some(
                    "可以改成自动选择，或在「运行库」里重新挑一个 Java".to_string(),
                );
                return out;
            }
            // 先看扫描结果里有没有同一个文件，避免重复执行 java -version
            let k = key_of(p);
            let known = runtimes.iter().find(|r| key_of(&r.path) == k).cloned();
            let rt = known.or_else(|| crate::java::probe_path(path));
            match rt {
                Some(rt) => {
                    out.version = Some(rt.version.clone());
                    out.major = Some(rt.major);
                    out.compatible = required.map(|r| satisfies(rt.major, r)).unwrap_or(true);
                    if let Some(r) = required {
                        if !out.compatible {
                            out.problem = Some(format!(
                                "这个 Java 是 {}，但 {} 需要 Java {}",
                                rt.major, inst.game_version, r
                            ));
                            out.suggestion = Some(format!("换成 Java {r} 或更高版本"));
                        }
                    }
                }
                None => {
                    out.problem = Some("这个文件存在，但运行 `java -version` 没有得到版本号".into());
                    out.suggestion = Some("它可能不是可用的 Java，建议重新选择".into());
                }
            }
        }
        None => {
            let picked = required.and_then(|r| crate::java::pick_for(runtimes, r));
            match picked {
                Some(rt) => {
                    out.exists = true;
                    out.version = Some(rt.version.clone());
                    out.major = Some(rt.major);
                    out.java_path = Some(rt.path.clone());
                    out.compatible = true;
                }
                None => {
                    out.problem = Some(match required {
                        Some(r) => format!("本机没有找到 Java {r}"),
                        None => "本机没有找到可用的 Java".to_string(),
                    });
                    out.suggestion = Some("在「运行库」里下载一个，或安装系统 Java".into());
                }
            }
        }
    }
    out
}

/// 全量审计：每个实例一行，加上托管 JDK 的占用与引用情况
pub fn audit(game: &Path, stats: &dyn Fn(&str) -> PlayStats) -> JavaAudit {
    let runtimes = crate::java::scan(game);
    let instances = crate::instance::discover(game, stats);
    let checks: Vec<JavaCheck> = instances
        .iter()
        .map(|i| check_one(game, i, &runtimes))
        .collect();

    let jvm_root = game.join("jvm");
    let mut managed: Vec<ManagedInfo> = crate::jdk::list_managed(game)
        .into_iter()
        .map(|m| {
            let dir_key = key_of(&m.dir);
            let mut refs = Vec::new();
            for i in &instances {
                if let Some(p) = &i.java_path {
                    if key_of(p).starts_with(&dir_key) {
                        refs.push(i.id.clone());
                    }
                }
            }
            ManagedInfo {
                major: m.major,
                dir: m.dir.clone(),
                bytes: m.bytes,
                exists: Path::new(&m.dir).is_dir(),
                java_path: m.java_path,
                referenced_by: refs,
            }
        })
        .collect();
    managed.sort_by_key(|m| m.major);

    let unused: Vec<u32> = managed
        .iter()
        .filter(|m| m.referenced_by.is_empty())
        .map(|m| m.major)
        .collect();

    let external: Vec<String> = runtimes
        .iter()
        .filter(|r| !key_of(&r.path).starts_with(&key_of(&jvm_root.to_string_lossy())))
        .map(|r| r.path.clone())
        .collect();

    JavaAudit {
        broken: checks
            .iter()
            .filter(|c| c.problem.is_some())
            .map(|c| c.instance_id.clone())
            .collect(),
        instances: checks,
        managed,
        unused,
        external,
        checked_at: crate::time::now_iso(),
    }
}

pub fn check_instance(
    game: &Path,
    instance_id: &str,
    stats: &dyn Fn(&str) -> PlayStats,
) -> Result<JavaCheck, String> {
    let runtimes = crate::java::scan(game);
    let inst = crate::instance::find(game, instance_id, stats)
        .ok_or_else(|| "INSTANCE_NOT_FOUND|实例不存在".to_string())?;
    Ok(check_one(game, &inst, &runtimes))
}

/// 删除启动器自己下载的 JDK。
///
/// 安全约束（任何一条不满足就拒绝）：
/// 1. 必须显式确认；
/// 2. 目录必须正好是 `<game>/jvm/adoptium-<major>`，不接受别的路径；
/// 3. 不能是符号链接/junction；
/// 4. 还有实例在用就不能删。
pub fn remove_managed(game: &Path, major: u32, confirm: bool) -> Result<serde_json::Value, String> {
    if !confirm {
        return Err("UNSUPPORTED|删除 Java 需要明确确认".into());
    }
    let jvm_root = game.join("jvm");
    let dir: PathBuf = jvm_root.join(format!("adoptium-{major}"));
    let root_canon = jvm_root
        .canonicalize()
        .map_err(|_| "NOT_FOUND|没有找到启动器管理的 Java 目录".to_string())?;
    let dir_canon = dir
        .canonicalize()
        .map_err(|_| format!("NOT_FOUND|没有安装 Java {major}"))?;
    if !dir_canon.starts_with(&root_canon) || dir_canon == root_canon {
        return Err("INVALID_PATH|只能删除启动器自己下载的 Java".into());
    }
    if crate::net::is_reparse_point(&dir_canon) {
        return Err("INVALID_PATH|目标是一个符号链接，已拒绝删除".into());
    }

    // 引用检查：任何实例记录的路径落在这个目录里都不许删
    let stats = |_: &str| PlayStats::default();
    let instances = crate::instance::discover(game, &stats);
    let dir_key = key_of(&dir_canon.to_string_lossy());
    let mut used_by = Vec::new();
    for i in &instances {
        if let Some(p) = &i.java_path {
            let k = key_of(p);
            if k == dir_key || k.starts_with(&format!("{dir_key}\\")) {
                used_by.push(i.name.clone());
            }
        }
    }
    if !used_by.is_empty() {
        return Err(format!(
            "INSTANCE_BUSY|还有实例在使用这个 Java|{}",
            used_by.join("、")
        ));
    }

    let mut bytes = 0u64;
    fn walk(p: &Path, acc: &mut u64, depth: u32) {
        if depth > 10 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(p) else { return };
        for e in rd.flatten() {
            let path = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => walk(&path, acc, depth + 1),
                Ok(t) if t.is_file() => *acc += e.metadata().map(|m| m.len()).unwrap_or(0),
                _ => {}
            }
        }
    }
    walk(&dir_canon, &mut bytes, 0);

    std::fs::remove_dir_all(&dir_canon)
        .map_err(|e| format!("PERMISSION|删除失败|{e}"))?;

    Ok(serde_json::json!({
        "removed": true,
        "major": major,
        "bytes": bytes,
        "dir": dir_canon.to_string_lossy(),
    }))
}

/// 下载缓存（`.astral/jdk/*.zip`）的占用情况
pub fn cache_stats(game: &Path) -> serde_json::Value {
    let root = game.join(".astral").join("jdk");
    let mut files = Vec::new();
    let mut bytes = 0u64;
    if let Ok(rd) = std::fs::read_dir(&root) {
        for e in rd.flatten() {
            let Ok(meta) = e.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            if !name.to_ascii_lowercase().ends_with(".zip") {
                continue;
            }
            bytes += meta.len();
            files.push(serde_json::json!({
                "name": name,
                "bytes": meta.len(),
                "modified": meta
                    .modified()
                    .ok()
                    .map(|t| {
                        let d = t
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default();
                        crate::time::unix_ms_to_iso(d.as_millis() as i64)
                    })
                    .unwrap_or_default(),
            }));
        }
    }
    files.sort_by(|a, b| {
        let ab = a.get("bytes").and_then(|x| x.as_u64()).unwrap_or(0);
        let bb = b.get("bytes").and_then(|x| x.as_u64()).unwrap_or(0);
        bb.cmp(&ab)
    });
    serde_json::json!({
        "dir": root.to_string_lossy(),
        "files": files,
        "count": files.len(),
        "bytes": bytes,
    })
}

/// 清空 JDK 下载缓存（只删 zip，不动已安装的 JDK）
pub fn clear_cache(game: &Path, confirm: bool) -> Result<serde_json::Value, String> {
    if !confirm {
        return Err("UNSUPPORTED|清空下载缓存需要明确确认".into());
    }
    let root = game.join(".astral").join("jdk");
    let mut removed = 0u32;
    let mut bytes = 0u64;
    if let Ok(rd) = std::fs::read_dir(&root) {
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_file() {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_ascii_lowercase();
            if !name.ends_with(".zip") {
                continue;
            }
            let size = e.metadata().map(|m| m.len()).unwrap_or(0);
            if std::fs::remove_file(&p).is_ok() {
                removed += 1;
                bytes += size;
            }
        }
    }
    Ok(serde_json::json!({ "removed": removed, "bytes": bytes }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expected_major_follows_official_requirements() {
        assert_eq!(expected_major("1.12.2"), Some(8));
        assert_eq!(expected_major("1.16.5"), Some(8));
        assert_eq!(expected_major("1.17.1"), Some(17));
        assert_eq!(expected_major("1.20.4"), Some(17));
        assert_eq!(expected_major("1.20.5"), Some(21));
        assert_eq!(expected_major("1.21.4"), Some(21));
        assert_eq!(expected_major("1.20.1-forge-47.2.0"), Some(17));
        assert_eq!(expected_major("garbage"), None);
    }

    #[test]
    fn satisfies_respects_java_8_ceiling() {
        assert!(satisfies(8, 8));
        assert!(!satisfies(17, 8));
        assert!(satisfies(17, 17));
        assert!(satisfies(21, 17));
        assert!(!satisfies(17, 21));
    }

    #[test]
    fn parse_mc_handles_short_versions() {
        assert_eq!(parse_mc("1.20"), Some((1, 20, 0)));
        assert_eq!(parse_mc("1.7.10"), Some((1, 7, 10)));
    }
}
