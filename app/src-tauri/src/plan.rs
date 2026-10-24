//! 游戏安装计划（契约 V2 §2.2）。
//!
//! 两段式：`prepare` 只做校验和计算，**不建目录、不下载**；`start` 才执行。
//! 计划 15 分钟过期，执行时校验 revision，过期/变更都要重新规划。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LoaderSelection {
    pub kind: String,
    pub version: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GameInstallRequest {
    pub name: String,
    pub game_version: String,
    #[serde(default)]
    pub loader: Option<LoaderSelection>,
    #[serde(default)]
    pub memory_mb: Option<u32>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PlanTarget {
    pub proposed_instance_id: String,
    pub display_path: String,
    pub isolated: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PlannedFile {
    pub id: String,
    pub display_name: String,
    pub size_bytes: Option<u64>,
    pub role: String,
    pub action: String,
    pub relative_path: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PlanIssue {
    pub code: String,
    pub message: String,
    pub blocking: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GameInstallPlan {
    pub plan_id: String,
    pub revision: u32,
    pub expires_at: String,
    pub request: GameInstallRequest,
    pub target: PlanTarget,
    pub files: Vec<PlannedFile>,
    pub issues: Vec<PlanIssue>,
    /// 未知总量返回 null，不是 0
    pub download_bytes: Option<u64>,
}

pub struct StoredPlan {
    pub plan: GameInstallPlan,
    pub created_ms: i64,
}

static PLANS: Mutex<Option<HashMap<String, StoredPlan>>> = Mutex::new(None);

const TTL_MS: i64 = 15 * 60 * 1000;

fn store() -> std::sync::MutexGuard<'static, Option<HashMap<String, StoredPlan>>> {
    PLANS.lock().unwrap()
}

fn issue(code: &str, message: impl Into<String>, blocking: bool) -> PlanIssue {
    PlanIssue {
        code: code.to_string(),
        message: message.into(),
        blocking,
    }
}

/// 只做校验与规划，不产生任何副作用
pub fn prepare(game: &std::path::Path, request: GameInstallRequest) -> Result<GameInstallPlan, String> {
    let mut issues = Vec::new();

    if let Err(e) = crate::instance::validate_name(&request.name) {
        issues.push(issue("INVALID_NAME", crate::task::decode(&e).message, true));
    }

    let installed = crate::core::version_installed(game, &request.game_version);
    if !installed {
        // 本地没有：确认官方清单里存在（网络失败按阻塞处理，不猜）
        match crate::core::list_versions("official") {
            Ok(list) => {
                if !list.iter().any(|v| v.id == request.game_version) {
                    issues.push(issue(
                        "VERSION_NOT_FOUND",
                        format!("官方清单里没有 {}", request.game_version),
                        true,
                    ));
                }
            }
            Err(e) => issues.push(issue("NETWORK", crate::task::decode(&e).message, true)),
        }
    }

    if let Some(loader) = &request.loader {
        match loader.kind.as_str() {
            "fabric" | "quilt" => {}
            "forge" | "neoforge" => issues.push(issue(
                "UNSUPPORTED",
                format!("{} 的安装尚未实现，只能查询版本", loader.kind),
                true,
            )),
            other => issues.push(issue("UNSUPPORTED", format!("未知加载器 {other}"), true)),
        }
    }

    if let Some(mb) = request.memory_mb {
        if !(512..=32768).contains(&mb) {
            issues.push(issue("INVALID_VALUE", "内存需在 512–32768 MB 之间", true));
        }
    }

    let mut files = Vec::new();
    if !installed {
        files.push(PlannedFile {
            id: "client".into(),
            display_name: format!("Minecraft {}", request.game_version),
            size_bytes: None,
            role: "required".into(),
            action: "add".into(),
            relative_path: format!("versions/{}", request.game_version),
        });
    }
    if let Some(loader) = &request.loader {
        files.push(PlannedFile {
            id: format!("loader-{}", loader.kind),
            display_name: format!("{} {}", loader.kind, loader.version),
            size_bytes: None,
            role: "required".into(),
            action: "add".into(),
            relative_path: "versions/".into(),
        });
    }

    let id = crate::instance::new_id();
    let plan = GameInstallPlan {
        plan_id: format!("plan_{:x}", crate::time::now_unix_ms()),
        revision: 1,
        expires_at: crate::time::unix_ms_to_iso(crate::time::now_unix_ms() + TTL_MS),
        target: PlanTarget {
            display_path: crate::instance::instance_dir(game, &id)
                .to_string_lossy()
                .to_string(),
            proposed_instance_id: id,
            isolated: true,
        },
        request,
        files,
        issues,
        download_bytes: None, // 明细要等版本元数据解析，先如实返回 null
    };

    let mut s = store();
    let map = s.get_or_insert_with(HashMap::new);
    // 顺手清理过期计划，避免无限增长
    let now = crate::time::now_unix_ms();
    map.retain(|_, p| now - p.created_ms < TTL_MS);
    map.insert(
        plan.plan_id.clone(),
        StoredPlan {
            plan: plan.clone(),
            created_ms: now,
        },
    );

    Ok(plan)
}

/// 取走计划并校验（一次性）
pub fn take(plan_id: &str, revision: u32) -> Result<GameInstallPlan, String> {
    let mut s = store();
    let map = s.as_mut().ok_or_else(|| "PLAN_EXPIRED|没有可用计划".to_string())?;
    let stored = map
        .remove(plan_id)
        .ok_or_else(|| "PLAN_EXPIRED|计划不存在或已使用".to_string())?;

    let now = crate::time::now_unix_ms();
    if now - stored.created_ms >= TTL_MS {
        return Err("PLAN_EXPIRED|计划已过期，请重新规划".into());
    }
    if stored.plan.revision != revision {
        return Err("PLAN_EXPIRED|计划已变更，请重新规划".into());
    }
    Ok(stored.plan)
}

pub fn has_blocking(plan: &GameInstallPlan) -> Option<&PlanIssue> {
    plan.issues.iter().find(|i| i.blocking)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_helper() {
        let i = issue("X", "msg", true);
        assert!(i.blocking);
    }
}
