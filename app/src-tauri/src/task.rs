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
/// 速度采样的最小间隔：太密的采样会被网络抖动带偏
const MIN_SAMPLE_MS: i64 = 200;

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
    /// 发起该任务时的参数，用于失败后重试（不对外承诺稳定结构）
    #[serde(default)]
    pub retry_spec: Option<serde_json::Value>,
    /// 稳定的启动阶段名，供启动动画使用（见 docs/v2/07 §20）
    #[serde(default)]
    pub phase: String,
    /// 阶段的中文可读名（界面不必自己维护映射表）
    #[serde(default)]
    pub stage_label: String,
    /// 最近一次测得的下载速度（字节/秒）。没在下载时是 0。
    #[serde(default)]
    pub speed_bps: u64,
    /// 按当前速度估算的剩余毫秒数；速度未知或总量未知时是 null。
    #[serde(default)]
    pub eta_ms: Option<u64>,
    /// 上一次测速的采样点，不序列化（重启后重新采样）
    #[serde(skip)]
    pub last_sample: Option<(u64, i64)>,
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
                        if let Ok(v) =
                            u64::from_str_radix(&n.chars().take(8).collect::<String>(), 16)
                        {
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
        retry_spec: None,
        phase: "queued".into(),
        stage_label: "排队中".into(),
        speed_bps: 0,
        eta_ms: None,
        last_sample: None,
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
///
/// 另外守住两条不变量：
/// 1. 终态（done / error / cancelled）一旦写入，任何迟到的进度或事件都不能改写它；
/// 2. 已请求取消（cancelling）的任务不会被旧的进度帧改回 running —— 否则界面会
///    在"正在取消"和"下载中"之间来回跳。
pub fn update(task_id: &str, f: impl FnOnce(&mut Task)) {
    let (updated, changed) = {
        let mut s = store().lock().unwrap();
        let Some(t) = s.tasks.get_mut(task_id) else {
            return;
        };
        if is_terminal(&t.state) {
            return;
        }
        let was_cancelling = t.cancelling;
        let before = t.state.clone();
        f(t);
        // 取消请求已发出时，进度更新不许把状态改回 running
        if was_cancelling && !is_terminal(&t.state) {
            t.state = "cancelling".into();
        }
        // 状态只能往前走：queued -> running -> 终态
        if before == "running" && t.state == "queued" {
            t.state = "running".into();
        }
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
        t.phase = phase_of(&t.stage).to_string();
        t.stage_label = stage_label_of(&t.stage).to_string();
    });
}

/// 便捷：改进度并带上字节数，顺带算下载速度与剩余时间。
///
/// 界面上的"3.2 MB/s，还剩 40 秒"就是从这里来的。以前 `Task.bytes` /
/// `total_bytes` 建好之后**从来没有被写过**，一直是 0 —— 前端拿到的进度
/// 只有文件条数，大文件下载时数字半天不动。
pub fn progress_bytes(
    task_id: &str,
    stage: &str,
    label: impl Into<String>,
    done: u32,
    total: u32,
    bytes: u64,
    total_bytes: u64,
) {
    update(task_id, |t| {
        t.stage = stage.to_string();
        t.label = label.into();
        t.done = done;
        t.total = total;
        t.bytes = bytes;
        t.total_bytes = total_bytes;
        t.state = "running".into();
        t.phase = phase_of(&t.stage).to_string();
        t.stage_label = stage_label_of(&t.stage).to_string();
        sample_speed(t);
    });
}

/// 按两次采样之间的增量算速度，再据此估剩余时间。
///
/// 采样间隔不到 200 ms 就沿用上次的速度 —— 太密的采样会被网络抖动带偏，
/// 界面上数字乱跳比不显示更糟。字节数变少（换文件了）就重新开始采样。
fn sample_speed(t: &mut Task) {
    let now = crate::time::now_unix_ms();
    apply_sample(t, now);
}

/// `sample_speed` 的可测试内核：时间从外面传进来。
fn apply_sample(t: &mut Task, now: i64) {
    match t.last_sample {
        Some((last_bytes, last_ms)) if t.bytes >= last_bytes => {
            let dt = now - last_ms;
            if dt >= MIN_SAMPLE_MS {
                let db = t.bytes - last_bytes;
                t.speed_bps = (db as f64 * 1000.0 / dt as f64).round() as u64;
                t.last_sample = Some((t.bytes, now));
            }
        }
        Some(_) => {
            // 换了个文件或者进度回退，重新开始
            t.speed_bps = 0;
            t.last_sample = Some((t.bytes, now));
        }
        None => {
            t.last_sample = Some((t.bytes, now));
        }
    }
    t.eta_ms = if t.speed_bps > 0 && t.total_bytes > t.bytes {
        Some(((t.total_bytes - t.bytes) as f64 * 1000.0 / t.speed_bps as f64).round() as u64)
    } else {
        None
    };
}

pub fn start(task_id: &str) {
    update(task_id, |t| {
        t.state = "running".into();
        t.stage = "resolve".into();
        t.phase = "checking-files".into();
        t.stage_label = stage_label_of(&t.stage).to_string();
        t.label = "开始".into();
    });
}

pub fn finish(task_id: &str, result: serde_json::Value) {
    update(task_id, |t| {
        t.state = "done".into();
        t.stage = "done".into();
        // 启动任务完成 = 进程已创建；其他任务完成 = 真的做完
        t.phase = if t.kind == "launch" {
            "process-created".into()
        } else {
            "done".into()
        };
        t.stage_label = stage_label_of(&t.stage).to_string();
        t.label = "完成".into();
        t.result = Some(result);
        t.ended_at = Some(now_iso());
        t.cancelling = false;
    });
    release_token(task_id);
}

pub fn fail(task_id: &str, encoded: String) {
    let err = decode(&encoded);
    update(task_id, |t| {
        t.state = "error".into();
        t.stage = "error".into();
        t.phase = "failed".into();
        t.stage_label = "失败".into();
        t.label = err.message.clone();
        t.error = Some(err);
        t.ended_at = Some(now_iso());
        t.cancelling = false;
    });
    release_token(task_id);
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
    release_token(task_id);
}

/// 任务结束后释放取消令牌，避免 `tokens` 表一直涨。
///
/// 保留一小段时间没有意义：终态任务不会再被取消。
fn release_token(task_id: &str) {
    if let Ok(mut s) = store().lock() {
        s.tokens.remove(task_id);
    }
}

/// 统计：有多少任务、多少还在跑、多少令牌。
///
/// 自检与诊断页用它确认没有泄漏。
pub fn health() -> serde_json::Value {
    let s = store().lock().unwrap();
    let running = s.tasks.values().filter(|t| !is_terminal(&t.state)).count();
    let cancelling = s.tasks.values().filter(|t| t.cancelling).count();
    serde_json::json!({
        "tasks": s.tasks.len(),
        "running": running,
        "cancelling": cancelling,
        "tokens": s.tokens.len(),
        "bootId": s.boot_id,
        "maxTasks": MAX_TASKS,
    })
}

/// worker 在阶段边界调用：已请求取消则返回错误串
pub fn check_cancel(task_id: &str) -> Result<(), String> {
    if is_cancelled(task_id) {
        Err("CANCELLED|任务已取消".into())
    } else {
        Ok(())
    }
}

/// 把内部 stage 映射成稳定 phase（供前端动画使用）
pub fn phase_of(stage: &str) -> &'static str {
    match stage {
        "validate" => "validating",
        "resolve" | "assets" | "extract" => "checking-files",
        "jvm" => "preparing-java",
        "download" => "downloading",
        "launch" => "launching",
        "spawned" => "process-created",
        "done" => "done",
        "error" => "failed",
        "cancelled" => "cancelled",
        "queued" => "queued",
        _ => "checking-files",
    }
}

/// 阶段的中文名，供界面直接显示
pub fn stage_label_of(stage: &str) -> &'static str {
    match stage {
        "queued" => "排队中",
        "validate" => "检查实例与版本",
        "resolve" => "解析版本",
        "download" => "下载文件",
        "assets" => "校验资源文件",
        "jvm" => "准备 Java",
        "extract" => "解包文件",
        "launch" => "组装启动参数",
        "spawned" => "进程已创建",
        "done" => "完成",
        "error" => "失败",
        "cancelled" => "已取消",
        _ => "处理中",
    }
}

/// 任务创建出实例后，把实例 id 绑到任务上（删除/改名保护依赖它）
pub fn bind_instance(task_id: &str, instance_id: &str) {
    let mut s = store().lock().unwrap();
    if let Some(t) = s.tasks.get_mut(task_id) {
        t.instance_id = Some(instance_id.to_string());
        persist(&s);
    }
}
/// 记录发起参数，供 retry_task 复用
pub fn set_retry_spec(task_id: &str, spec: serde_json::Value) {
    let mut s = store().lock().unwrap();
    if let Some(t) = s.tasks.get_mut(task_id) {
        t.retry_spec = Some(spec);
        persist(&s);
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

/// 是否已有同类的运行中任务（用于去重，避免重复下载）
pub fn has_running_kind(kind: &str, instance_id: Option<&str>, game_version: Option<&str>) -> bool {
    let s = store().lock().unwrap();
    s.tasks.values().any(|t| {
        t.kind == kind
            && !is_terminal(&t.state)
            && instance_id
                .map(|v| t.instance_id.as_deref() == Some(v))
                .unwrap_or(true)
            && game_version
                .map(|v| t.game_version.as_deref() == Some(v))
                .unwrap_or(true)
    })
}

pub fn has_running(instance_id: &str) -> bool {
    let s = store().lock().unwrap();
    s.tasks
        .values()
        .any(|t| t.instance_id.as_deref() == Some(instance_id) && !is_terminal(&t.state))
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

    fn blank_task() -> Task {
        Task {
            task_id: "task_test".into(),
            revision: 1,
            kind: "install".into(),
            instance_id: None,
            game_version: None,
            title: "测试".into(),
            stage: "download".into(),
            label: "下载中".into(),
            done: 0,
            total: 0,
            bytes: 0,
            total_bytes: 0,
            state: "running".into(),
            error: None,
            result: None,
            started_at: "2026-01-01T00:00:00Z".into(),
            ended_at: None,
            retry_of: None,
            boot_id: "boot".into(),
            cancelling: false,
            retry_spec: None,
            phase: "downloading".into(),
            stage_label: "下载文件".into(),
            speed_bps: 0,
            eta_ms: None,
            last_sample: None,
        }
    }

    #[test]
    fn first_sample_only_records_the_baseline() {
        let mut t = blank_task();
        t.bytes = 1000;
        t.total_bytes = 10_000;
        apply_sample(&mut t, 1_000);
        assert_eq!(t.speed_bps, 0);
        assert!(t.eta_ms.is_none());
        assert_eq!(t.last_sample, Some((1000, 1_000)));
    }

    #[test]
    fn computes_speed_and_eta_between_samples() {
        let mut t = blank_task();
        t.total_bytes = 10_000;
        t.bytes = 1_000;
        apply_sample(&mut t, 1_000);
        // 1 秒内又下了 2000 字节 → 2000 B/s，剩 7000 字节 → 3.5 秒
        t.bytes = 3_000;
        apply_sample(&mut t, 2_000);
        assert_eq!(t.speed_bps, 2_000);
        assert_eq!(t.eta_ms, Some(3_500));
    }

    #[test]
    fn keeps_last_speed_when_samples_are_too_close() {
        let mut t = blank_task();
        t.total_bytes = 100_000;
        t.bytes = 1_000;
        apply_sample(&mut t, 1_000);
        t.bytes = 2_000;
        apply_sample(&mut t, 1_100);
        assert_eq!(t.speed_bps, 0, "间隔太短不该更新速度");
        t.bytes = 5_000;
        apply_sample(&mut t, 2_000);
        assert_eq!(t.speed_bps, 4_000);
        // 再给一个 50 ms 后的采样，速度保持上一次的值，不重算
        t.bytes = 5_100;
        apply_sample(&mut t, 2_050);
        assert_eq!(t.speed_bps, 4_000);
    }

    #[test]
    fn resets_when_progress_goes_backwards() {
        let mut t = blank_task();
        t.total_bytes = 10_000;
        t.bytes = 5_000;
        apply_sample(&mut t, 1_000);
        t.bytes = 9_000;
        apply_sample(&mut t, 2_000);
        assert_eq!(t.speed_bps, 4_000);
        // 换文件了：字节数比上次小
        t.bytes = 100;
        apply_sample(&mut t, 2_100);
        assert_eq!(t.speed_bps, 0);
        assert!(t.eta_ms.is_none());
        assert_eq!(t.last_sample, Some((100, 2_100)));
    }

    #[test]
    fn no_eta_without_a_total() {
        let mut t = blank_task();
        t.total_bytes = 0;
        t.bytes = 1_000;
        apply_sample(&mut t, 1_000);
        t.bytes = 5_000;
        apply_sample(&mut t, 2_000);
        assert_eq!(t.speed_bps, 4_000);
        assert!(t.eta_ms.is_none(), "总量未知时不能瞎报剩余时间");
    }
}
