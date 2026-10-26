//! 本地 Mod 静态扫描：重复 ID、缺失依赖、版本范围不满足、加载器不匹配。
//!
//! 和 `deps.rs` 的分工：`deps.rs` 是**联网**从 Modrinth 递归解析依赖再下载；
//! 这里是**纯离线**读玩家 `mods/` 目录里已有的 jar，回答"为什么启动就崩"。
//! 两者共用不了数据（一个要 project id，一个只有 jar 里的 mod id），所以分开写。
//!
//! 只读，不删不改任何文件。判断依据全部来自 jar 内元数据，读不到就说读不到，
//! 不猜。

use serde::Serialize;
use std::io::Read;
use std::path::{Path, PathBuf};

/// 这些 mod id 是平台提供的，不算"缺失依赖"
const PLATFORM_IDS: [&str; 10] = [
    "minecraft",
    "java",
    "fabric",
    "fabricloader",
    "fabric-api",
    "quilt_loader",
    "quilt_base",
    "forge",
    "neoforge",
    "cloth-config",
];

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ScannedId {
    pub mod_id: String,
    pub name: String,
    pub version: String,
    pub loader: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ScannedDep {
    pub mod_id: String,
    pub version_range: Option<String>,
    /// 必需依赖：缺失会导致启动失败
    pub mandatory: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ScannedMod {
    pub file_name: String,
    pub enabled: bool,
    pub size_bytes: u64,
    pub ids: Vec<ScannedId>,
    pub depends: Vec<ScannedDep>,
    /// `breaks` / `conflicts` 声明
    pub conflicts: Vec<ScannedDep>,
    /// 读不出来时的原因（jar 损坏、没有可识别的元数据等）
    pub error: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModIssue {
    /// duplicate-id | missing-dependency | version-mismatch | broken-jar |
    /// no-metadata | declared-conflict | disabled-duplicate
    pub code: String,
    /// error | warn | info
    pub severity: String,
    pub file_name: Option<String>,
    pub mod_id: Option<String>,
    pub message: String,
    pub detail: Option<String>,
    /// high | medium | low —— 基于什么证据得出的结论
    pub confidence: String,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScanCounts {
    pub total: u32,
    pub enabled: u32,
    pub disabled: u32,
    pub readable: u32,
    pub unreadable: u32,
    pub errors: u32,
    pub warnings: u32,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModScanReport {
    pub mods: Vec<ScannedMod>,
    pub issues: Vec<ModIssue>,
    pub counts: ScanCounts,
    /// 一句话结论，没有问题时为 None
    pub summary: Option<String>,
}

fn split_name(file_name: &str) -> Option<(String, bool)> {
    let lower = file_name.to_ascii_lowercase();
    if let Some(stem) = lower.strip_suffix(".disabled") {
        if stem.ends_with(".jar") {
            let cut = file_name.len() - ".disabled".len();
            return Some((file_name[..cut].to_string(), false));
        }
        return None;
    }
    lower
        .ends_with(".jar")
        .then(|| (file_name.to_string(), true))
}

fn read_entry<R: Read + std::io::Seek>(zip: &mut zip::ZipArchive<R>, name: &str) -> Option<String> {
    const MAX_META: u64 = 1024 * 1024;
    let mut f = zip.by_name(name).ok()?;
    if f.size() > MAX_META {
        return None;
    }
    let mut s = String::new();
    f.read_to_string(&mut s).ok()?;
    Some(s)
}

fn json_str(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// `depends` 里的值可以是字符串、字符串数组，也可以是对象数组（带 version）。
/// 统一抽成 (modId, 版本范围)。
fn fabric_dep_list(v: &serde_json::Value, key: &str) -> Vec<ScannedDep> {
    let Some(obj) = v.get(key).and_then(|d| d.as_object()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (mod_id, val) in obj {
        let range = match val {
            serde_json::Value::String(s) => {
                let s = s.trim();
                (!s.is_empty() && s != "*").then(|| s.to_string())
            }
            serde_json::Value::Array(a) => {
                let mut parts = Vec::new();
                for item in a {
                    if let Some(s) = item.as_str() {
                        if !s.trim().is_empty() && s.trim() != "*" {
                            parts.push(s.trim().to_string());
                        }
                    } else if let Some(s) = item.get("version").and_then(|x| x.as_str()) {
                        if !s.trim().is_empty() && s.trim() != "*" {
                            parts.push(s.trim().to_string());
                        }
                    }
                }
                (!parts.is_empty()).then(|| parts.join(" || "))
            }
            serde_json::Value::Object(o) => o
                .get("version")
                .and_then(|x| x.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty() && s != "*"),
            _ => None,
        };
        out.push(ScannedDep {
            mod_id: mod_id.clone(),
            version_range: range,
            mandatory: true,
        });
    }
    out
}

/// 极简 TOML 读取：只处理 `key = "value"` 与 `[[dependencies.x]]` 段落。
///
/// 不引 `toml` crate 是为了不给启动器加一个只为读元数据的依赖；
/// 这里只需要几个固定字段，多行字符串、内联表一律不认（读不到就当没有）。
struct TomlMods {
    mod_id: Option<String>,
    display_name: Option<String>,
    version: Option<String>,
    deps: Vec<ScannedDep>,
    breaks: Vec<ScannedDep>,
}

fn unquote(v: &str) -> String {
    let v = v.split('#').next().unwrap_or(v).trim();
    v.trim_matches('"').trim_matches('\'').trim().to_string()
}

fn toml_parse(text: &str) -> TomlMods {
    let mut out = TomlMods {
        mod_id: None,
        display_name: None,
        version: None,
        deps: Vec::new(),
        breaks: Vec::new(),
    };
    let mut in_dep = false;
    // [[dependencies.xxx]] 里当前块是不是 breaks
    let mut dep_kind: Option<&'static str> = None;
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        if l.starts_with("[[") {
            let inner = l.trim_start_matches("[[").trim_end_matches("]]").trim();
            in_dep = inner.starts_with("dependencies.");
            dep_kind = if in_dep { Some("dep") } else { None };
            continue;
        }
        if l.starts_with('[') {
            in_dep = false;
            dep_kind = None;
            continue;
        }
        let Some(eq) = l.find('=') else { continue };
        let key = l[..eq].trim();
        let val = unquote(&l[eq + 1..]);
        if in_dep {
            // Forge 的 type 字段可能写在块首也可能写在块尾，见到就立刻改归类
            if key == "type" && val.eq_ignore_ascii_case("breaks") && dep_kind != Some("break") {
                dep_kind = Some("break");
                if let Some(moved) = out.deps.pop() {
                    out.breaks.push(moved);
                }
            }
            if key == "modId" && !val.is_empty() {
                // 先占位，等 mandatory 与 versionRange 读到再补
                let entry = ScannedDep {
                    mod_id: val.clone(),
                    version_range: None,
                    mandatory: true,
                };
                match dep_kind {
                    Some("break") => out.breaks.push(entry),
                    _ => out.deps.push(entry),
                }
            }
            if key == "mandatory" {
                let flag = !val.eq_ignore_ascii_case("false");
                if let Some(last) = match dep_kind {
                    Some("break") => out.breaks.last_mut(),
                    _ => out.deps.last_mut(),
                } {
                    last.mandatory = flag;
                }
            }
            if key == "versionRange" && !val.is_empty() && val != "*" {
                if let Some(last) = match dep_kind {
                    Some("break") => out.breaks.last_mut(),
                    _ => out.deps.last_mut(),
                } {
                    last.version_range = Some(val);
                }
            }
            continue;
        }
        match key {
            "modId" if out.mod_id.is_none() => {
                out.mod_id = (!val.is_empty()).then_some(val);
            }
            "displayName" if out.display_name.is_none() => {
                out.display_name = (!val.is_empty()).then_some(val);
            }
            "version" if out.version.is_none() => {
                out.version = (!val.is_empty()).then_some(val);
            }
            _ => {}
        }
    }
    out
}

fn scan_one(dir: &Path, file_name: &str, enabled: bool) -> ScannedMod {
    let path = dir.join(file_name);
    let size_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let mut out = ScannedMod {
        file_name: file_name.to_string(),
        enabled,
        size_bytes,
        ids: Vec::new(),
        depends: Vec::new(),
        conflicts: Vec::new(),
        error: None,
    };

    let Ok(file) = std::fs::File::open(&path) else {
        out.error = Some("文件打不开".into());
        return out;
    };
    let Ok(mut zip) = zip::ZipArchive::new(file) else {
        out.error = Some("不是有效的 jar/zip".into());
        return out;
    };

    // Fabric / Quilt
    if let Some(text) = read_entry(&mut zip, "fabric.mod.json") {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            let id = json_str(&v, "id").unwrap_or_default();
            if !id.is_empty() {
                out.ids.push(ScannedId {
                    mod_id: id,
                    name: json_str(&v, "name").unwrap_or_else(|| file_name.to_string()),
                    version: json_str(&v, "version").unwrap_or_else(|| "?".into()),
                    loader: "fabric".into(),
                });
                out.depends = fabric_dep_list(&v, "depends");
                out.conflicts = fabric_dep_list(&v, "breaks");
                return out;
            }
        }
    }
    if let Some(text) = read_entry(&mut zip, "quilt.mod.json") {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            let q = v.get("quilt_loader");
            let id = q
                .and_then(|x| x.get("id"))
                .and_then(|x| x.as_str())
                .unwrap_or_default();
            if !id.is_empty() {
                out.ids.push(ScannedId {
                    mod_id: id.to_string(),
                    name: q
                        .and_then(|x| x.get("metadata"))
                        .and_then(|m| m.get("name"))
                        .and_then(|n| n.as_str())
                        .unwrap_or(file_name)
                        .to_string(),
                    version: q
                        .and_then(|x| x.get("version"))
                        .and_then(|x| x.as_str())
                        .unwrap_or("?")
                        .to_string(),
                    loader: "quilt".into(),
                });
                if let Some(deps) = q.and_then(|x| x.get("depends")).and_then(|d| d.as_array()) {
                    for d in deps {
                        let dep_id = d.get("id").and_then(|x| x.as_str()).unwrap_or_default();
                        if dep_id.is_empty() {
                            continue;
                        }
                        let range = d
                            .get("versions")
                            .and_then(|x| x.as_str())
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty() && s != "*");
                        out.depends.push(ScannedDep {
                            mod_id: dep_id.to_string(),
                            version_range: range,
                            mandatory: true,
                        });
                    }
                }
                return out;
            }
        }
    }

    // Forge / NeoForge
    for name in ["META-INF/neoforge.mods.toml", "META-INF/mods.toml"] {
        if let Some(text) = read_entry(&mut zip, name) {
            let parsed = toml_parse(&text);
            let loader = if name.contains("neoforge") {
                "neoforge"
            } else {
                "forge"
            };
            // mods.toml 可能是多 mod 的，[[mods]] 段每段一个；这里只取第一个
            let id = parsed.mod_id.clone().unwrap_or_default();
            if !id.is_empty() {
                out.ids.push(ScannedId {
                    mod_id: id,
                    name: parsed.display_name.unwrap_or_else(|| file_name.to_string()),
                    version: parsed.version.unwrap_or_else(|| "?".into()),
                    loader: loader.into(),
                });
                out.depends = parsed.deps;
                out.conflicts = parsed.breaks;
                return out;
            }
        }
    }

    // 老版 mcmod.info
    if let Some(text) = read_entry(&mut zip, "META-INF/mcmod.info") {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(first) = v.as_array().and_then(|a| a.first()) {
                let id = json_str(first, "modid").unwrap_or_default();
                if !id.is_empty() {
                    out.ids.push(ScannedId {
                        mod_id: id,
                        name: json_str(first, "name").unwrap_or_else(|| file_name.to_string()),
                        version: json_str(first, "version").unwrap_or_else(|| "?".into()),
                        loader: "forge".into(),
                    });
                    return out;
                }
            }
        }
    }

    out.error = Some("jar 里没有可识别的 mod 元数据（可能不是 mod）".into());
    out
}

/// 版本比较：按 `.`、`-`、`+` 切成段，数字段按数值比，其余按字典序。
/// 比不出来时返回 None（调用方会降级成"无法判断"而不是判错）。
pub fn compare_versions(a: &str, b: &str) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    let split = |s: &str| -> Vec<String> {
        s.split(['.', '-', '+', '_'])
            .map(|x| x.trim().to_ascii_lowercase())
            .filter(|x| !x.is_empty())
            .collect()
    };
    let (av, bv) = (split(a), split(b));
    if av.is_empty() || bv.is_empty() {
        return None;
    }
    for i in 0..av.len().max(bv.len()) {
        let x = av.get(i).map(String::as_str).unwrap_or("0");
        let y = bv.get(i).map(String::as_str).unwrap_or("0");
        match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(nx), Ok(ny)) => {
                if nx != ny {
                    return Some(nx.cmp(&ny));
                }
            }
            // 一边数字一边文字：数字更新（1.20.1 > 1.20.1-beta）
            (Ok(_), Err(_)) => return Some(Ordering::Greater),
            (Err(_), Ok(_)) => return Some(Ordering::Less),
            (Err(_), Err(_)) => {
                if x != y {
                    return Some(x.cmp(y));
                }
            }
        }
    }
    Some(Ordering::Equal)
}

/// 单个约束匹配。支持 `*`、`>=1.2`、`>1.2`、`<=1.2`、`<1.2`、`=1.2`、`1.2.x`、`1.2.*`。
fn matches_one(version: &str, spec: &str) -> Option<bool> {
    use std::cmp::Ordering;
    let spec = spec.trim();
    if spec.is_empty() || spec == "*" {
        return Some(true);
    }
    let (op, rest) = if let Some(r) = spec.strip_prefix(">=") {
        (">=", r)
    } else if let Some(r) = spec.strip_prefix("<=") {
        ("<=", r)
    } else if let Some(r) = spec.strip_prefix('>') {
        (">", r)
    } else if let Some(r) = spec.strip_prefix('<') {
        ("<", r)
    } else if let Some(r) = spec.strip_prefix('=') {
        ("=", r)
    } else if let Some(r) = spec.strip_prefix('~') {
        ("~", r)
    } else {
        ("=", spec)
    };
    let rest = rest.trim();
    if rest.is_empty() || rest == "*" {
        return Some(true);
    }
    // 版本号里至少得有一个数字，否则是写错了（比如 ">=abc"），不能当成"满足"
    if !rest.bytes().any(|b| b.is_ascii_digit()) {
        return None;
    }

    // 通配：1.20.x / 1.20.*
    if let Some(prefix) = rest
        .strip_suffix(".x")
        .or_else(|| rest.strip_suffix(".*"))
        .or_else(|| rest.strip_suffix('.'))
    {
        let v = version.trim();
        let hit = v == prefix
            || v.strip_prefix(prefix)
                .is_some_and(|r| r.starts_with('.') || r.starts_with('-'));
        return Some(hit);
    }

    let ord = compare_versions(version, rest)?;
    Some(match op {
        ">=" => ord != Ordering::Less,
        ">" => ord == Ordering::Greater,
        "<=" => ord != Ordering::Greater,
        "<" => ord == Ordering::Less,
        "~" => {
            // ~1.20.1 意思是"1.20.x 里 >= 1.20.1"
            let base: Vec<&str> = rest.split('.').collect();
            if base.len() < 2 {
                return Some(ord != Ordering::Less);
            }
            let prefix = format!("{}.{}", base[0], base[1]);
            let same = version == prefix
                || version
                    .strip_prefix(&prefix)
                    .is_some_and(|r| r.starts_with('.') || r.starts_with('-'));
            same && ord != Ordering::Less
        }
        _ => ord == Ordering::Equal,
    })
}

/// 判断版本是否满足约束表达式。
///
/// 支持 `||` 分组、空格分隔的 AND（如 `>=1.20 <1.21`）、以及 `x`/`*` 通配。
/// **任何一段解析不出来就返回 None**，调用方必须把它当成"无法判断"而不是"不满足"。
pub fn satisfies(version: &str, range: &str) -> Option<bool> {
    if range.trim().is_empty() || range.trim() == "*" {
        return Some(true);
    }
    let mut any_group_ok = false;
    for group in range.split("||") {
        let group = group.trim();
        if group.is_empty() {
            continue;
        }
        let mut all_ok = true;
        let mut known = true;
        // 逗号与空格都当分隔符；但 "<1.20 >=1.19" 这种要分开看
        let parts: Vec<&str> = group
            .split([',', ' '])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        if parts.is_empty() {
            continue;
        }
        for p in parts {
            match matches_one(version, p) {
                Some(true) => {}
                Some(false) => {
                    all_ok = false;
                    break;
                }
                None => {
                    known = false;
                    break;
                }
            }
        }
        if !known {
            return None;
        }
        if all_ok {
            any_group_ok = true;
        }
    }
    Some(any_group_ok)
}

fn is_platform(mod_id: &str) -> bool {
    let lower = mod_id.to_ascii_lowercase();
    PLATFORM_IDS.contains(&lower.as_str()) || lower.starts_with("fabric-")
}

/// 扫描一个 mods 目录。
pub fn scan(dir: &Path) -> ModScanReport {
    let mut mods = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let path: PathBuf = e.path();
            if !path.is_file() {
                continue;
            }
            let file_name = e.file_name().to_string_lossy().to_string();
            let Some((_, enabled)) = split_name(&file_name) else {
                continue;
            };
            mods.push(scan_one(dir, &file_name, enabled));
        }
    }
    mods.sort_by_key(|m| m.file_name.to_lowercase());

    let mut issues: Vec<ModIssue> = Vec::new();

    // 1) 读不出来的 jar
    for m in &mods {
        if let Some(err) = &m.error {
            issues.push(ModIssue {
                code: if m.enabled {
                    "broken-jar"
                } else {
                    "no-metadata"
                }
                .into(),
                severity: if m.enabled { "error" } else { "info" }.into(),
                file_name: Some(m.file_name.clone()),
                mod_id: None,
                message: if m.enabled {
                    format!("{} 读不出 mod 信息", m.file_name)
                } else {
                    format!("{}（已禁用）读不出 mod 信息", m.file_name)
                },
                detail: Some(err.clone()),
                confidence: "high".into(),
            });
        }
    }

    // 2) 重复 mod id
    //    mod_id → [(file_name, version, enabled)]
    let mut by_id: std::collections::BTreeMap<String, Vec<(String, String, bool)>> =
        std::collections::BTreeMap::new();
    for m in &mods {
        for id in &m.ids {
            by_id
                .entry(id.mod_id.to_ascii_lowercase())
                .or_default()
                .push((m.file_name.clone(), id.version.clone(), m.enabled));
        }
    }
    for (id, list) in &by_id {
        if list.len() < 2 {
            continue;
        }
        let enabled: Vec<&(String, String, bool)> = list.iter().filter(|x| x.2).collect();
        if enabled.is_empty() {
            // 全都禁用了，只是提醒，不影响启动
            issues.push(ModIssue {
                code: "disabled-duplicate".into(),
                severity: "info".into(),
                file_name: None,
                mod_id: Some(id.clone()),
                message: format!("{id} 有 {} 个副本，但都已禁用", list.len()),
                detail: Some(
                    list.iter()
                        .map(|x| format!("{} ({})", x.0, x.1))
                        .collect::<Vec<_>>()
                        .join("、"),
                ),
                confidence: "high".into(),
            });
            continue;
        }
        let files = enabled
            .iter()
            .map(|x| x.0.clone())
            .collect::<Vec<_>>()
            .join("、");
        issues.push(ModIssue {
            code: "duplicate-id".into(),
            severity: "error".into(),
            file_name: enabled.first().map(|x| x.0.clone()),
            mod_id: Some(id.clone()),
            message: format!("mod id `{id}` 重复装了 {} 份", enabled.len()),
            detail: Some(format!(
                "{files}。同一个 mod id 出现多次通常会让游戏直接崩溃，请只保留一份"
            )),
            confidence: "high".into(),
        });
    }

    // 3) 缺失依赖 / 版本不满足（只看启用中的）
    let enabled_mods: Vec<&ScannedMod> = mods.iter().filter(|m| m.enabled).collect();
    for m in &enabled_mods {
        for dep in &m.depends {
            if is_platform(&dep.mod_id) {
                continue;
            }
            let key = dep.mod_id.to_ascii_lowercase();
            let present = by_id
                .get(&key)
                .map(|l| l.iter().filter(|x| x.2).collect::<Vec<_>>())
                .unwrap_or_default();
            if present.is_empty() {
                if !dep.mandatory {
                    continue;
                }
                issues.push(ModIssue {
                    code: "missing-dependency".into(),
                    severity: "error".into(),
                    file_name: Some(m.file_name.clone()),
                    mod_id: Some(dep.mod_id.clone()),
                    message: format!("{} 需要 {}，但 mods 里没有", m.file_name, dep.mod_id),
                    detail: dep
                        .version_range
                        .as_ref()
                        .map(|r| format!("要求的版本范围：{r}")),
                    confidence: "high".into(),
                });
                continue;
            }
            let Some(range) = dep.version_range.as_deref() else {
                continue;
            };
            // 任一已装版本满足即可
            let mut verdict = None;
            for (file, ver, _) in &present {
                match satisfies(ver, range) {
                    Some(true) => {
                        verdict = Some(true);
                        break;
                    }
                    Some(false) => {
                        verdict = Some(false);
                    }
                    None => {
                        verdict = None;
                        break;
                    }
                }
                let _ = file;
            }
            match verdict {
                Some(false) => {
                    let have = present
                        .iter()
                        .map(|x| format!("{} ({})", x.0, x.1))
                        .collect::<Vec<_>>()
                        .join("、");
                    issues.push(ModIssue {
                        code: "version-mismatch".into(),
                        severity: "warn".into(),
                        file_name: Some(m.file_name.clone()),
                        mod_id: Some(dep.mod_id.clone()),
                        message: format!(
                            "{} 要求 {} {range}，当前是 {have}",
                            m.file_name, dep.mod_id
                        ),
                        detail: Some("版本号可能写成别的格式，建议核对".into()),
                        confidence: "medium".into(),
                    });
                }
                None => {
                    issues.push(ModIssue {
                        code: "version-mismatch".into(),
                        severity: "info".into(),
                        file_name: Some(m.file_name.clone()),
                        mod_id: Some(dep.mod_id.clone()),
                        message: format!(
                            "{} 对 {} 的版本要求是 {range}，这个格式暂时判断不了",
                            m.file_name, dep.mod_id
                        ),
                        detail: Some("不是错误，只是没法自动核对".into()),
                        confidence: "low".into(),
                    });
                }
                Some(true) => {}
            }
        }
    }

    // 4) 显式声明的冲突
    for m in &enabled_mods {
        for c in &m.conflicts {
            let key = c.mod_id.to_ascii_lowercase();
            if by_id
                .get(&key)
                .map(|l| l.iter().any(|x| x.2))
                .unwrap_or(false)
            {
                issues.push(ModIssue {
                    code: "declared-conflict".into(),
                    severity: "warn".into(),
                    file_name: Some(m.file_name.clone()),
                    mod_id: Some(c.mod_id.clone()),
                    message: format!("{} 声明与 {} 冲突，但两者都装了", m.file_name, c.mod_id),
                    detail: c
                        .version_range
                        .as_ref()
                        .map(|r| format!("冲突声明的版本范围：{r}")),
                    confidence: "high".into(),
                });
            }
        }
    }

    issues.sort_by_key(|i| match i.severity.as_str() {
        "error" => 0,
        "warn" => 1,
        _ => 2,
    });

    let total = mods.len() as u32;
    let enabled = mods.iter().filter(|m| m.enabled).count() as u32;
    let unreadable = mods.iter().filter(|m| m.error.is_some()).count() as u32;
    let counts = ScanCounts {
        total,
        enabled,
        disabled: total - enabled,
        readable: total - unreadable,
        unreadable,
        errors: issues.iter().filter(|i| i.severity == "error").count() as u32,
        warnings: issues.iter().filter(|i| i.severity == "warn").count() as u32,
    };
    let summary = if counts.errors > 0 {
        Some(format!(
            "发现 {} 个会导致启动失败的问题，建议先处理",
            counts.errors
        ))
    } else if counts.warnings > 0 {
        Some(format!("有 {} 个可能影响运行的问题", counts.warnings))
    } else {
        None
    };

    ModScanReport {
        mods,
        issues,
        counts,
        summary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare_orders_numerically() {
        use std::cmp::Ordering;
        assert_eq!(compare_versions("1.20.1", "1.20.1"), Some(Ordering::Equal));
        assert_eq!(compare_versions("1.20.2", "1.20.10"), Some(Ordering::Less));
        assert_eq!(compare_versions("1.21", "1.20.9"), Some(Ordering::Greater));
        assert_eq!(compare_versions("", "1.0"), None);
    }

    #[test]
    fn satisfies_handles_common_ranges() {
        assert_eq!(satisfies("1.20.1", "*"), Some(true));
        assert_eq!(satisfies("1.20.1", ">=1.20"), Some(true));
        assert_eq!(satisfies("1.19.4", ">=1.20"), Some(false));
        assert_eq!(satisfies("1.20.1", ">=1.20 <1.21"), Some(true));
        assert_eq!(satisfies("1.21", ">=1.20 <1.21"), Some(false));
        assert_eq!(satisfies("1.20.4", "1.20.x"), Some(true));
        assert_eq!(satisfies("1.21", "1.20.x"), Some(false));
        assert_eq!(satisfies("0.5.13", ">=0.5.11"), Some(true));
        assert_eq!(satisfies("1.20.1", ">=1.20 || <1.19"), Some(true));
    }

    #[test]
    fn satisfies_is_unknown_for_garbage() {
        assert_eq!(satisfies("1.20.1", ">=abc"), None);
    }

    #[test]
    fn toml_reads_forge_dependencies() {
        let text = r#"
modLoader="javafml"
[[mods]]
modId="jei"
displayName="JEI"
version="15.2.0.27"

[[dependencies.jei]]
modId="minecraft"
mandatory=true
versionRange="[1.20,1.21)"
ordering="NONE"
side="BOTH"

[[dependencies.jei]]
modId="forge"
mandatory=true
versionRange="[47,)"

[[dependencies.jei]]
modId="someother"
mandatory=false
versionRange="*"
type="breaks"
"#;
        let m = toml_parse(text);
        assert_eq!(m.mod_id.as_deref(), Some("jei"));
        assert_eq!(m.display_name.as_deref(), Some("JEI"));
        assert_eq!(m.version.as_deref(), Some("15.2.0.27"));
        assert_eq!(m.deps.len(), 2);
        assert_eq!(m.deps[0].mod_id, "minecraft");
        assert_eq!(m.deps[0].version_range.as_deref(), Some("[1.20,1.21)"));
        assert_eq!(m.breaks.len(), 1);
        assert_eq!(m.breaks[0].mod_id, "someother");
        assert!(!m.breaks[0].mandatory);
    }

    #[test]
    fn platform_ids_are_not_missing_deps() {
        assert!(is_platform("minecraft"));
        assert!(is_platform("Fabric"));
        assert!(is_platform("fabric-api"));
        assert!(is_platform("neoforge"));
        assert!(!is_platform("sodium"));
    }

    #[test]
    fn detects_duplicate_and_missing() {
        let dir = std::env::temp_dir().join(format!("astral-scan-{}", crate::time::now_nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        let mk = |name: &str, body: &str| {
            let f = std::fs::File::create(dir.join(name)).unwrap();
            let mut zw = zip::ZipWriter::new(f);
            let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default();
            use std::io::Write as _;
            zw.start_file("fabric.mod.json", opts).unwrap();
            zw.write_all(body.as_bytes()).unwrap();
            zw.finish().unwrap();
        };
        mk(
            "a.jar",
            r#"{"id":"alpha","name":"Alpha","version":"1.0.0","depends":{"minecraft":">=1.20","beta":">=2.0"}}"#,
        );
        mk(
            "a-copy.jar",
            r#"{"id":"alpha","name":"Alpha","version":"1.0.0"}"#,
        );
        mk("b.jar", r#"{"id":"beta","name":"Beta","version":"1.0.0"}"#);

        let r = scan(&dir);
        assert_eq!(r.counts.total, 3);
        // 重复 id 是 error，版本不满足只是 warn（版本号写法多样，不能一棍子打死）
        assert_eq!(
            r.counts.errors, 1,
            "重复 id 应该只有 1 个 error。实际: {:?}",
            r.issues
        );
        assert!(r
            .issues
            .iter()
            .any(|i| i.code == "duplicate-id" && i.mod_id.as_deref() == Some("alpha")));
        // beta 装的是 1.0.0，alpha 要求 >=2.0 → 应该是 warn
        assert!(r.issues.iter().any(|i| i.code == "version-mismatch"
            && i.mod_id.as_deref() == Some("beta")
            && i.severity == "warn"));
        assert!(r.summary.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clean_dir_has_no_issues() {
        let dir = std::env::temp_dir().join(format!("astral-scan-{}", crate::time::now_nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = std::fs::File::create(dir.join("ok.jar")).unwrap();
        let mut zw = zip::ZipWriter::new(f);
        let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default();
        use std::io::Write as _;
        zw.start_file("fabric.mod.json", opts).unwrap();
        zw.write_all(
            br#"{"id":"solo","name":"Solo","version":"1.0.0","depends":{"minecraft":">=1.20"}}"#,
        )
        .unwrap();
        zw.finish().unwrap();
        let r = scan(&dir);
        assert!(r.issues.is_empty(), "{:?}", r.issues);
        assert!(r.summary.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
