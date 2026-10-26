//! Modrinth 项目详情页（V2.5 E33）。
//!
//! `resources::project` 只给标题、简介、图标、下载量四样，够列表用，不够详情页：
//! 详情页要图集、完整介绍正文、分类、客户端/服务端支持、关注数、相关链接。
//! 这里单独拉一次 `/project/{id}`，把详情页要的字段整理出来。
//!
//! 原则：只读、可离线（走 `fetch_json_cached`）、正文与图集都有上限，
//! 不会因为某个项目的介绍写了几万字就把 IPC 撑爆。

use serde::Serialize;

const API: &str = "https://api.modrinth.com/v2";

/// 正文最多给这么多字符，超了截断并标 `bodyTruncated`
pub const MAX_BODY_CHARS: usize = 20_000;
/// 图集最多给这么多张
pub const MAX_GALLERY: usize = 20;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GalleryImage {
    pub url: String,
    pub featured: bool,
    pub title: Option<String>,
    pub description: Option<String>,
    pub created: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LicenseInfo {
    pub id: Option<String>,
    pub name: Option<String>,
    pub url: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDetail {
    pub id: String,
    pub slug: Option<String>,
    pub title: String,
    pub description: String,
    /// 完整介绍（Markdown），已按 `MAX_BODY_CHARS` 截断
    pub body: String,
    pub body_chars: u32,
    pub body_truncated: bool,
    pub icon_url: Option<String>,
    pub project_type: String,
    pub categories: Vec<String>,
    pub additional_categories: Vec<String>,
    pub client_side: Option<String>,
    pub server_side: Option<String>,
    pub downloads: u64,
    pub followers: u64,
    pub published: Option<String>,
    pub updated: Option<String>,
    pub status: Option<String>,
    pub license: LicenseInfo,
    pub source_url: Option<String>,
    pub issues_url: Option<String>,
    pub wiki_url: Option<String>,
    pub discord_url: Option<String>,
    pub gallery: Vec<GalleryImage>,
    /// 这个项目支持的游戏版本（新的在前）
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub versions_count: u32,
    /// 数据来自网络还是本地缓存
    pub source: String,
    pub stale: bool,
    pub age_ms: i64,
    pub network_error: Option<String>,
}

fn str_of(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(|x| x.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_string())
}

fn str_list(v: &serde_json::Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn u64_of(v: &serde_json::Value, key: &str) -> u64 {
    v.get(key).and_then(|x| x.as_u64()).unwrap_or(0)
}

/// 按字符（不是字节）截断，避免把中文截成半个
fn truncate_chars(text: &str, max: usize) -> (String, bool) {
    let mut out = String::new();
    for (count, ch) in text.chars().enumerate() {
        if count >= max {
            return (out, true);
        }
        out.push(ch);
    }
    (out, false)
}

fn parse_gallery(v: &serde_json::Value) -> Vec<GalleryImage> {
    let arr = match v.get("gallery").and_then(|g| g.as_array()) {
        Some(a) => a,
        None => return Vec::new(),
    };
    let mut out: Vec<GalleryImage> = Vec::new();
    for item in arr {
        // 只要 https 的绝对地址，别的一律跳过（不猜相对路径）
        let url = match str_of(item, "url") {
            Some(u) if u.starts_with("https://") => u,
            _ => continue,
        };
        out.push(GalleryImage {
            url,
            featured: item
                .get("featured")
                .and_then(|f| f.as_bool())
                .unwrap_or(false),
            title: str_of(item, "title"),
            description: str_of(item, "description"),
            created: str_of(item, "created"),
        });
        if out.len() >= MAX_GALLERY {
            break;
        }
    }
    // 作者指定的封面图排最前
    out.sort_by_key(|g| std::cmp::Reverse(g.featured));
    out
}

fn parse_license(v: &serde_json::Value) -> LicenseInfo {
    let l = match v.get("license") {
        Some(l) if l.is_object() => l,
        _ => {
            return LicenseInfo {
                id: None,
                name: None,
                url: None,
            }
        }
    };
    LicenseInfo {
        id: str_of(l, "id"),
        name: str_of(l, "name"),
        url: str_of(l, "url"),
    }
}

/// 把 `/project/{id}` 的响应整理成详情页要的样子。
///
/// `__cache` 由 `net::fetch_json_cached` 注入，缺省当作网络来源。
pub fn parse_detail(v: &serde_json::Value, fallback_id: &str) -> Result<ProjectDetail, String> {
    if !v.is_object() {
        return Err("INTERNAL|项目详情格式异常".to_string());
    }
    let id = str_of(v, "id").unwrap_or_else(|| fallback_id.to_string());
    if id.is_empty() {
        return Err("INTERNAL|项目详情缺少 id".to_string());
    }

    let raw_body = str_of(v, "body").unwrap_or_default();
    let body_chars = raw_body.chars().count();
    let (body, body_truncated) = truncate_chars(&raw_body, MAX_BODY_CHARS);

    let cache = v.get("__cache");
    let source = cache
        .and_then(|c| str_of(c, "source"))
        .unwrap_or_else(|| "network".to_string());
    let stale = cache
        .and_then(|c| c.get("stale"))
        .and_then(|s| s.as_bool())
        .unwrap_or(false);
    let age_ms = cache
        .and_then(|c| c.get("ageMs"))
        .and_then(|a| a.as_i64())
        .unwrap_or(0);
    let network_error = cache.and_then(|c| str_of(c, "networkError"));

    let versions = v
        .get("versions")
        .and_then(|x| x.as_array())
        .map(|a| a.len() as u32)
        .unwrap_or(0);

    Ok(ProjectDetail {
        id,
        slug: str_of(v, "slug"),
        title: str_of(v, "title").unwrap_or_default(),
        description: str_of(v, "description").unwrap_or_default(),
        body,
        body_chars: body_chars.min(u32::MAX as usize) as u32,
        body_truncated,
        icon_url: str_of(v, "icon_url"),
        project_type: str_of(v, "project_type").unwrap_or_else(|| "mod".to_string()),
        categories: str_list(v, "categories"),
        additional_categories: str_list(v, "additional_categories"),
        client_side: str_of(v, "client_side"),
        server_side: str_of(v, "server_side"),
        downloads: u64_of(v, "downloads"),
        followers: u64_of(v, "followers"),
        published: str_of(v, "published"),
        updated: str_of(v, "updated"),
        status: str_of(v, "status"),
        license: parse_license(v),
        source_url: str_of(v, "source_url"),
        issues_url: str_of(v, "issues_url"),
        wiki_url: str_of(v, "wiki_url"),
        discord_url: str_of(v, "discord_url"),
        gallery: parse_gallery(v),
        game_versions: str_list(v, "game_versions"),
        loaders: str_list(v, "loaders"),
        versions_count: versions,
        source,
        stale,
        age_ms,
        network_error,
    })
}

fn fetch(base: &str, project_id: &str) -> Result<serde_json::Value, String> {
    let url = format!(
        "{base}/project/{}",
        crate::search::encode_component(project_id)
    );
    if base == API {
        crate::net::fetch_json_cached(&url, None)
    } else {
        crate::net::fetch_json_cached_for_test(&url, None)
    }
}

/// 拉一个项目的详情。`project_id` 可以是 id 也可以是 slug。
pub fn detail(project_id: &str) -> Result<ProjectDetail, String> {
    detail_at(API, project_id)
}

#[doc(hidden)]
pub fn detail_at(base: &str, project_id: &str) -> Result<ProjectDetail, String> {
    let id = project_id.trim();
    if id.is_empty() {
        return Err("INVALID_VALUE|项目 id 不能为空".to_string());
    }
    if id.len() > 128 {
        return Err("INVALID_VALUE|项目 id 太长了".to_string());
    }
    let v = fetch(base, id)?;
    parse_detail(&v, id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> serde_json::Value {
        serde_json::json!({
            "id": "AANobbMI",
            "slug": "sodium",
            "title": "Sodium",
            "description": "现代渲染引擎",
            "body": "## Sodium\n\n非常快。",
            "icon_url": "https://cdn.modrinth.com/data/AANobbMI/icon.png",
            "project_type": "mod",
            "categories": ["optimization", "decoration"],
            "additional_categories": ["worldgen"],
            "client_side": "required",
            "server_side": "unsupported",
            "downloads": 12345678,
            "followers": 4321,
            "published": "2020-01-01T00:00:00Z",
            "updated": "2025-06-01T00:00:00Z",
            "status": "approved",
            "license": { "id": "lgpl-3", "name": "LGPL-3.0", "url": "https://example.com/l" },
            "source_url": "https://github.com/x/y",
            "issues_url": "https://github.com/x/y/issues",
            "wiki_url": null,
            "discord_url": "https://discord.gg/x",
            "gallery": [
                { "url": "https://cdn.modrinth.com/a.png", "featured": false, "title": "A", "description": null, "created": "2024-01-01T00:00:00Z" },
                { "url": "https://cdn.modrinth.com/b.png", "featured": true, "title": "B", "description": "封面", "created": "2024-02-01T00:00:00Z" },
                { "url": "http://insecure/c.png", "featured": true }
            ],
            "game_versions": ["1.21.4", "1.21.1"],
            "loaders": ["fabric", "quilt"],
            "versions": ["v1", "v2", "v3"]
        })
    }

    #[test]
    fn parses_the_detail_fields() {
        let d = parse_detail(&sample(), "fallback").unwrap();
        assert_eq!(d.id, "AANobbMI");
        assert_eq!(d.slug.as_deref(), Some("sodium"));
        assert_eq!(d.title, "Sodium");
        assert_eq!(d.project_type, "mod");
        assert_eq!(d.categories, vec!["optimization", "decoration"]);
        assert_eq!(d.additional_categories, vec!["worldgen"]);
        assert_eq!(d.client_side.as_deref(), Some("required"));
        assert_eq!(d.server_side.as_deref(), Some("unsupported"));
        assert_eq!(d.downloads, 12_345_678);
        assert_eq!(d.followers, 4321);
        assert_eq!(d.versions_count, 3);
        assert_eq!(d.game_versions, vec!["1.21.4", "1.21.1"]);
        assert_eq!(d.loaders, vec!["fabric", "quilt"]);
        assert_eq!(d.license.id.as_deref(), Some("lgpl-3"));
        assert_eq!(d.license.name.as_deref(), Some("LGPL-3.0"));
        assert!(d.wiki_url.is_none());
        assert_eq!(d.source, "network");
        assert!(!d.stale);
    }

    #[test]
    fn keeps_only_https_gallery_and_puts_featured_first() {
        let d = parse_detail(&sample(), "fallback").unwrap();
        assert_eq!(d.gallery.len(), 2);
        assert_eq!(d.gallery[0].url, "https://cdn.modrinth.com/b.png");
        assert!(d.gallery[0].featured);
        assert_eq!(d.gallery[0].description.as_deref(), Some("封面"));
        assert_eq!(d.gallery[1].url, "https://cdn.modrinth.com/a.png");
    }

    #[test]
    fn truncates_a_long_body_by_chars() {
        let long = "字".repeat(MAX_BODY_CHARS + 500);
        let mut v = sample();
        v["body"] = serde_json::Value::String(long);
        let d = parse_detail(&v, "x").unwrap();
        assert!(d.body_truncated);
        assert_eq!(d.body.chars().count(), MAX_BODY_CHARS);
        assert_eq!(d.body_chars as usize, MAX_BODY_CHARS + 500);
    }

    #[test]
    fn missing_fields_stay_empty() {
        let v = serde_json::json!({ "id": "onlyid" });
        let d = parse_detail(&v, "fallback").unwrap();
        assert_eq!(d.id, "onlyid");
        assert_eq!(d.title, "");
        assert_eq!(d.project_type, "mod");
        assert!(d.body.is_empty());
        assert!(!d.body_truncated);
        assert!(d.gallery.is_empty());
        assert!(d.categories.is_empty());
        assert_eq!(d.downloads, 0);
        assert!(d.license.id.is_none());
    }

    #[test]
    fn rejects_non_object_and_empty_id() {
        assert!(parse_detail(&serde_json::json!("nope"), "x").is_err());
        assert!(parse_detail(&serde_json::json!({ "title": "no id" }), "").is_err());
    }

    #[test]
    fn reports_cache_source() {
        let mut v = sample();
        v["__cache"] = serde_json::json!({
            "source": "cache",
            "stale": true,
            "ageMs": 12345,
            "networkError": "连接超时"
        });
        let d = parse_detail(&v, "x").unwrap();
        assert_eq!(d.source, "cache");
        assert!(d.stale);
        assert_eq!(d.age_ms, 12345);
        assert_eq!(d.network_error.as_deref(), Some("连接超时"));
    }

    #[test]
    fn caps_the_gallery() {
        let items: Vec<serde_json::Value> = (0..(MAX_GALLERY + 10))
            .map(|i| serde_json::json!({ "url": format!("https://cdn.modrinth.com/{i}.png"), "featured": false }))
            .collect();
        let mut v = sample();
        v["gallery"] = serde_json::Value::Array(items);
        let d = parse_detail(&v, "x").unwrap();
        assert_eq!(d.gallery.len(), MAX_GALLERY);
    }
}
