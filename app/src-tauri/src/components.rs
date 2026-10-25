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
/// 某个组件的静态信息（供计划校验与版本列表使用）
pub struct SpecInfo {
    pub project_id: &'static str,
    pub label: &'static str,
    pub allowed: &'static [&'static str],
    pub note: Option<&'static str>,
}

pub fn spec_info(id: &str) -> Option<SpecInfo> {
    SPECS.iter().find(|s| s.id == id).map(|s| SpecInfo {
        project_id: s.project_id,
        label: s.label,
        allowed: s.allowed,
        note: s.note,
    })
}

/// 组件的全部兼容发行版本（供玩家自选）
#[derive(serde::Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ComponentVersion {
    pub version_id: String,
    pub version_number: String,
    pub channel: String,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub file_name: String,
    pub size: u64,
    pub published_at: String,
    pub recommended: bool,
}

pub fn versions(
    id: &str,
    game_version: &str,
    loader: Option<&str>,
) -> Result<Vec<ComponentVersion>, String> {
    let spec = spec_info(id)
        .ok_or_else(|| "INVALID_VALUE|不认识的光影组件".to_string())?;
    if let Some(note) = spec.note {
        return Err(format!("UNSUPPORTED|{}|{note}", spec.label));
    }
    if !loader_allows(spec.allowed, loader) {
        return Err(format!(
            "INCOMPATIBLE|{} 需要 {}，当前是 {}",
            spec.label,
            spec.allowed.join(" / "),
            loader.unwrap_or("原版")
        ));
    }
    let files = crate::resources::files(spec.project_id, Some(game_version), loader)?;
    let mut out: Vec<ComponentVersion> = files
        .into_iter()
        .filter(|f| f.game_versions.iter().any(|g| g == game_version))
        .map(|f| ComponentVersion {
            version_id: f.version_id,
            version_number: f.version_number,
            channel: f.channel,
            game_versions: f.game_versions,
            loaders: f.loaders,
            file_name: f.file_name,
            size: f.size,
            published_at: f.published_at,
            recommended: false,
        })
        .collect();
    // 稳定性优先级：release > beta > alpha，同级别按接口返回顺序（新的在前）
    out.sort_by_key(|v| match v.channel.as_str() {
        "release" => 0,
        "beta" => 1,
        _ => 2,
    });
    if let Some(first) = out.first_mut() {
        first.recommended = true;
    }
    Ok(out)
}

/// 校验"玩家选中的版本"确实属于该组件、且与游戏版本/加载器兼容
pub fn validate_choice(
    id: &str,
    version_id: &str,
    game_version: &str,
    loader: Option<&str>,
) -> Result<(crate::resources::ResourceFile, String), String> {
    let spec = spec_info(id)
        .ok_or_else(|| "INVALID_VALUE|不认识的光影组件".to_string())?;
    if let Some(note) = spec.note {
        return Err(format!("UNSUPPORTED|{}|{note}", spec.label));
    }
    let (file, project) = crate::resources::version_detail(version_id)?;
    if project.as_deref() != Some(spec.project_id) {
        return Err("INVALID_VALUE|该版本不属于所选组件".into());
    }
    if !file.game_versions.iter().any(|g| g == game_version) {
        return Err(format!(
            "INCOMPATIBLE|{} {} 不支持 Minecraft {}",
            spec.label, file.version_number, game_version
        ));
    }
    if let Some(l) = loader {
        if !file.loaders.iter().any(|x| x.eq_ignore_ascii_case(l)) {
            return Err(format!(
                "INCOMPATIBLE|{} {} 不支持加载器 {}",
                spec.label, file.version_number, l
            ));
        }
    }
    Ok((file, spec.project_id.to_string()))
}
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
