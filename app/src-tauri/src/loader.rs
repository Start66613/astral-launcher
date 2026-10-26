//! 加载器目录（契约 V2 §2.1）。
//!
//! 数据全部来自各家公开元数据，本地缓存：
//! - Fabric / Quilt：meta 的 JSON
//! - Forge：maven-metadata.xml + promotions_slim.json
//! - NeoForge：maven-metadata.xml（1.20.1 走 forge 命名空间）
//!
//! 只做「列出真实可用的构建」，安装仍走 `start_install_loader`（Fabric/Quilt）。

use serde::Serialize;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LoaderCatalog {
    pub kind: String,
    pub label: String,
    pub recommended: Option<String>,
    pub versions: Vec<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LoaderBuild {
    pub id: String,
    pub kind: String,
    pub version: String,
    pub mc_version: String,
    pub stability: String,
    pub recommended: bool,
    pub published_at: Option<String>,
    pub available: bool,
    pub unavailable_reason: Option<String>,
}

const FABRIC_META: &str = "https://meta.fabricmc.net/v2/versions/loader";
const QUILT_META: &str = "https://meta.quiltmc.org/v3/versions/loader";
const FORGE_META: &str = "https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml";
const FORGE_PROMOS: &str = "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json";
const NEOFORGE_META: &str = "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml";
const NEOFORGE_LEGACY_META: &str = "https://maven.neoforged.net/releases/net/neoforged/forge/maven-metadata.xml";

/// 所有下载都要过白名单（这些 URL 也会走 net 校验）
/// 拉取清单：走带缓存的下载，断网时回落到上次成功的结果。
fn fetch(url: &str) -> Result<String, String> {
    crate::net::check_url(url)?;
    let dest = std::env::temp_dir().join(format!(
        "astral-loader-{:x}.txt",
        crate::time::now_unix_ms()
    ));
    let opts = crate::net::DownloadOptions {
        max_attempts: 2,
        resume: false,
        reuse_verified: false,
        ..Default::default()
    };
    let cancel = crate::net::never_cancel();
    let res = crate::net::download(url, &dest, &opts, &*cancel, |_, _| {});
    if let Err(e) = res {
        let _ = std::fs::remove_file(&dest);
        // 断网时读元数据缓存（文本形式）
        if let Some((body, info)) = crate::net::meta_read(url) {
            if let Ok(text) = String::from_utf8(body) {
                if !text.trim().is_empty() {
                    let age = crate::time::now_unix_ms().saturating_sub(info.fetched_ms);
                    let _ = age;
                    return Ok(text);
                }
            }
        }
        return Err(e);
    }
    let text = std::fs::read_to_string(&dest)
        .map_err(|e| format!("INTERNAL|读取清单失败|{e}"))?;
    let _ = std::fs::remove_file(&dest);
    let _ = crate::net::meta_write(url, text.as_bytes(), None);
    Ok(text)
}

// ── 解析 ────────────────────────────────────────────────────────

pub(crate) fn parse_fabric(json: &str) -> Vec<(String, bool)> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let Some(arr) = v.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|e| {
            let l = e.get("loader")?;
            let ver = l.get("version")?.as_str()?.to_string();
            let stable = l.get("stable").and_then(|s| s.as_bool()).unwrap_or(false);
            Some((ver, stable))
        })
        .collect()
}

fn parse_quilt(json: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let Some(arr) = v.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|e| e.get("loader")?.get("version")?.as_str())
        .map(|s| s.to_string())
        .collect()
}

/// `<version>` 标签内容
fn xml_versions(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<version>") {
        rest = &rest[start + 9..];
        let Some(end) = rest.find("</version>") else {
            break;
        };
        out.push(rest[..end].trim().to_string());
        rest = &rest[end + 10..];
    }
    out
}

fn reverse(mut v: Vec<String>) -> Vec<String> {
    v.reverse();
    v
}

// ── 各加载器 ────────────────────────────────────────────────────

fn fabric_builds(mc: &str) -> Result<Vec<LoaderBuild>, String> {
    let json = fetch(&format!("{FABRIC_META}/{mc}"))?;
    let list = parse_fabric(&json);
    if list.is_empty() {
        return Ok(Vec::new());
    }
    let rec = list.iter().find(|(_, s)| *s).map(|(v, _)| v.clone());
    Ok(list
        .into_iter()
        .map(|(v, stable)| LoaderBuild {
            id: format!("fabric-{v}"),
            kind: "fabric".into(),
            version: v,
            mc_version: mc.to_string(),
            stability: if stable { "stable".into() } else { "unknown".into() },
            recommended: false,
            published_at: None,
            available: true,
            unavailable_reason: None,
        })
        .map(|mut b| {
            b.recommended = rec.as_deref() == Some(b.version.as_str());
            b
        })
        .collect())
}

fn quilt_builds(mc: &str) -> Result<Vec<LoaderBuild>, String> {
    let json = fetch(&format!("{QUILT_META}/{mc}"))?;
    let list = parse_quilt(&json);
    let rec = list.first().cloned();
    Ok(list
        .into_iter()
        .map(|v| LoaderBuild {
            id: format!("quilt-{v}"),
            kind: "quilt".into(),
            version: v,
            mc_version: mc.to_string(),
            // Quilt meta 不提供稳定标记，不推断
            stability: "unknown".into(),
            recommended: false,
            published_at: None,
            available: true,
            unavailable_reason: None,
        })
        .map(|mut b| {
            b.recommended = rec.as_deref() == Some(b.version.as_str());
            b
        })
        .collect())
}

fn forge_recommended(mc: &str) -> Option<String> {
    let text = fetch(FORGE_PROMOS).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v.get("promos")?
        .get(&format!("{mc}-recommended"))?
        .as_str()
        .map(|s| s.to_string())
}

fn stability_of(version: &str) -> String {
    let lower = version.to_ascii_lowercase();
    if lower.contains("beta") {
        "beta".into()
    } else if lower.contains("alpha") {
        "alpha".into()
    } else {
        "stable".into()
    }
}

fn make(
    kind: &str,
    mc: &str,
    versions: Vec<String>,
    recommended: Option<String>,
) -> Vec<LoaderBuild> {
    versions
        .into_iter()
        .map(|v| {
            let rec = recommended.as_deref() == Some(v.as_str());
            let stab = stability_of(&v);
            LoaderBuild {
                id: format!("{kind}-{v}"),
                kind: kind.to_string(),
                version: v,
                mc_version: mc.to_string(),
                stability: stab,
                recommended: rec,
                published_at: None,
                available: true,
                unavailable_reason: None,
            }
        })
        .collect()
}

fn forge_builds(mc: &str) -> Result<Vec<LoaderBuild>, String> {
    let xml = fetch(FORGE_META)?;
    let prefix = format!("{mc}-");
    let mut versions: Vec<String> = xml_versions(&xml)
        .into_iter()
        .filter_map(|v| v.strip_prefix(&prefix).map(|s| s.to_string()))
        .collect();
    versions = reverse(versions);
    let rec = forge_recommended(mc);
    Ok(make("forge", mc, versions, rec))
}

/// MC 版本 → NeoForge 的版本前缀（1.20.2 → `20.2.`，1.21.1 → `21.1.`）
/// NeoForge 的版本前缀：1.20.2 → "20.2."、1.21 → "21.0."、1.21.1 → "21.1."
///
/// 只对 NeoForge 实际支持的版本返回 Some；1.20.1 走 legacy 仓库，不在这里。
fn neoforge_prefix(mc: &str) -> Option<String> {
    let rest = mc.strip_prefix("1.")?;
    let parts: Vec<&str> = rest.split('.').collect();
    let major: u32 = parts.first()?.parse().ok()?;
    if major < 20 {
        return None;
    }
    let minor: u32 = parts.get(1).copied().unwrap_or("0").parse().ok()?;
    if major == 20 && minor < 2 {
        return None;
    }
    Some(format!("{major}.{minor}."))
}

fn neoforge_builds(mc: &str) -> Result<Vec<LoaderBuild>, String> {
    // 1.20.1 的 NeoForge 走 net/neoforged/forge，版本形如 1.20.1-47.1.x
    if mc == "1.20.1" {
        let xml = fetch(NEOFORGE_LEGACY_META)?;
        let mut versions: Vec<String> = xml_versions(&xml)
            .into_iter()
            .filter_map(|v| v.strip_prefix("1.20.1-").map(|s| s.to_string()))
            .collect();
        versions = reverse(versions);
        let rec = versions.first().cloned();
        return Ok(make("neoforge", mc, versions, rec));
    }

    let Some(prefix) = neoforge_prefix(mc) else {
        return Ok(Vec::new());
    };
    let xml = fetch(NEOFORGE_META)?;
    let mut versions: Vec<String> = xml_versions(&xml)
        .into_iter()
        .filter(|v| v.starts_with(&prefix))
        .collect();
    versions = reverse(versions);
    let rec = versions.first().cloned();
    Ok(make("neoforge", mc, versions, rec))
}

pub fn builds(mc: &str, kind: &str) -> Result<Vec<LoaderBuild>, String> {
    match kind {
        "fabric" => fabric_builds(mc),
        "quilt" => quilt_builds(mc),
        "forge" => forge_builds(mc),
        "neoforge" => neoforge_builds(mc),
        other => Err(format!("UNSUPPORTED|未知的加载器类型：{other}")),
    }
}

// ── 兼容旧接口 ──────────────────────────────────────────────────

/// 旧的 `list_loaders`：Fabric + Quilt 的版本列表
pub fn list(game_version: &str) -> Result<Vec<LoaderCatalog>, String> {
    let mut out = Vec::new();

    let fabric = fabric_builds(game_version)?;
    if !fabric.is_empty() {
        out.push(LoaderCatalog {
            kind: "fabric".into(),
            label: "Fabric".into(),
            recommended: fabric
                .iter()
                .find(|b| b.recommended)
                .map(|b| b.version.clone()),
            versions: fabric.into_iter().map(|b| b.version).collect(),
        });
    }

    if let Ok(quilt) = quilt_builds(game_version) {
        if !quilt.is_empty() {
            out.push(LoaderCatalog {
                kind: "quilt".into(),
                label: "Quilt".into(),
                recommended: quilt.first().map(|b| b.version.clone()),
                versions: quilt.into_iter().map(|b| b.version).collect(),
            });
        }
    }

    if out.is_empty() {
        return Err(format!(
            "VERSION_NOT_FOUND|没有适配 {game_version} 的 Fabric/Quilt 版本"
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"[
      {"loader":{"version":"0.16.9","stable":true}},
      {"loader":{"version":"0.16.8","stable":false}}
    ]"#;

    #[test]
    fn fabric_flags() {
        assert_eq!(parse_fabric(SAMPLE).len(), 2);
        assert!(parse_fabric(SAMPLE)[0].1);
    }

    #[test]
    fn quilt_parse() {
        let j = r#"[{"loader":{"version":"0.26.0"}}]"#;
        assert_eq!(parse_quilt(j), vec!["0.26.0"]);
    }

    #[test]
    fn xml_parse() {
        let xml = "<metadata><versioning><versions><version>1.20.1-47.2.0</version><version>1.20.1-47.1.3</version></versions></versioning></metadata>";
        assert_eq!(xml_versions(xml).len(), 2);
    }

    #[test]
    fn neoforge_prefix_map() {
        assert_eq!(neoforge_prefix("1.20.2").as_deref(), Some("20.2."));
        assert_eq!(neoforge_prefix("1.21.1").as_deref(), Some("21.1."));
        assert_eq!(neoforge_prefix("1.21").as_deref(), Some("21.0."));
        assert_eq!(neoforge_prefix("1.8.9"), None);
        assert_eq!(neoforge_prefix("1.20.1"), None);
        assert_eq!(neoforge_prefix("1.19.2"), None);
    }

    #[test]
    fn stability_flags() {
        assert_eq!(stability_of("47.2.0"), "stable");
        assert_eq!(stability_of("47.2.0-beta"), "beta");
    }
}
