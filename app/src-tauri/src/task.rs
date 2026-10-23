//! 任务注册表：`start_*` 立即返回 Task，状态变化推 `astral://task`。
//!
//! 要点（契约 v3 §2.2）：
//! - `revision` 单调递增，前端按它取大合并；
//! - 每帧都是完整快照，不做增量；
//! - 终态保留，可被 `get_tasks` 补拉。

use crate::error::ApiError;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter};

pub const EVENT: &str = "astral://task";

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub task_id: String,
    pub revision: u32,
    pub kind: String,
    pub instance_id: Option<String>,
    pub game_version: Option<String>,
    pub title: String,
    pub stage: String,
    pub label: String,
    pub done: u32,
    pub total: u32,
    pub bytes: u64,
    pub total_bytes: u64,
    pub state: String,
    pub error: Option<ApiError>,
    pub result: Option<serde_json::Value>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub retry_of: Option<String>,
}

pub fn now_iso() -> String {
    crate::time::now_iso()
}

struct Store {
    app: Option<AppHandle>,
    tasks: HashMap<String, Task>,
    seq: u64,
}

static STORE: OnceLock<Mutex<Store>> = OnceLock::new();

fn store() -> &'static Mutex<Store> {
    STORE.get_or_init(|| {
        Mutex::new(Store {
            app: None,
            tasks: HashMap::new(),
            seq: 0,
        })
    })
}

pub fn init(app: AppHandle) {
    store().lock().unwrap().app = Some(app);
}

fn emit(task: &Task) {
    let app = store().lock().unwrap().app.clone();
    if let Some(app) = app {
        let _ = app.emit(EVENT, task);
    }
}

/// 建任务并立刻推一帧
pub fn create(
    kind: &str,
    instance_id: Option<String>,
    game_version: Option<String>,
    title: impl Into<String>,
) -> Task {
    let mut s = store().lock().unwrap();
    s.seq += 1;
    let task = Task {
        task_id: format!("task_{:x}", s.seq),
        revision: 1,
        kind: kind.to_string(),
        instance_id,
        game_version,
        title: title.into(),
        stage: "queued".into(),
        label: "排队中".into(),
        done: 0,
        total: 0,
        bytes: 0,
        total_bytes: 0,
        state: "queued".into(),
        error: None,
        result: None,
        started_at: now_iso(),
        ended_at: None,
        retry_of: None,
    };
    s.tasks.insert(task.task_id.clone(), task.clone());
    trim(&mut s);
    drop(s);
    emit(&task);
    task
}

fn trim(s: &mut Store) {
    if s.tasks.len() <= 50 {
        return;
    }
    let mut finished: Vec<(String, String)> = s
        .tasks
        .values()
        .filter(|t| t.ended_at.is_some())
        .map(|t| (t.started_at.clone(), t.task_id.clone()))
        .collect();
    finished.sort();
    for (_, id) in finished.into_iter().take(s.tasks.len().saturating_sub(50)) {
        s.tasks.remove(&id);
    }
}

/// 改状态：revision +1 并推帧
pub fn update(task_id: &str, f: impl FnOnce(&mut Task)) {
    let updated = {
        let mut s = store().lock().unwrap();
        let Some(t) = s.tasks.get_mut(task_id) else {
            return;
        };
        f(t);
        t.revision += 1;
        t.clone()
    };
    emit(&updated);
}

/// 便捷：只改进度
pub fn progress(task_id: &str, stage: &str, label: impl Into<String>, done: u32, total: u32) {
    update(task_id, |t| {
        t.stage = stage.to_string();
        t.label = label.into();
        t.done = done;
        t.total = total;
        t.state = "running".into();
    });
}

pub fn start(task_id: &str) {
    update(task_id, |t| {
        t.state = "running".into();
        t.stage = "resolve".into();
        t.label = "开始".into();
    });
}

pub fn finish(task_id: &str, result: serde_json::Value) {
    update(task_id, |t| {
        t.state = "done".into();
        t.stage = "done".into();
        t.label = "完成".into();
        t.result = Some(result);
        t.ended_at = Some(now_iso());
    });
}

pub fn fail(task_id: &str, encoded: String) {
    let err = decode(&encoded);
    update(task_id, |t| {
        t.state = "error".into();
        t.stage = "error".into();
        t.label = err.message.clone();
        t.error = Some(err);
        t.ended_at = Some(now_iso());
    });
}

pub fn cancel_task(task_id: &str) -> bool {
    let mut s = store().lock().unwrap();
    let Some(t) = s.tasks.get_mut(task_id) else {
        return false;
    };
    if t.state == "done" || t.state == "error" || t.state == "cancelled" {
        return false;
    }
    t.state = "cancelled".into();
    t.label = "已取消".into();
    t.ended_at = Some(now_iso());
    t.revision += 1;
    let snapshot = t.clone();
    drop(s);
    emit(&snapshot);
    true
}

/// `CODE|msg|detail` → ApiError
pub fn decode(encoded: &str) -> ApiError {
    let mut it = encoded.splitn(3, '|');
    let code = it.next().unwrap_or("INTERNAL").to_string();
    let message = it.next().unwrap_or("未知错误").to_string();
    let detail = it.next().map(|s| s.to_string());
    ApiError {
        code,
        message,
        detail,
    }
}

pub fn get(task_id: &str) -> Option<Task> {
    store().lock().unwrap().tasks.get(task_id).cloned()
}

pub fn snapshot(state: Option<&str>, kind: Option<&str>, instance: Option<&str>) -> Vec<Task> {
    let s = store().lock().unwrap();
    let mut list: Vec<Task> = s
        .tasks
        .values()
        .filter(|t| state.map(|v| t.state == v).unwrap_or(true))
        .filter(|t| kind.map(|v| t.kind == v).unwrap_or(true))
        .filter(|t| {
            instance
                .map(|v| t.instance_id.as_deref() == Some(v))
                .unwrap_or(true)
        })
        .cloned()
        .collect();
    list.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    list
}

pub fn has_running(instance_id: &str) -> bool {
    let s = store().lock().unwrap();
    s.tasks.values().any(|t| {
        t.instance_id.as_deref() == Some(instance_id)
            && !matches!(t.state.as_str(), "done" | "error" | "cancelled")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_roundtrip() {
        let e = decode("NETWORK|下载失败|io");
        assert_eq!(e.code, "NETWORK");
        assert_eq!(e.message, "下载失败");
        assert_eq!(e.detail.as_deref(), Some("io"));
    }

    #[test]
    fn decode_without_detail() {
        let e = decode("BUSY|忙");
        assert_eq!(e.code, "BUSY");
        assert!(e.detail.is_none());
    }

    #[test]
    fn revision_is_monotonic() {
        let t = create("install", None, Some("1.20.1".into()), "安装 1.20.1");
        let r1 = t.revision;
        update(&t.task_id, |t| t.label = "下载中".into());
        let r2 = get(&t.task_id).unwrap().revision;
        assert_eq!(r2, r1 + 1);
    }
}
