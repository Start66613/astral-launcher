//! 日志读取与诊断预览（契约 V2 §10）。
//!
//! 只读本地日志，不联网、不上传。`prepare_diagnosis` 只做**脱敏预览**，
//! 真正的上传必须由用户在界面上确认后才发生。

use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LogFile {
    /// 稳定 id，由相对路径的哈希派生
    pub id: String,
    pub name: String,
    /// game | crash | launcher
    pub kind: String,
    pub size_bytes: u64,
    pub modified_at: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LogText {
    pub id: String,
    pub name: String,
    pub text: String,
    pub lines: u32,
    /// 是否因为 limit 只返回了尾部
    pub truncated: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosisPreview {
    pub text: String,
    pub redactions: u32,
    pub bytes: u64,
    pub truncated: bool,
    pub warnings: Vec<String>,
}

fn id_of(rel: &str) -> String {
    use sha1::{Digest, Sha1};
    let mut h = Sha1::new();
    h.update(rel.as_bytes());
    let digest = h.finalize();
    let hex: String = digest.iter().take(4).map(|b| format!("{b:02x}")).collect();
    format!("log_{hex}")
}

fn mtime_iso(p: &Path) -> String {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .map(crate::time::system_time_to_iso)
        .unwrap_or_else(|_| crate::time::now_iso())
}

fn collect(dir: &Path, kind: &str, rel_prefix: &str, out: &mut Vec<(LogFile, PathBuf)>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if !p.is_file() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        let lower = name.to_lowercase();
        if !(lower.ends_with(".log") || lower.ends_with(".txt") || lower.ends_with(".gz")) {
            continue;
        }
        // .gz 不能直接按文本读，暂不列出
        if lower.ends_with(".gz") {
            continue;
        }
        let rel = format!("{rel_prefix}{name}");
        let size = e.metadata().map(|m| m.len()).unwrap_or(0);
        out.push((
            LogFile {
                id: id_of(&rel),
                name,
                kind: kind.to_string(),
                size_bytes: size,
                modified_at: mtime_iso(&p),
            },
            p,
        ));
    }
}

/// 列出实例可读的日志：logs/*.log 与 crash-reports/*.txt
pub fn list(instance_dir: &Path) -> Vec<LogFile> {
    let mut entries: Vec<(LogFile, PathBuf)> = Vec::new();
    collect(&instance_dir.join("logs"), "game", "logs/", &mut entries);
    collect(
        &instance_dir.join("crash-reports"),
        "crash",
        "crash-reports/",
        &mut entries,
    );
    entries.sort_by(|a, b| b.0.modified_at.cmp(&a.0.modified_at));
    entries.into_iter().map(|(f, _)| f).collect()
}

fn resolve(instance_dir: &Path, id: &str) -> Option<PathBuf> {
    let mut entries: Vec<(LogFile, PathBuf)> = Vec::new();
    collect(&instance_dir.join("logs"), "game", "logs/", &mut entries);
    collect(
        &instance_dir.join("crash-reports"),
        "crash",
        "crash-reports/",
        &mut entries,
    );
    entries
        .into_iter()
        .find(|(f, _)| f.id == id)
        .map(|(_, p)| p)
}

/// 读日志尾部；`limit` 为行数上限（默认 400，最大 2000）
pub fn read(instance_dir: &Path, id: &str, limit: Option<u32>) -> Result<LogText, String> {
    let path = resolve(instance_dir, id).ok_or_else(|| "INVALID_PATH|日志不存在".to_string())?;
    let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    // 超大日志只读尾部 4 MiB，避免一次读进内存
    const MAX_READ: u64 = 4 * 1024 * 1024;
    let text = if bytes > MAX_READ {
        use std::io::{Read, Seek, SeekFrom};
        let mut f = std::fs::File::open(&path).map_err(|e| format!("PERMISSION|读取失败|{e}"))?;
        f.seek(SeekFrom::Start(bytes - MAX_READ))
            .map_err(|e| format!("PERMISSION|定位失败|{e}"))?;
        let mut buf = String::new();
        f.read_to_string(&mut buf)
            .map_err(|e| format!("PERMISSION|读取失败|{e}"))?;
        buf
    } else {
        std::fs::read_to_string(&path).map_err(|e| format!("PERMISSION|读取失败|{e}"))?
    };

    let limit = limit.unwrap_or(400).clamp(1, 2000) as usize;
    let all: Vec<&str> = text.lines().collect();
    let truncated = all.len() > limit;
    let tail: Vec<&str> = if truncated {
        all[all.len() - limit..].to_vec()
    } else {
        all.clone()
    };

    Ok(LogText {
        id: id.to_string(),
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        text: tail.join("\n"),
        lines: tail.len() as u32,
        truncated,
    })
}

/// 本地脱敏：token、邮箱、IPv4、Windows 用户目录
pub fn redact(input: &str) -> (String, u32) {
    let mut count = 0u32;
    let mut out = String::with_capacity(input.len());

    for line in input.lines() {
        let mut l = line.to_string();
        let lower = l.to_lowercase();

        // 常见的凭证关键字：整行只保留键名
        for key in [
            "access_token",
            "accessToken",
            "refresh_token",
            "authorization",
            "client_secret",
            "sessiontoken",
            "xuid",
        ] {
            if lower.contains(&key.to_lowercase()) {
                if let Some(pos) = l.to_lowercase().find(&key.to_lowercase()) {
                    let head = &l[..pos];
                    if !head.contains('[') {
                        l = format!("{head}{key}=<redacted>");
                        count += 1;
                    }
                }
                break;
            }
        }

        // 邮箱
        if let Some(at) = l.find('@') {
            let bytes = l.as_bytes();
            let mut start = at;
            while start > 0 && !bytes[start - 1].is_ascii_whitespace() {
                start -= 1;
            }
            let mut end = at + 1;
            while end < bytes.len() && !bytes[end].is_ascii_whitespace() {
                end += 1;
            }
            if end > at + 1 && l[start..end].contains('.') {
                l.replace_range(start..end, "<email>");
                count += 1;
            }
        }

        // Windows 用户目录
        if let Some(pos) = l.find("\\Users\\") {
            let rest = &l[pos + 7..];
            if let Some(end) = rest.find('\\') {
                if end > 0 {
                    let name = rest[..end].to_string();
                    if name != "<user>" {
                        l = l.replacen(&name, "<user>", 1);
                        count += 1;
                    }
                }
            }
        }

        // IPv4：先收集区间，再倒序替换，避免边借边改
        let mut ip_ranges: Vec<(usize, usize)> = Vec::new();
        {
            let bytes = l.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                if bytes[i].is_ascii_digit() {
                    let start = i;
                    let mut dots = 0;
                    let mut j = i;
                    while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b'.') {
                        if bytes[j] == b'.' {
                            dots += 1;
                        }
                        j += 1;
                    }
                    if dots == 3 && j - start >= 7 {
                        ip_ranges.push((start, j));
                    }
                    i = j;
                } else {
                    i += 1;
                }
            }
        }
        for (start, end) in ip_ranges.into_iter().rev() {
            l.replace_range(start..end, "<ip>");
            count += 1;
        }

        out.push_str(&l);
        out.push('\n');
    }

    (out, count)
}

/// 诊断预览：读日志尾部 + 脱敏，不上传
pub fn prepare(instance_dir: &Path, log_ids: &[String], max_lines: u32) -> Result<DiagnosisPreview, String> {
    if log_ids.is_empty() {
        return Err("INVALID_VALUE|至少选择一个日志".into());
    }
    let mut merged = String::new();
    for id in log_ids.iter().take(5) {
        let t = read(instance_dir, id, Some(max_lines))?;
        merged.push_str(&format!("===== {} =====\n{}\n", t.name, t.text));
    }
    let (text, redactions) = redact(&merged);
    let truncated = text.lines().count() as u32 >= max_lines.saturating_mul(log_ids.len().min(5) as u32);
    let mut warnings = Vec::new();
    if redactions > 0 {
        warnings.push(format!("已在本机脱敏 {redactions} 处敏感信息"));
    }
    if text.len() > 200_000 {
        warnings.push("内容较多，建议只上传选中片段".into());
    }
    Ok(DiagnosisPreview {
        bytes: text.len() as u64,
        text,
        redactions,
        truncated,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_email_and_ip() {
        let (out, n) = redact("contact me at steve@example.com from 192.168.1.20");
        assert!(out.contains("<email>"));
        assert!(out.contains("<ip>"));
        assert!(n >= 2);
    }

    #[test]
    fn redacts_token_line() {
        let (out, _) = redact("Authorization: Bearer abcdef123456");
        assert!(out.contains("<redacted>"));
    }

    #[test]
    fn redacts_user_dir() {
        let (out, _) = redact("C:\\Users\\alice\\AppData\\Roaming\\.minecraft");
        assert!(out.contains("<user>"));
    }
}
