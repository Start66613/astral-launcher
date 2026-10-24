//! 模组更新检查（契约 V2 §5 的补充）。
//!
//! 用 jar 的 SHA1 去 Modrinth 反查项目与版本，再比对"当前实例版本 + 加载器"
//! 下最新的兼容文件。全部只读，不会改动文件。

use serde::Serialize;
use std::path::Path;

const API: &str = "https://api.modrinth.com/v2";

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModUpdate {
    pub file_name: String,
    pub project_id: String,
    pub current_version: String,
    pub latest_version: String,
    pub latest_version_id: String,
    pub latest_file_name: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UpdateReport {
    pub checked: u32,
    pub unknown: u32,
    pub updates: Vec<ModUpdate>,
    /// 被截断时提示前端还有多少没查
    pub skipped: u32,
}

fn hash_of(path: &Path) -> Option<String> {
    crate::net::sha1_file(path).ok()
}

/// 单个文件 hash → Modrinth 版本对象（GET /version_file/{hash}）
fn version_by_hash(sha1: &str) -> Option<serde_json::Value> {
    let url = format!("{API}/version_file/{sha1}?algorithm=sha1");
    crate::net::fetch_json(&url).ok()
}

fn str_of(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

/// 检查实例里的模组更新；`limit` 控制最多查多少个文件（默认 20）
pub fn check(
    mods_dir: &Path,
    game_version: &str,
    loader: Option<&str>,
    limit: Option<u32>,
) -> UpdateReport {
    let limit = limit.unwrap_or(20).clamp(1, 200) as usize;
    let files: Vec<_> = std::fs::read_dir(mods_dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| {
                    let n = e.file_name().to_string_lossy().to_lowercase();
                    n.ends_with(".jar")
                })
                .map(|e| e.path())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let skipped = files.len().saturating_sub(limit) as u32;
    let mut checked = 0u32;
    let mut unknown = 0u32;
    let mut updates = Vec::new();

    for path in files.into_iter().take(limit) {
        let Some(sha1) = hash_of(&path) else {
            unknown += 1;
            continue;
        };
        let Some(cur) = version_by_hash(&sha1) else {
            // 不是 Modrinth 来源（或已下架），无法判断
            unknown += 1;
            continue;
        };
        checked += 1;

        let Some(project_id) = str_of(&cur, "project_id") else {
            unknown += 1;
            continue;
        };
        let current_version = str_of(&cur, "version_number").unwrap_or_default();

        // 该实例版本 + 加载器下的全部版本，取最新
        let mut url = format!("{API}/project/{project_id}/version");
        let mut query: Vec<String> = Vec::new();
        query.push(format!(
            "game_versions={}",
            urlencoding(game_version)
        ));
        if let Some(l) = loader {
            query.push(format!("loaders={}", urlencoding(l)));
        }
        if !query.is_empty() {
            url.push('?');
            url.push_str(&query.join("&"));
        }

        let Ok(list) = crate::net::fetch_json(&url) else {
            unknown += 1;
            continue;
        };
        let Some(arr) = list.as_array() else {
            unknown += 1;
            continue;
        };
        let Some(latest) = arr.first() else {
            continue; // 没有兼容版本：不算有更新
        };
        let latest_id = str_of(latest, "id").unwrap_or_default();
        let cur_id = str_of(&cur, "id").unwrap_or_default();
        if latest_id.is_empty() || latest_id == cur_id {
            continue;
        }
        let latest_version = str_of(latest, "version_number").unwrap_or_default();
        let latest_file_name = latest
            .get("files")
            .and_then(|f| f.as_array())
            .and_then(|a| a.first())
            .and_then(|f| f.get("filename"))
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string();

        updates.push(ModUpdate {
            file_name: path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            project_id,
            current_version,
            latest_version,
            latest_version_id: latest_id,
            latest_file_name,
        });
    }

    UpdateReport {
        checked,
        unknown,
        updates,
        skipped,
    }
}

/// Modrinth 的数组参数形如 `["1.20.1"]`，做最小转义
fn urlencoding(value: &str) -> String {
    format!("%5B%22{}%22%5D", value.replace('"', "%22"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_encoding() {
        assert_eq!(urlencoding("1.20.1"), "%5B%221.20.1%22%5D");
    }
}
