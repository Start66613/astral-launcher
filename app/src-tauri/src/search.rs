//! Modrinth 资源搜索（契约 V2 §5 的补充）。
//!
//! 只做"查询 + 结构化"，不下载、不写文件。所有网络请求都走 `net` 的白名单
//! 与离线缓存，断网时返回最后一次有效结果并如实标注来源。
//!
//! 设计上把「拼 URL」「解析响应」抽成纯函数，便于在自检里用固定 JSON
//! 断言，不需要真的联网。

use serde::Serialize;

const API: &str = "https://api.modrinth.com/v2";

/// 允许搜索的资源类型。Modrinth 的 `project_type` 只有这几种。
pub const KINDS: [&str; 6] = [
    "mod",
    "modpack",
    "resourcepack",
    "shaderpack",
    "datapack",
    "plugin",
];

/// 允许的排序方式，与 Modrinth `index` 参数一致。
pub const SORTS: [&str; 5] = ["relevance", "downloads", "follows", "newest", "updated"];

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub author: String,
    pub project_type: String,
    pub categories: Vec<String>,
    pub display_categories: Vec<String>,
    pub downloads: u64,
    pub follows: u64,
    pub icon_url: Option<String>,
    pub license: Option<String>,
    pub versions: Vec<String>,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub latest_version: Option<String>,
    pub date_modified: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub hits: Vec<SearchHit>,
    pub total_hits: u64,
    pub offset: u64,
    pub limit: u64,
    pub index: String,
    pub query: String,
    /// `network` 表示这次真的请求了网络，`cache` 表示用的本地缓存
    pub source: String,
    /// 缓存是否已经过期（只有 `source == "cache"` 时才可能为 true）
    pub stale: bool,
    pub age_ms: i64,
    /// 网络失败但仍有缓存时的错误原文，方便前端提示"当前是离线结果"
    pub network_error: Option<String>,
}

/// 只保留 RFC 3986 的非保留字符，其余一律百分号编码。
///
/// 不能用 `format!` 拼裸 `[` `]` `"`：那要求服务端容忍非法 URL，
/// 换个实现就坏。
fn encode_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.as_bytes() {
        let keep = b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~');
        if keep {
            out.push(*b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// 校验并归一化资源类型；空字符串视为 `mod`。
pub fn normalize_kind(kind: Option<&str>) -> Result<String, String> {
    let raw = kind.unwrap_or("").trim().to_ascii_lowercase();
    let kind = if raw.is_empty() {
        "mod".to_string()
    } else {
        raw
    };
    if KINDS.contains(&kind.as_str()) {
        Ok(kind)
    } else {
        Err(format!("INVALID_VALUE|不支持的资源类型：{kind}"))
    }
}

/// 校验并归一化排序方式；空字符串视为 `relevance`。
pub fn normalize_sort(sort: Option<&str>) -> Result<String, String> {
    let raw = sort.unwrap_or("").trim().to_ascii_lowercase();
    let sort = if raw.is_empty() {
        "relevance".to_string()
    } else {
        raw
    };
    if SORTS.contains(&sort.as_str()) {
        Ok(sort)
    } else {
        Err(format!("INVALID_VALUE|不支持的排序方式：{sort}"))
    }
}

/// 拼搜索 URL。纯函数，自检可以直接断言。
///
/// facets 是"外层 OR、内层 AND"的二维数组：项目类型、游戏版本、加载器
/// 三组之间是 AND，所以各占一个内层数组。
pub fn build_url(
    query: &str,
    kind: &str,
    game_version: Option<&str>,
    loader: Option<&str>,
    limit: u32,
    offset: u32,
    index: &str,
) -> String {
    build_url_at(API, query, kind, game_version, loader, limit, offset, index)
}

/// 同 `build_url`，但可以指定 API 根地址（自检指向本地测试服务器用）。
#[allow(clippy::too_many_arguments)]
pub fn build_url_at(
    base: &str,
    query: &str,
    kind: &str,
    game_version: Option<&str>,
    loader: Option<&str>,
    limit: u32,
    offset: u32,
    index: &str,
) -> String {
    let limit = limit.clamp(1, 100);
    let mut groups: Vec<String> = vec![format!("[\"project_type:{kind}\"]")];
    if let Some(gv) = game_version.map(str::trim).filter(|s| !s.is_empty()) {
        groups.push(format!("[\"versions:{gv}\"]"));
    }
    if let Some(l) = loader.map(str::trim).filter(|s| !s.is_empty()) {
        groups.push(format!("[\"categories:{l}\"]"));
    }
    let facets = format!("[{}]", groups.join(","));
    format!(
        "{base}/search?query={}&facets={}&limit={}&offset={}&index={}",
        encode_component(query.trim()),
        encode_component(&facets),
        limit,
        offset,
        encode_component(index),
    )
}

fn str_list(v: &serde_json::Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str())
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn str_of(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

fn u64_of(v: &serde_json::Value, key: &str) -> u64 {
    v.get(key).and_then(|x| x.as_u64()).unwrap_or(0)
}

fn parse_hit(v: &serde_json::Value) -> Option<SearchHit> {
    let project_id = str_of(v, "project_id").or_else(|| str_of(v, "id"))?;
    Some(SearchHit {
        project_id,
        slug: str_of(v, "slug").unwrap_or_default(),
        title: str_of(v, "title").unwrap_or_default(),
        description: str_of(v, "description").unwrap_or_default(),
        author: str_of(v, "author").unwrap_or_default(),
        project_type: str_of(v, "project_type").unwrap_or_default(),
        categories: str_list(v, "categories"),
        display_categories: str_list(v, "display_categories"),
        downloads: u64_of(v, "downloads"),
        follows: u64_of(v, "follows"),
        icon_url: str_of(v, "icon_url"),
        license: str_of(v, "license"),
        versions: str_list(v, "versions"),
        game_versions: str_list(v, "game_versions"),
        loaders: str_list(v, "loaders"),
        latest_version: str_of(v, "latest_version"),
        date_modified: str_of(v, "date_modified"),
    })
}

/// 解析 Modrinth 搜索响应。`__cache` 由 `net::fetch_json_cached` 注入。
pub fn parse_result(v: &serde_json::Value, query: &str) -> Result<SearchResult, String> {
    let arr = v
        .get("hits")
        .and_then(|h| h.as_array())
        .ok_or_else(|| "INTERNAL|搜索结果格式异常".to_string())?;
    let hits: Vec<SearchHit> = arr.iter().filter_map(parse_hit).collect();
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
    Ok(SearchResult {
        hits,
        total_hits: u64_of(v, "total_hits"),
        offset: u64_of(v, "offset"),
        limit: u64_of(v, "limit"),
        index: str_of(v, "index").unwrap_or_default(),
        query: query.trim().to_string(),
        source,
        stale,
        age_ms,
        network_error,
    })
}

/// 执行一次搜索。`kind` / `sort` 会先校验，非法值直接报错而不是静默降级。
#[allow(clippy::too_many_arguments)]
pub fn search(
    query: &str,
    kind: Option<&str>,
    game_version: Option<&str>,
    loader: Option<&str>,
    limit: Option<u32>,
    offset: Option<u32>,
    sort: Option<&str>,
) -> Result<SearchResult, String> {
    let kind = normalize_kind(kind)?;
    let index = normalize_sort(sort)?;
    let limit = limit.unwrap_or(20).clamp(1, 100);
    let offset = offset.unwrap_or(0).min(10_000);
    let url = build_url(query, &kind, game_version, loader, limit, offset, &index);
    let v = crate::net::fetch_json_cached(&url, None)?;
    parse_result(&v, query)
}

/// 只给自检用：把 API 根地址换成 `base`，并允许 `http://127.0.0.1`。
///
/// 走的是和线上完全一样的 `build_url_at` → `fetch_json_cached_for_test`
/// → `parse_result` 链路，只是不强制 https、不校验白名单。
#[allow(clippy::too_many_arguments)]
#[doc(hidden)]
pub fn search_for_test(
    base: &str,
    query: &str,
    kind: Option<&str>,
    game_version: Option<&str>,
    loader: Option<&str>,
    limit: Option<u32>,
    offset: Option<u32>,
    sort: Option<&str>,
) -> Result<SearchResult, String> {
    let kind = normalize_kind(kind)?;
    let index = normalize_sort(sort)?;
    let limit = limit.unwrap_or(20).clamp(1, 100);
    let offset = offset.unwrap_or(0).min(10_000);
    let url = build_url_at(
        base,
        query,
        &kind,
        game_version,
        loader,
        limit,
        offset,
        &index,
    );
    // 默认 TTL：第一次请求会走网络，紧接着的第二次会命中刚写下的缓存。
    // 测试服务器的端口是随机的，所以每次运行都是新的缓存键，不会互相干扰。
    let v = crate::net::fetch_json_cached_for_test(&url, None)?;
    parse_result(&v, query)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_reserved_characters() {
        assert_eq!(encode_component("a b"), "a%20b");
        assert_eq!(encode_component("[\"x\"]"), "%5B%22x%22%5D");
        assert_eq!(encode_component("A-z_0.9~"), "A-z_0.9~");
    }

    #[test]
    fn builds_facets_for_each_filter() {
        let url = build_url(
            "sodium",
            "mod",
            Some("1.20.1"),
            Some("fabric"),
            20,
            0,
            "relevance",
        );
        assert!(url.starts_with("https://api.modrinth.com/v2/search?query=sodium"));
        // 三组 facet 必须都编码进 facets 参数
        assert!(url.contains("project_type%3Amod"));
        assert!(url.contains("versions%3A1.20.1"));
        assert!(url.contains("categories%3Afabric"));
        assert!(url.contains("limit=20"));
        assert!(url.contains("index=relevance"));
    }

    #[test]
    fn clamps_limit_and_offset() {
        let url = build_url("x", "mod", None, None, 0, 999_999, "downloads");
        assert!(url.contains("limit=1"));
        assert!(url.contains("offset=999999"));
    }

    #[test]
    fn rejects_unknown_kind_and_sort() {
        assert!(normalize_kind(Some("taco")).is_err());
        assert!(normalize_sort(Some("taco")).is_err());
        assert_eq!(normalize_kind(None).unwrap(), "mod");
        assert_eq!(normalize_sort(Some("")).unwrap(), "relevance");
        assert_eq!(normalize_kind(Some("MODPACK")).unwrap(), "modpack");
    }

    #[test]
    fn parses_hits_and_cache_flag() {
        let v = serde_json::json!({
            "hits": [{
                "project_id": "AANobbMI",
                "slug": "sodium",
                "title": "Sodium",
                "description": "Modern rendering engine",
                "author": "jellysquid3",
                "project_type": "mod",
                "categories": ["optimization"],
                "display_categories": ["Optimization"],
                "downloads": 12345,
                "follows": 678,
                "icon_url": "https://cdn.modrinth.com/x.png",
                "license": "lgpl-3",
                "versions": ["mc1.20.1-0.5.13"],
                "game_versions": ["1.20.1"],
                "loaders": ["fabric"],
                "latest_version": "mc1.20.1-0.5.13",
                "date_modified": "2024-01-01T00:00:00Z"
            }],
            "total_hits": 42,
            "offset": 0,
            "limit": 20,
            "index": "relevance",
            "__cache": { "source": "cache", "ageMs": 1234, "stale": true }
        });
        let r = parse_result(&v, " sodium ").unwrap();
        assert_eq!(r.hits.len(), 1);
        assert_eq!(r.hits[0].title, "Sodium");
        assert_eq!(r.hits[0].downloads, 12345);
        assert_eq!(r.total_hits, 42);
        assert_eq!(r.query, "sodium");
        assert_eq!(r.source, "cache");
        assert!(r.stale);
        assert_eq!(r.age_ms, 1234);
    }

    #[test]
    fn rejects_malformed_result() {
        assert!(parse_result(&serde_json::json!({"oops": true}), "x").is_err());
    }
}
