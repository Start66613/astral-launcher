//! Java 运行时探测（契约 V2 §7）。
//!
//! 只做「找得到、跑得起来、版本对得上」三件事，不下载、不改系统。
//! 自动安装 Mojang JRE 仍由 portablemc 负责（启动时按需）。

use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct JavaRuntime {
    pub path: String,
    /// 原始版本串，例如 `17.0.9` / `1.8.0_402`
    pub version: String,
    /// 主版本号：8 / 17 / 21 …
    pub major: u32,
    pub vendor: String,
    pub bits: Option<u32>,
    /// 来源：java_home / path / game_jvm / common_dir
    pub source: String,
}

/// `1.8.0_402` → 8；`17.0.9` → 17；`21` → 21
pub fn parse_major(version: &str) -> Option<u32> {
    let v = version.trim().trim_matches('"');
    if let Some(rest) = v.strip_prefix("1.") {
        return rest.split('.').next()?.parse::<u32>().ok();
    }
    v.split(['.', '-', '+']).next()?.parse::<u32>().ok()
}

fn run_version(exe: &Path) -> Option<(String, String, Option<u32>)> {
    let out = std::process::Command::new(exe)
        .arg("-version")
        .output()
        .ok()?;
    // java -version 写到 stderr
    let text = String::from_utf8_lossy(&out.stderr).to_string()
        + &String::from_utf8_lossy(&out.stdout);
    if text.is_empty() {
        return None;
    }
    let first = text.lines().next().unwrap_or("");
    // 例：openjdk version "17.0.9" 2023-10-17
    let version = first
        .split('"')
        .nth(1)
        .map(|s| s.to_string())
        .or_else(|| {
            first
                .split_whitespace()
                .find(|w| w.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false))
                .map(|s| s.to_string())
        })?;
    let vendor = if first.to_lowercase().contains("openjdk") {
        "OpenJDK"
    } else if first.to_lowercase().contains("java") {
        "Oracle/Other"
    } else {
        "Unknown"
    }
    .to_string();
    let bits = if text.contains("64-Bit") {
        Some(64)
    } else if text.contains("32-Bit") {
        Some(32)
    } else {
        None
    };
    Some((version, vendor, bits))
}

fn probe(path: &Path, source: &str) -> Option<JavaRuntime> {
    if !path.is_file() {
        return None;
    }
    let (version, vendor, bits) = run_version(path)?;
    let major = parse_major(&version)?;
    Some(JavaRuntime {
        path: path.to_string_lossy().to_string(),
        version,
        major,
        vendor,
        bits,
        source: source.to_string(),
    })
}


/// 扫描本机可用的 Java
pub fn scan(game_dir: &Path) -> Vec<JavaRuntime> {
    let mut cands: Vec<(PathBuf, &str)> = Vec::new();

    if let Some(home) = std::env::var_os("JAVA_HOME") {
        cands.push((PathBuf::from(home).join("bin").join("java.exe"), "java_home"));
    }

    // 启动器管理的 JVM：可能是 jvm/<name>/bin/java.exe，
    // 也可能是我们自己解压的 jvm/adoptium-<major>/<jdk 目录>/bin/java.exe
    if let Ok(rd) = std::fs::read_dir(game_dir.join("jvm")) {
        for e in rd.flatten() {
            let root = e.path();
            let direct = root.join("bin").join("java.exe");
            if direct.is_file() {
                cands.push((direct, "game_jvm"));
                continue;
            }
            if let Ok(inner) = std::fs::read_dir(&root) {
                for sub in inner.flatten() {
                    let p = sub.path().join("bin").join("java.exe");
                    if p.is_file() {
                        cands.push((p, "game_jvm"));
                        break;
                    }
                }
            }
        }
    }

    // 常见安装目录
    let mut roots: Vec<PathBuf> = Vec::new();
    for env in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
        if let Some(base) = std::env::var_os(env) {
            let b = PathBuf::from(base);
            roots.push(b.join("Java"));
            roots.push(b.join("Eclipse Adoptium"));
            roots.push(b.join("Microsoft").join("jdk"));
            roots.push(b.join("Zulu"));
            roots.push(b.join("Amazon Corretto"));
            roots.push(b.join("BellSoft"));
            roots.push(b.join("Programs").join("Eclipse Adoptium"));
        }
    }
    for root in roots {
        let Ok(rd) = std::fs::read_dir(&root) else { continue };
        for e in rd.flatten() {
            cands.push((e.path().join("bin").join("java.exe"), "common_dir"));
        }
    }

    // PATH 里的 java
    if let Ok(out) = std::process::Command::new("where").arg("java").output() {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            if let Some(first) = text.lines().next() {
                let p = PathBuf::from(first.trim());
                if p.is_file() {
                    cands.push((p, "path"));
                }
            }
        }
    }

    let mut out: Vec<JavaRuntime> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for (path, source) in cands {
        let canon = path.canonicalize().unwrap_or_else(|_| path.clone());
        let key = canon.to_string_lossy().to_ascii_lowercase();
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        if let Some(rt) = probe(&canon, source) {
            out.push(rt);
        }
    }
    out.sort_by(|a, b| b.major.cmp(&a.major));
    out
}

/// 从实例的版本 JSON 里读需要的 Java 主版本（`javaVersion.majorVersion`）
pub fn required_major(game_dir: &Path, mc_version: &str) -> Option<u32> {
    let p = game_dir
        .join("versions")
        .join(mc_version)
        .join(format!("{mc_version}.json"));
    let text = std::fs::read_to_string(p).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v.get("javaVersion")
        .and_then(|j| j.get("majorVersion"))
        .and_then(|m| m.as_u64())
        .map(|m| m as u32)
}

/// 为某个版本挑一个可用的 Java：优先主版本相等，其次更高
pub fn pick_for(runtimes: &[JavaRuntime], required: u32) -> Option<JavaRuntime> {
    runtimes
        .iter()
        .find(|r| r.major == required)
        .or_else(|| runtimes.iter().filter(|r| r.major > required).min_by_key(|r| r.major))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn major_parsing() {
        assert_eq!(parse_major("1.8.0_402"), Some(8));
        assert_eq!(parse_major("17.0.9"), Some(17));
        assert_eq!(parse_major("21"), Some(21));
        assert_eq!(parse_major("25.0.1+8-LTS"), Some(25));
        assert_eq!(parse_major("garbage"), None);
    }

    #[test]
    fn picking_prefers_exact_then_lowest_higher() {
        let mk = |m: u32| JavaRuntime {
            path: format!("j{m}"),
            version: m.to_string(),
            major: m,
            vendor: "t".into(),
            bits: Some(64),
            source: "t".into(),
        };
        let list = vec![mk(25), mk(21), mk(17)];
        assert_eq!(pick_for(&list, 17).unwrap().major, 17);
        assert_eq!(pick_for(&list, 18).unwrap().major, 21);
        assert_eq!(pick_for(&list, 30).map(|r| r.major), None);
    }
}
