//! 已安装资源的账本（`mods/.astral-ledger.json`）。
//!
//! 起因：`deps::scan_installed` 只能从 jar 里读到**人类可读的版本号**
//! （`1.0.0`、`mc1.20.1-0.5.13`），读不到 Modrinth 的 version id。
//! 没有 version id，依赖解析就分不清"已装的是不是这个依赖要的版本"，
//! 只能把已装的 Mod 全部当成 `installed_unknown` 跳过——能用，但会漏掉
//! "装的是旧版、依赖要新版"这种情况。
//!
//! 这份账本记的是"我们装过什么"：project id → version id + 落盘文件名。
//! 只有 Astral 自己装的才会记；玩家手动丢进来的 jar 依然走 `unknown`，
//! 这样不会把玩家自己放的文件当成"我们保证过的版本"。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const LEDGER_FILE: &str = ".astral-ledger.json";

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct LedgerEntry {
    pub project_id: String,
    pub version_id: String,
    /// 人类可读版本号，只用来显示
    #[serde(default)]
    pub version_number: String,
    pub file_name: String,
    /// 装的时间（ISO 8601），只用来显示
    #[serde(default)]
    pub installed_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Ledger {
    /// 落盘文件名 → 条目。用文件名做键，因为玩家会手动改名/禁用。
    #[serde(default)]
    pub files: BTreeMap<String, LedgerEntry>,
}

fn ledger_path(dir: &Path) -> PathBuf {
    dir.join(LEDGER_FILE)
}

pub fn load(dir: &Path) -> Ledger {
    let p = ledger_path(dir);
    let Ok(text) = std::fs::read_to_string(&p) else {
        return Ledger::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

fn save(dir: &Path, ledger: &Ledger) -> Result<(), String> {
    let p = ledger_path(dir);
    let text =
        serde_json::to_string_pretty(ledger).map_err(|e| format!("INTERNAL|账本序列化失败|{e}"))?;
    let tmp = p.with_extension("json.part");
    std::fs::write(&tmp, text).map_err(|e| format!("PERMISSION|写账本失败|{e}"))?;
    std::fs::rename(&tmp, &p).map_err(|e| format!("PERMISSION|提交账本失败|{e}"))
}

/// 记一条。同一个 project id 的旧记录会被新记录顶掉
/// （否则升级之后账本里会留着两个版本）。
pub fn record(
    dir: &Path,
    project_id: &str,
    version_id: &str,
    version_number: &str,
    file_name: &str,
) -> Result<(), String> {
    if project_id.is_empty() || version_id.is_empty() || file_name.is_empty() {
        return Err("INVALID_VALUE|账本条目的 project/version/file 都不能为空".into());
    }
    let mut ledger = load(dir);
    ledger
        .files
        .retain(|_, e| e.project_id != project_id || e.file_name == file_name);
    ledger.files.insert(
        file_name.to_string(),
        LedgerEntry {
            project_id: project_id.to_string(),
            version_id: version_id.to_string(),
            version_number: version_number.to_string(),
            file_name: file_name.to_string(),
            installed_at: crate::task::now_iso(),
        },
    );
    save(dir, &ledger)
}

/// 删掉一条（按文件名）。删文件之后调用。
pub fn forget(dir: &Path, file_name: &str) {
    let mut ledger = load(dir);
    if ledger.files.remove(file_name).is_some() {
        let _ = save(dir, &ledger);
    }
}

/// 改名时同步账本（启用/禁用会在 `.jar` 与 `.jar.disabled` 之间改文件名）。
pub fn rename(dir: &Path, from: &str, to: &str) {
    let mut ledger = load(dir);
    if let Some(mut entry) = ledger.files.remove(from) {
        entry.file_name = to.to_string();
        ledger.files.insert(to.to_string(), entry);
        let _ = save(dir, &ledger);
    }
}

/// 只返回**文件还在**的条目。玩家手动删了 jar，账本里就不该再算它已装。
pub fn live(dir: &Path) -> Vec<LedgerEntry> {
    let ledger = load(dir);
    ledger
        .files
        .into_iter()
        .filter(|(name, _)| dir.join(name).is_file())
        .map(|(_, e)| e)
        .collect()
}

/// 给 `deps::scan_installed` 用的形态：project_id → version_id
pub fn known_versions(dir: &Path) -> std::collections::HashMap<String, String> {
    live(dir)
        .into_iter()
        .map(|e| (e.project_id, e.version_id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("astral-ledger-{}-{tag}", crate::time::now_nanos()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn records_and_reads_back() {
        let d = tmp("basic");
        record(&d, "sodium", "ver123", "0.5.13", "sodium-0.5.13.jar").unwrap();
        let before = live(&d);
        assert_eq!(before.len(), 0, "文件还没落地时不该算 live");
        // 落地之后才算
        std::fs::write(d.join("sodium-0.5.13.jar"), b"x").unwrap();
        let after = live(&d);
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].project_id, "sodium");
        assert_eq!(after[0].version_id, "ver123");
        let known = known_versions(&d);
        assert_eq!(known.get("sodium").map(String::as_str), Some("ver123"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn upgrade_replaces_old_entry() {
        let d = tmp("upgrade");
        std::fs::write(d.join("sodium-old.jar"), b"x").unwrap();
        record(&d, "sodium", "v1", "0.5.12", "sodium-old.jar").unwrap();
        std::fs::write(d.join("sodium-new.jar"), b"y").unwrap();
        record(&d, "sodium", "v2", "0.5.13", "sodium-new.jar").unwrap();
        let known = known_versions(&d);
        assert_eq!(known.len(), 1, "同一个 project 只该留一条");
        assert_eq!(known.get("sodium").map(String::as_str), Some("v2"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn forget_and_rename_follow_files() {
        let d = tmp("rename");
        std::fs::write(d.join("a.jar"), b"x").unwrap();
        record(&d, "alpha", "v1", "1.0", "a.jar").unwrap();
        std::fs::rename(d.join("a.jar"), d.join("a.jar.disabled")).unwrap();
        rename(&d, "a.jar", "a.jar.disabled");
        let after = live(&d);
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].file_name, "a.jar.disabled");
        forget(&d, "a.jar.disabled");
        assert!(live(&d).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn missing_file_is_not_counted() {
        let d = tmp("missing");
        record(&d, "ghost", "v9", "9.9", "ghost.jar").unwrap();
        assert!(live(&d).is_empty(), "文件不在就不能算已装");
        assert!(known_versions(&d).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn rejects_empty_fields() {
        let d = tmp("empty");
        assert!(record(&d, "", "v1", "1.0", "a.jar").is_err());
        assert!(record(&d, "p", "", "1.0", "a.jar").is_err());
        assert!(record(&d, "p", "v1", "1.0", "").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
