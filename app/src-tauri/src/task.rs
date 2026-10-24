//! 任务注册表：`start_*` 立即返回 Task，状态变化推 `astral://task`。
//!
//! 契约 V2（相对 v3 的增量）：
//! - `bootId`：进程启动标识，用来识别"上次没结束"的任务；
//! - `cancelling`：已请求取消、等 worker 停止；
//! - 取消令牌：worker 在阶段边界检查，真停下才置 `cancelled`；
//! - 持久化：任务落 `%APPDATA%\Astral\tasks.json`，重启后把上次的
//!   running/queued 一律改成 `error` + `INTERRUPTED`。

use crate::error::ApiError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::{AppHandle, Emitter};

pub const EVENT: &str = "astral://task";
const MAX_TASKS: usize = 50;

#[derive(Serialize, Deserialize, Clone, Debug)]
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
    /// 本任务所属进程启动标识
    #[serde(default)]
    pub boot_id: String,
    /// 已请求取消，等待 worker 停止
    #[serde(default)]
    pub cancelling: bool,
}

pub fn now_iso() -> String {
    crate::time::now_iso()
}

struct Store {
    app: Option<AppHandle>,
    tasks: HashMap<String, Task>,
    seq: u64,
    boot_id: String,
    tokens: HashMap<String, Arc<AtomicBool>>,
}

static STORE: OnceLock<Mutex<Store>> = OnceLock::new();

fn store() -> &'static Mutex<Store> {
    STORE.get_or_init(|| {
        Mutex::new(Store {
            app: None,
            tasks: HashMap::new(),
            seq: 0,
            boot_id: String::new(),
            tokens: HashMap::new(),
        })
    })
}

fn tasks_file() -> Option<PathBuf> {
    // 测试与隔离运行可覆盖数据目录，避免写真实用户配置
    if let Some(dir) = std::env::var_os("ASTRAL_DATA_DIR") {
        return Some(PathBuf::from(dir).join("tasks.json"));
    }
    let base = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(base).join("Astral").join("tasks.json"))
}

fn persist_tasks(tasks: &HashMap<String, Task>) {
    let Some(path) = tasks_file() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut list: Vec<&Task> = tasks.values().collect();
    list.sort_by(|a, b| a.started_at.cmp(&b.started_at));
    if let Ok(json) = serde_json::to_string_pretty(&list) {
        let tmp = path.with_extension("json.part");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

fn persist(s: &Store) {
    persist_tasks(&s.tasks);
}

/// 启动：生成 bootId，载入上次任务，把未结束的标记为中断
pub fn boot() -> String {
    let boot_id = format!("boot_{:x}", crate::time::now_unix_ms());
    let mut s = store().lock().unwrap();
    s.boot_id = boot_id.clone();

    if let Some(path) = tasks_file() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(list) = serde_json::from_str::<Vec<Task>>(&text) {
                for mut t in list {
                    if t.state == "running" || t.state == "queued" || t.cancelling {
                        t.state = "error".into();
                        t.stage = "error".into();
                        t.label = "上次未结束".into();
                        t.cancelling = false;
                        t.error = Some(ApiError {
                            code: "INTERRUPTED".into(),
                            message: "启动器上次退出时该任务还没结束".into(),
                            detail: Some(format!("bootId={}", t.boot_id)),
                        });
                        t.ended_at = Some(now_iso());
                        t.revision += 1;
                    }
                    if let Some(n) = t.task_id.strip_prefix("task_") {
                        if let Ok(v) = u64::from_str_radix(&n.chars().take(8).collect::<String>(), 16) {
                            s.seq = s.seq.max(v);
                        }
                    }
                    s.tasks.insert(t.task_id.clone(), t);
                }
                trim(&mut s);
                persist(&s);
            }
        }
    }
    boot_id
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
    // id 带时间戳，跨进程也不重复
    let task = Task {
        task_id: format!("task_{:x}_{:x}", crate::time::now_unix_ms(), s.seq),
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
        boot_id: s.boot_id.clone(),
        cancelling: false,
    };
    s.tasks.insert(task.task_id.clone(), task.clone());
    s.tokens
        .insert(task.task_id.clone(), Arc::new(AtomicBool::new(false)));
    trim(&mut s);
    persist(&s);
    drop(s);
    emit(&task);
    task
}

fn trim(s: &mut Store) {
    if s.tasks.len() <= MAX_TASKS {
        return;
    }
    let mut finished: Vec<(String, String)> = s
        .tasks
        .values()
        .filter(|t| t.ended_at.is_some())
        .map(|t| (t.started_at.clone(), t.task_id.clone()))
        .collect();
    finished.sort();
    for (_, id) in finished
        .into_iter()
        .take(s.tasks.len().saturating_sub(MAX_TASKS))
    {
        s.tasks.remove(&id);
        s.tokens.remove(&id);
    }
}

/// 终态粘滞：done / error / cancelled 之后不再接受任何修改
fn is_terminal(state: &str) -> bool {
    matches!(state, "done" | "error" | "cancelled")
}

/// 改状态：revision +1 并推帧；终态后忽略（防止迟到的进度覆盖结果）
pub fn update(task_id: &str, f: impl FnOnce(&mut Task)) {
    let (updated, changed) = {
        let mut s = store().lock().unwrap();
        let Some(t) = s.tasks.get_mut(task_id) else {
            return;
        };
        if is_terminal(&t.state) {
            return;
        }
        f(t);
        t.revision += 1;
        let changed = t.state != "running";
        let snap = t.clone();
        if changed {
            persist(&s);
        }
        (snap, changed)
    };
    let _ = changed;
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
        t.cancelling = false;
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
        t.cancelling = false;
    });
}

/// 请求取消：置标记并通知 worker；worker 确认停止后调用 [`cancelled`]。
///
/// 返回 true 表示请求已受理。没有注册令牌的任务（历史任务）直接置终态。
pub fn cancel_task(task_id: &str) -> bool {
    // 先取令牌，避免与 tasks 的可变借用冲突
    let token = {
        let s = store().lock().unwrap();
        s.tokens.get(task_id).cloned()
    };

    let (accepted, snapshot) = {
        let mut s = store().lock().unwrap();
        let Some(t) = s.tasks.get_mut(task_id) else {
            return false;
        };
        if is_terminal(&t.state) || t.cancelling {
            return false;
        }
        t.cancelling = true;
        t.label = "正在取消…".into();
        t.revision += 1;

        if let Some(tok) = token {
            tok.store(true, Ordering::SeqCst);
        } else {
            // 没有令牌（例如重启前留下的），直接收尾
            t.state = "cancelled".into();
            t.stage = "cancelled".into();
            t.label = "已取消".into();
            t.ended_at = Some(now_iso());
            t.cancelling = false;
            t.revision += 1;
        }

        let snap = t.clone();
        persist(&s);
        (true, snap)
    };
    emit(&snapshot);
    accepted
}

/// worker 侧：是否已请求取消
pub fn is_cancelled(task_id: &str) -> bool {
    let s = store().lock().unwrap();
    s.tokens
        .get(task_id)
        .map(|t| t.load(Ordering::SeqCst))
        .unwrap_or(false)
}

/// worker 确认真的停了：置终态
pub fn cancelled(task_id: &str) {
    update(task_id, |t| {
        t.state = "cancelled".into();
        t.stage = "cancelled".into();
        t.label = "已取消".into();
        t.ended_at = Some(now_iso());
        t.cancelling = false;
    });
}

/// worker 在阶段边界调用：已请求取消则返回错误串
pub fn check_cancel(task_id: &str) -> Result<(), String> {
    if is_cancelled(task_id) {
        Err("CANCELLED|任务已取消".into())
    } else {
        Ok(())
    }
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
        t.instance_id.as_deref() == Some(instance_id) && !is_terminal(&t.state)
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
}
