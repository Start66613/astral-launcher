//! AI 诊断（BYOK，DeepSeek）。
//!
//! 设计原则（对应路线图 §6「AI 系统设计」）：
//!
//! 1. **密钥不出本机、不进前端**。API Key 用 Windows DPAPI 加密后存在
//!    `%APPDATA%\Astral\ai.key`，绑当前用户；前端只能知道「有没有配」和末四位。
//! 2. **规则先行，AI 补长尾**。本地 `logs::diagnose` 已经跑过 12 条规则，
//!    这里把规则结果和脱敏后的日志一起给模型，让它解释而不是重新猜。
//! 3. **只发脱敏后的内容，且由玩家确认**。发送前必须走 `ai_preview`，
//!    前端把要发的东西原样展示，玩家点了才 `ai_analyze`。
//! 4. **不假装成功**。没 Key、断网、模型回的内容不是合法 JSON，都明确报错，
//!    绝不编一个「AI 说是 Mod 冲突」出来。
//! 5. **成本可控**。单次请求有字符上限、超时、`max_tokens` 上限；
//!    结果按输入哈希缓存，同一份日志不重复花钱。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 默认模型。DeepSeek 的对话补全模型名。
pub const DEFAULT_MODEL: &str = "deepseek-chat";

/// 单次请求最多带多少字符的日志（脱敏后）。超了截断并在结果里说明。
pub const MAX_PROMPT_CHARS: usize = 24_000;

/// 结果缓存最多保留多少条。
const MAX_CACHE: usize = 20;

/// 单次请求超时（秒）。
pub const TIMEOUT_SECS: u64 = 90;

/// 模型输出的 token 上限。够写一份结构化诊断，不至于失控。
pub const MAX_TOKENS: u32 = 1600;

/// 密钥与缓存的存放目录。`ASTRAL_AI_DIR` 只给自检用，正常运行读 `%APPDATA%\Astral`。
fn ai_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("ASTRAL_AI_DIR") {
        return PathBuf::from(d);
    }
    std::env::var_os("APPDATA")
        .map(|p| PathBuf::from(p).join("Astral"))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn key_path() -> PathBuf {
    ai_dir().join("ai.key")
}

fn cache_path() -> PathBuf {
    ai_dir().join("ai-cache.json")
}

// ── 密钥保管 ────────────────────────────────────────────────────

/// 密钥状态。前端只能看到这些，拿不到明文。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct KeyStatus {
    pub configured: bool,
    /// 末四位，方便玩家确认配的是哪把钥匙
    pub hint: Option<String>,
    pub model: String,
    /// 加密方式，界面上说明用
    pub storage: String,
}

/// 密钥格式校验：DeepSeek 的 key 形如 `sk-` + 32 位以上。
pub fn validate_key(key: &str) -> Result<String, String> {
    let k = key.trim();
    if k.is_empty() {
        return Err("INVALID_VALUE|API Key 不能为空".into());
    }
    if k.len() < 20 || k.len() > 200 {
        return Err("INVALID_VALUE|API Key 长度不对，请检查是否复制完整".into());
    }
    if !k.is_ascii() || k.chars().any(|c| c.is_whitespace()) {
        return Err("INVALID_VALUE|API Key 里不应该有空格或中文".into());
    }
    if !k.starts_with("sk-") {
        return Err("INVALID_VALUE|DeepSeek 的 API Key 一般以 sk- 开头，请确认没复制错".into());
    }
    Ok(k.to_string())
}

#[cfg(windows)]
mod dpapi {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        }
    }

    /// 取出 `CRYPT_INTEGER_BLOB` 里的字节并释放系统分配的内存。
    unsafe fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        if out.pbData.is_null() {
            return Vec::new();
        }
        let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        LocalFree(out.pbData as *mut core::ffi::c_void);
        v
    }

    pub fn protect(plain: &[u8]) -> Result<Vec<u8>, String> {
        let input = blob(plain);
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: 输入缓冲区在整个调用期间有效；out 由系统分配，成功时用 LocalFree 释放。
        let ok = unsafe {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        };
        if ok == 0 {
            return Err("INTERNAL|加密 API Key 失败".into());
        }
        Ok(unsafe { take(out) })
    }

    pub fn unprotect(cipher: &[u8]) -> Result<Vec<u8>, String> {
        let input = blob(cipher);
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: 同上。
        let ok = unsafe {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        };
        if ok == 0 {
            return Err("INTERNAL|解密 API Key 失败（可能换了 Windows 用户）".into());
        }
        Ok(unsafe { take(out) })
    }
}

/// 存密钥。写 `.part` 再 rename，断电不会留下半截文件。
pub fn save_key(key: &str) -> Result<KeyStatus, String> {
    let k = validate_key(key)?;
    let p = key_path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("PERMISSION|创建配置目录失败|{e}"))?;
    }
    #[cfg(windows)]
    let bytes = dpapi::protect(k.as_bytes())?;
    #[cfg(not(windows))]
    let bytes = k.as_bytes().to_vec();
    let tmp = p.with_extension("key.part");
    std::fs::write(&tmp, &bytes).map_err(|e| format!("PERMISSION|保存 API Key 失败|{e}"))?;
    std::fs::rename(&tmp, &p).map_err(|e| format!("PERMISSION|保存 API Key 失败|{e}"))?;
    Ok(status())
}

/// 读密钥。没配、解不开都返回 `None`（不 panic，不把错误抛给每次调用）。
pub fn load_key() -> Option<String> {
    let bytes = std::fs::read(key_path()).ok()?;
    #[cfg(windows)]
    let plain = dpapi::unprotect(&bytes).ok()?;
    #[cfg(not(windows))]
    let plain = bytes;
    String::from_utf8(plain)
        .ok()
        .filter(|s| !s.trim().is_empty())
}

/// 删密钥。没配过也算成功。
pub fn clear_key() -> Result<KeyStatus, String> {
    let p = key_path();
    if p.exists() {
        std::fs::remove_file(&p).map_err(|e| format!("PERMISSION|删除 API Key 失败|{e}"))?;
    }
    let _ = std::fs::remove_file(cache_path());
    Ok(status())
}

pub fn status() -> KeyStatus {
    let key = load_key();
    KeyStatus {
        configured: key.is_some(),
        hint: key.map(|k| {
            let tail: String = k
                .chars()
                .rev()
                .take(4)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            format!("…{tail}")
        }),
        model: DEFAULT_MODEL.to_string(),
        storage: if cfg!(windows) {
            "Windows DPAPI（绑当前用户，换用户或换机器都解不开）".to_string()
        } else {
            "本地文件（当前平台没有 DPAPI）".to_string()
        },
    }
}

// ── 请求准备 ────────────────────────────────────────────────────

/// 发给模型的内容预览。玩家点了「开始分析」才会真的发出去。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AiPreview {
    pub instance_id: String,
    /// 参与分析的日志名
    pub logs: Vec<String>,
    /// 本地规则已经命中的结论（一并给模型，减少幻觉）
    pub findings: Vec<String>,
    /// 真正要发送的文本（已脱敏、已裁剪）
    pub prompt: String,
    /// 发送的字符数
    pub chars: u64,
    /// 是否因为太长被截断
    pub truncated: bool,
    /// 脱敏了多少处
    pub redactions: u32,
    /// 发送前必须知道的话
    pub warnings: Vec<String>,
    pub model: String,
    /// 目标地址（写死，不接受前端传）
    pub endpoint: String,
}

pub const ENDPOINT: &str = "https://api.deepseek.com/chat/completions";

/// 生成要发送的内容。**只读**，不发网络请求。
pub fn preview(
    instance_dir: &std::path::Path,
    instance_id: &str,
    log_ids: &[String],
    max_lines: u32,
) -> Result<AiPreview, String> {
    let prepared = crate::logs::prepare(instance_dir, log_ids, max_lines)?;
    let mut text = prepared.text;
    let mut truncated = prepared.truncated;
    if text.chars().count() > MAX_PROMPT_CHARS {
        // 按字符截断，别把 UTF-8 切开
        text = text.chars().take(MAX_PROMPT_CHARS).collect();
        truncated = true;
    }
    let mut warnings = prepared.warnings.clone();
    if truncated {
        warnings.push(format!(
            "内容超过 {} 字符，只发送前半部分；更早的日志建议自己先看一眼",
            MAX_PROMPT_CHARS
        ));
    }
    if !status().configured {
        warnings.push("还没有配置 API Key，配置之后才能分析".into());
    }
    // 本地规则的结果一起给模型：让它解释，而不是从头猜
    let findings = crate::logs::classify_text(&text)
        .iter()
        .map(|f| format!("{}（{}，可信度 {}）", f.label, f.cause, f.confidence))
        .collect();
    let logs = crate::logs::list(instance_dir)
        .into_iter()
        .filter(|f| log_ids.contains(&f.id))
        .map(|f| f.name)
        .collect();
    Ok(AiPreview {
        instance_id: instance_id.to_string(),
        logs,
        findings,
        chars: text.chars().count() as u64,
        prompt: text,
        truncated,
        redactions: prepared.redactions,
        warnings,
        model: DEFAULT_MODEL.to_string(),
        endpoint: ENDPOINT.to_string(),
    })
}

/// 组装 messages。系统提示词写死在这里，不让前端改（避免被当成通用聊天接口）。
pub fn build_messages(preview: &AiPreview) -> serde_json::Value {
    let system = "你是 Minecraft 启动器里的崩溃诊断助手。玩家给了你一段已经脱敏的日志，\
以及本地规则命中的结论。请只根据日志里真实出现的内容判断，不要编造日志里没有的 Mod、\
类名或参数。如果证据不足，就明说证据不足，不要猜。\
\n\n只输出一个 JSON 对象，不要 markdown 代码块，不要多余文字。格式：\
\n{\"summary\":\"一句话结论\",\"confidence\":\"high|medium|low\",\
\"causes\":[{\"title\":\"可能原因\",\"detail\":\"为什么这么判断，引用日志里的关键行\",\
\"confidence\":\"high|medium|low\"}],\
\"steps\":[{\"title\":\"要做的事\",\"detail\":\"具体怎么操作\"}],\
\"need_more\":\"还需要什么信息才能更确定，没有就写空字符串\"}\
\n\n最多 3 条 causes、4 条 steps。用简体中文。";

    let mut user = String::new();
    if !preview.findings.is_empty() {
        user.push_str("本地规则已经命中的结论：\n");
        for f in &preview.findings {
            user.push_str("- ");
            user.push_str(f);
            user.push('\n');
        }
        user.push('\n');
    }
    if preview.truncated {
        user.push_str("（注意：日志只给了后半部分）\n\n");
    }
    user.push_str("日志内容：\n");
    user.push_str(&preview.prompt);

    serde_json::json!([
        { "role": "system", "content": system },
        { "role": "user", "content": user }
    ])
}

// ── 结果 ────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AiCause {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub confidence: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AiStep {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub detail: String,
}

/// 模型返回的结构化结果。字段缺失时留空，不 panic。
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct AiAnalysis {
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub confidence: String,
    #[serde(default)]
    pub causes: Vec<AiCause>,
    #[serde(default)]
    pub steps: Vec<AiStep>,
    #[serde(default)]
    pub need_more: String,
}

/// 一次分析的完整结果（含元信息，让玩家知道这次花了什么）。
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AiResult {
    pub analysis: AiAnalysis,
    pub model: String,
    /// network | cache
    pub source: String,
    /// 本地规则命中的条数，和 AI 的结论分开展示
    pub rule_findings: u32,
    pub prompt_chars: u64,
    pub truncated: bool,
    pub redactions: u32,
    /// 本次用了多少 token（模型返回的 usage，拿不到就是 null）
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub elapsed_ms: u64,
    /// 生成时间
    pub at: String,
}

/// 从模型返回的文本里抠出 JSON 对象。模型偶尔会套 ```json 围栏。
pub fn extract_json(text: &str) -> Result<AiAnalysis, String> {
    let t = text.trim();
    let body = if let Some(rest) = t.strip_prefix("```") {
        // 去掉 ```json 和结尾的 ```
        let rest = rest.strip_prefix("json").unwrap_or(rest);
        let rest = rest.trim_start_matches(['\r', '\n']);
        rest.strip_suffix("```").unwrap_or(rest).trim()
    } else {
        t
    };
    // 万一前后还有废话，取第一个 { 到最后一个 }
    let start = body.find('{');
    let end = body.rfind('}');
    let slice = match (start, end) {
        (Some(a), Some(b)) if b > a => &body[a..=b],
        _ => body,
    };
    let v: serde_json::Value =
        serde_json::from_str(slice).map_err(|e| format!("INTERNAL|模型返回的不是合法 JSON|{e}"))?;
    let a: AiAnalysis =
        serde_json::from_value(v).map_err(|e| format!("INTERNAL|模型返回的字段对不上|{e}"))?;
    if a.summary.trim().is_empty() && a.causes.is_empty() && a.steps.is_empty() {
        return Err("INTERNAL|模型没有给出有效结论，请稍后再试".into());
    }
    Ok(a)
}

// ── 缓存 ────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone)]
struct CacheEntry {
    hash: String,
    at: String,
    result: AiResult,
}

fn prompt_hash(p: &AiPreview) -> String {
    use sha1::{Digest, Sha1};
    let mut h = Sha1::new();
    h.update(p.prompt.as_bytes());
    h.update(b"|");
    h.update(DEFAULT_MODEL.as_bytes());
    h.update(b"|v1");
    let d = h.finalize();
    d.iter().map(|b| format!("{b:02x}")).collect()
}

fn load_cache() -> Vec<CacheEntry> {
    std::fs::read_to_string(cache_path())
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<CacheEntry>>(&s).ok())
        .unwrap_or_default()
}

fn save_cache(list: &[CacheEntry]) {
    let text = match serde_json::to_string(list) {
        Ok(t) => t,
        Err(_) => return,
    };
    let p = cache_path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = p.with_extension("json.part");
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, &p);
    }
}

fn cache_get(hash: &str) -> Option<AiResult> {
    load_cache()
        .into_iter()
        .find(|e| e.hash == hash)
        .map(|e| AiResult {
            source: "cache".into(),
            elapsed_ms: 0,
            ..e.result
        })
}

fn cache_put(hash: &str, result: &AiResult) {
    let mut list = load_cache();
    list.retain(|e| e.hash != hash);
    list.insert(
        0,
        CacheEntry {
            hash: hash.to_string(),
            at: result.at.clone(),
            result: AiResult {
                source: "network".into(),
                ..result.clone()
            },
        },
    );
    list.truncate(MAX_CACHE);
    save_cache(&list);
}

/// 清掉分析缓存。返回清掉几条。
pub fn clear_cache() -> u64 {
    let n = load_cache().len() as u64;
    let _ = std::fs::remove_file(cache_path());
    n
}

// ── 调用 ────────────────────────────────────────────────────────

/// 真正发请求。`base` 可覆盖（自检指向本地测试服务器），默认官方地址。
fn call_api(
    base: &str,
    key: &str,
    messages: &serde_json::Value,
) -> Result<(String, Option<u64>, Option<u64>), String> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
        .user_agent("Astral/0.1.0")
        .build()
        .map_err(|e| format!("INTERNAL|创建 HTTP 客户端失败|{e}"))?;
    let body = serde_json::json!({
        "model": DEFAULT_MODEL,
        "messages": messages,
        "max_tokens": MAX_TOKENS,
        "temperature": 0.2,
        "stream": false,
        "response_format": { "type": "json_object" }
    });
    // base 既可以只给域名（自动补 /chat/completions），也可以直接给完整地址
    // （自检要带 ?broken=1 这类查询参数时用）
    let url = if base.contains("/chat/completions") {
        base.to_string()
    } else {
        format!("{base}/chat/completions")
    };
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("INTERNAL|创建运行时失败|{e}"))?;
    let resp = rt.block_on(async {
        client
            .post(&url)
            .header("Authorization", format!("Bearer {key}"))
            .json(&body)
            .send()
            .await
    });
    let resp = match resp {
        Ok(r) => r,
        Err(e) if e.is_timeout() => {
            return Err(format!("NETWORK|AI 服务超时（{} 秒）|{e}", TIMEOUT_SECS))
        }
        Err(e) => return Err(format!("NETWORK|连不上 AI 服务|{e}")),
    };
    let status = resp.status();
    let text = rt
        .block_on(async { resp.text().await })
        .map_err(|e| format!("NETWORK|读取 AI 响应失败|{e}"))?;
    if !status.is_success() {
        // 401/402/429 是玩家最可能撞上的三种，直接说人话
        let msg = match status.as_u16() {
            401 => "API Key 不对或已失效，请重新填一个".to_string(),
            402 => "DeepSeek 账户余额不足，请先充值".to_string(),
            429 => "请求太频繁，等一会儿再试".to_string(),
            500..=599 => "DeepSeek 服务暂时不可用，稍后再试".to_string(),
            _ => format!("AI 服务返回 {status}"),
        };
        let brief: String = text.chars().take(300).collect();
        return Err(format!("NETWORK|{msg}|{brief}"));
    }
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("INTERNAL|AI 响应不是合法 JSON|{e}"))?;
    let content = v
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .ok_or_else(|| "INTERNAL|AI 响应里没有内容".to_string())?;
    let usage = v.get("usage");
    let pt = usage
        .and_then(|u| u.get("prompt_tokens"))
        .and_then(|n| n.as_u64());
    let ct = usage
        .and_then(|u| u.get("completion_tokens"))
        .and_then(|n| n.as_u64());
    Ok((content.to_string(), pt, ct))
}

/// 分析。`base` 为空时用官方地址。
pub fn analyze(
    instance_dir: &std::path::Path,
    instance_id: &str,
    log_ids: &[String],
    max_lines: u32,
    base: Option<&str>,
) -> Result<AiResult, String> {
    analyze_with_key(instance_dir, instance_id, log_ids, max_lines, base, None)
}

/// 带显式密钥的分析。`key` 为空时读本地保存的那把。
///
/// `base` 与 `key` 只在自检里传，线上命令永远走 `analyze`（密钥从 DPAPI 读）。
pub fn analyze_with_key(
    instance_dir: &std::path::Path,
    instance_id: &str,
    log_ids: &[String],
    max_lines: u32,
    base: Option<&str>,
    key: Option<&str>,
) -> Result<AiResult, String> {
    let key = match key {
        Some(k) => validate_key(k)?,
        None => load_key().ok_or_else(|| {
            "AUTH_REQUIRED|还没有配置 DeepSeek API Key|在「设置 → AI 诊断」里填一个".to_string()
        })?,
    };
    let p = preview(instance_dir, instance_id, log_ids, max_lines)?;
    let hash = prompt_hash(&p);
    if let Some(hit) = cache_get(&hash) {
        return Ok(hit);
    }
    let messages = build_messages(&p);
    let started = crate::time::now_unix_ms();
    let api_base = base.unwrap_or_else(|| ENDPOINT.trim_end_matches("/chat/completions"));
    let (content, pt, ct) = call_api(api_base, &key, &messages)?;
    let analysis = extract_json(&content)?;
    let elapsed = crate::time::now_unix_ms().saturating_sub(started).max(0) as u64;
    let result = AiResult {
        analysis,
        model: DEFAULT_MODEL.to_string(),
        source: "network".to_string(),
        rule_findings: p.findings.len() as u32,
        prompt_chars: p.chars,
        truncated: p.truncated,
        redactions: p.redactions,
        prompt_tokens: pt,
        completion_tokens: ct,
        elapsed_ms: elapsed,
        at: crate::time::now_iso(),
    };
    cache_put(&hash, &result);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_bad_keys() {
        assert!(validate_key("").is_err());
        assert!(validate_key("short").is_err());
        assert!(validate_key("sk-带中文的钥匙").is_err());
        assert!(validate_key("pk-0123456789012345678901").is_err());
        assert!(validate_key("sk-0123456789012345678901").is_ok());
    }

    #[test]
    fn parses_plain_json() {
        let a =
            extract_json(r#"{"summary":"内存不足","confidence":"high","causes":[],"steps":[]}"#)
                .unwrap();
        assert_eq!(a.summary, "内存不足");
        assert_eq!(a.confidence, "high");
    }

    #[test]
    fn parses_fenced_json() {
        let text = "```json\n{\"summary\":\"Mod 冲突\",\"causes\":[{\"title\":\"重复\",\"detail\":\"两个 sodium\"}],\"steps\":[{\"title\":\"删一个\",\"detail\":\"去 mods 里删\"}]}\n```";
        let a = extract_json(text).unwrap();
        assert_eq!(a.summary, "Mod 冲突");
        assert_eq!(a.causes.len(), 1);
        assert_eq!(a.steps.len(), 1);
    }

    #[test]
    fn tolerates_chatter_around_json() {
        let a = extract_json("好的，这是结果：\n{\"summary\":\"Java 版本不对\"}\n以上。").unwrap();
        assert_eq!(a.summary, "Java 版本不对");
    }

    #[test]
    fn rejects_non_json_and_empty() {
        assert!(extract_json("我觉得是内存不够").is_err());
        assert!(extract_json(r#"{"summary":"  ","causes":[],"steps":[]}"#).is_err());
    }

    #[test]
    fn builds_messages_with_findings() {
        let p = AiPreview {
            instance_id: "i".into(),
            logs: vec!["latest.log".into()],
            findings: vec!["内存不足（堆溢出）".into()],
            prompt: "java.lang.OutOfMemoryError".into(),
            chars: 25,
            truncated: true,
            redactions: 2,
            warnings: vec![],
            model: DEFAULT_MODEL.into(),
            endpoint: ENDPOINT.into(),
        };
        let m = build_messages(&p);
        let arr = m.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["role"], "system");
        let user = arr[1]["content"].as_str().unwrap();
        assert!(user.contains("本地规则已经命中的结论"));
        assert!(user.contains("内存不足"));
        assert!(user.contains("只给了后半部分"));
        assert!(user.contains("OutOfMemoryError"));
    }

    #[test]
    fn hashes_differ_for_different_prompts() {
        let mk = |text: &str| AiPreview {
            instance_id: "i".into(),
            logs: vec![],
            findings: vec![],
            prompt: text.into(),
            chars: text.len() as u64,
            truncated: false,
            redactions: 0,
            warnings: vec![],
            model: DEFAULT_MODEL.into(),
            endpoint: ENDPOINT.into(),
        };
        assert_ne!(prompt_hash(&mk("a")), prompt_hash(&mk("b")));
        assert_eq!(prompt_hash(&mk("a")), prompt_hash(&mk("a")));
    }
}
