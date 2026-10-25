//! 光影支持组件目录（契约 V2 §6）。
//!
//! 光影**支持组件**（Iris / Oculus）本质是 mod，装法就是资源安装；
//! 这里只负责回答"这个 MC 版本 + 加载器能不能用、用哪个版本"。
//!
//! OptiFine 不走这条路：它没有官方 API，分发条款也不允许自动下载，
//! 所以永远标记为不可自动安装。

use serde::Serialize;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ComponentBuild {
    pub id: String,
    pub label: String,
    pub project_id: String,
    /// 该组件要求的加载器，null 表示不限
    pub required_loader: Option<String>,
    /// Modrinth 的 version id（安装时用它，不是版本号）
    #[serde(default)]
    pub version_id: Option<String>,
    /// 展示用版本号，例如 1.7.6+1.20.1
    pub version: Option<String>,
    pub file_name: Option<String>,
    pub published_at: Option<String>,
    pub stability: String,
    pub recommended: bool,
    pub available: bool,
    pub unavailable_reason: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ComponentCatalog {
    pub shader_support: Vec<ComponentBuild>,
    pub fetched_at: String,
}

struct Spec {
    id: &'static str,
    label: &'static str,
    project_id: &'static str,
    loader: Option<&'static str>,
    /// 该组件适用的加载器集合（空表示不限）
    allowed: &'static [&'static str],
    note: Option<&'static str>,
}

const SPECS: &[Spec] = &[
    Spec {
        id: "iris",
        label: "Iris Shaders",
        project_id: "YL57xq9U",
        loader: Some("fabric"),
        allowed: &["fabric", "quilt"],
        note: None,
    },
    Spec {
        id: "oculus",
        label: "Oculus",
        project_id: "GchcoXML",
        loader: Some("forge"),
        allowed: &["forge"],
        note: None,
    },
    Spec {
        id: "optifine",
        label: "OptiFine",
        project_id: "",
        loader: None,
        allowed: &[],
        note: Some("OptiFine 没有公开 API，分发条款也不允许自动下载，请手动安装"),
    },
];

/// 组件与目标加载器是否兼容（纯函数，便于测试）
pub fn loader_allows(allowed: &[&str], loader: Option<&str>) -> bool {
    if allowed.is_empty() {
        return false;
    }
    match loader {
        Some(l) => allowed.iter().any(|a| a.eq_ignore_ascii_case(l)),
        None => false,
    }
}

/// 列光影支持组件；只查网络，不写文件
pub fn catalog(game_version: &str, loader: Option<&str>) -> ComponentCatalog {
    let mut out = Vec::new();

    for spec in SPECS {
        let mut build = ComponentBuild {
            id: spec.id.into(),
            label: spec.label.into(),
            project_id: spec.project_id.into(),
            required_loader: spec.loader.map(|s| s.to_string()),
            version_id: None,
            version: None,
            file_name: None,
            published_at: None,
            stability: "unknown".into(),
            recommended: false,
            available: false,
            unavailable_reason: None,
        };

        if let Some(note) = spec.note {
            build.unavailable_reason = Some(note.to_string());
            out.push(build);
            continue;
        }

        if !loader_allows(spec.allowed, loader) {
            build.unavailable_reason = Some(format!(
                "需要 {}，当前实例是 {}",
                spec.allowed.join(" / "),
                loader.unwrap_or("原版")
            ));
            out.push(build);
            continue;
        }

        match crate::resources::files(spec.project_id, Some(game_version), loader) {
            Ok(files) if !files.is_empty() => {
                let pick = files
                    .iter()
                    .find(|f| f.channel == "release")
                    .unwrap_or(&files[0]);
                build.version_id = Some(pick.version_id.clone());
                build.version = Some(pick.version_number.clone());
                build.file_name = Some(pick.file_name.clone());
                build.published_at = Some(pick.published_at.clone());
                build.stability = pick.channel.clone();
                build.recommended = true;
                build.available = true;
            }
            Ok(_) => {
                build.unavailable_reason = Some(format!(
                    "{} 没有适配 Minecraft {} 的版本",
                    spec.label, game_version
                ));
            }
            Err(e) => {
                build.unavailable_reason = Some(crate::task::decode(&e).message);
            }
        }
        out.push(build);
    }

    ComponentCatalog {
        shader_support: out,
        fetched_at: crate::time::now_iso(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loader_matching() {
        assert!(loader_allows(&["fabric", "quilt"], Some("fabric")));
        assert!(loader_allows(&["fabric", "quilt"], Some("QUILT")));
        assert!(!loader_allows(&["fabric"], Some("forge")));
        assert!(!loader_allows(&["fabric"], None));
        assert!(!loader_allows(&[], Some("fabric")));
    }
}
