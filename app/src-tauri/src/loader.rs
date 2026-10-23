//! 加载器目录（契约 v3 §5.4 / §4.10）。
//!
//! 目前只做 Fabric：直接读 Fabric 官方 meta，不打包任何版本清单。

use serde::Serialize;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LoaderCatalog {
    pub kind: String,
    pub label: String,
    pub recommended: Option<String>,
    pub versions: Vec<String>,
}

const META: &str = "https://meta.fabricmc.net/v2/versions/loader";

/// 纯解析，方便单测：Fabric meta 的 loader 列表
pub fn parse_fabric(json: &str) -> Vec<String> {
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

fn recommended_fabric(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let arr = v.as_array()?;
    // 优先 stable，否则取第一个
    arr.iter()
        .find(|e| {
            e.get("loader")
                .and_then(|l| l.get("stable"))
                .and_then(|s| s.as_bool())
                .unwrap_or(false)
        })
        .or_else(|| arr.first())
        .and_then(|e| e.get("loader")?.get("version")?.as_str())
        .map(|s| s.to_string())
}

fn fetch_cached(url: &str) -> Result<String, String> {
    let mut single = portablemc::download::single_cached(url);
    single
        .download(())
        .map_err(|e| format!("NETWORK|拉取加载器清单失败|{e:?}"))?;
    std::fs::read_to_string(single.file()).map_err(|e| format!("INTERNAL|读取清单失败|{e}"))
}

/// 某个游戏版本可用的加载器
pub fn list(game_version: &str) -> Result<Vec<LoaderCatalog>, String> {
    let url = format!("{META}/{game_version}");
    let json = fetch_cached(&url)?;
    let versions = parse_fabric(&json);

    if versions.is_empty() {
        return Err(format!(
            "VERSION_NOT_FOUND|Fabric 没有适配 {game_version} 的版本"
        ));
    }

    let recommended = recommended_fabric(&json).or_else(|| versions.first().cloned());
    Ok(vec![LoaderCatalog {
        kind: "fabric".into(),
        label: "Fabric".into(),
        recommended,
        versions,
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"[
      {"loader":{"version":"0.16.9","stable":true},"intermediary":{"version":"1.20.1"}},
      {"loader":{"version":"0.16.8","stable":false},"intermediary":{"version":"1.20.1"}}
    ]"#;

    #[test]
    fn parse_versions() {
        assert_eq!(parse_fabric(SAMPLE), vec!["0.16.9", "0.16.8"]);
    }

    #[test]
    fn recommended_prefers_stable() {
        assert_eq!(recommended_fabric(SAMPLE).as_deref(), Some("0.16.9"));
    }

    #[test]
    fn empty_json_is_safe() {
        assert!(parse_fabric("[]").is_empty());
        assert_eq!(recommended_fabric("[]"), None);
        assert!(parse_fabric("not json").is_empty());
    }
}
