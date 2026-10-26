//! 游戏会话：真实运行状态（契约 v3 §2.3 / §4.3 / §6.2）。
//!
//! `Task(done)` 只代表进程已创建；「还在不在跑」一律以 session 为准，
//! 启动器重启后按 pid 对账。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter};

pub const EVENT: &str = "astral://game-exit";

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GameSession {
    pub session_id: String,
    pub instance_id: String,
    pub task_id: String,
    pub pid: u32,
    /// 进程创建时间（Unix 秒）。用来识别 PID 复用：
    /// 老会话的 pid 被系统分配给了别的进程时，创建时间对不上就不是同一个进程。
    #[serde(default)]
    pub pid_started_at: Option<u64>,
    pub started_at: String,
    pub state: String, // starting | running | exited
    pub exit_code: Option<i32>,
    pub ended_at: Option<String>,
    pub duration_ms: Option<u64>,
    /// 启动器重启后按 pid 对账收尾的会话：退出码与时长都不精确
    #[serde(default)]
    pub reconciled: bool,
}

static STORE: OnceLock<Mutex<Vec<GameSession>>> = OnceLock::new();
static SEQ: Mutex<u64> = Mutex::new(0);

fn store() -> &'static Mutex<Vec<GameSession>> {
    STORE.get_or_init(|| Mutex::new(Vec::new()))
}

fn file() -> PathBuf {
    // 测试/隔离运行可覆盖数据目录
    if let Some(dir) = std::env::var_os("ASTRAL_DATA_DIR") {
        return PathBuf::from(dir).join("sessions.json");
    }
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Astral")
        .join("sessions.json")
}

/// 自检专用：直接把会话列表写进存储，用于构造 PID 复用等边界场景。
#[doc(hidden)]
pub fn save_for_test(list: Vec<GameSession>) {
    let mut s = store().lock().unwrap();
    *s = list;
    persist(&s);
}

fn persist(list: &[GameSession]) {
    let p = file();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tail: Vec<&GameSession> = list.iter().rev().take(500).collect();
    if let Ok(text) = serde_json::to_string_pretty(&tail) {
        let _ = std::fs::write(p, text);
    }
}

pub fn load() {
    if let Ok(text) = std::fs::read_to_string(file()) {
        if let Ok(mut loaded) = serde_json::from_str::<Vec<GameSession>>(&text) {
            loaded.reverse();
            *store().lock().unwrap() = loaded;
        }
    }
}

/// 读某个 pid 的创建时间（Unix 秒）。进程不存在返回 None。
fn pid_start_time(pid: u32) -> Option<u64> {
    if pid == 0 {
        return None;
    }
    let mut sys = sysinfo::System::new();
    let p = sysinfo::Pid::from_u32(pid);
    sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[p]), true);
    sys.process(p).map(|proc| proc.start_time())
}

/// 进程是否还是**同一个**进程。
///
/// 只比 pid 不够：Windows 会复用 pid，关掉游戏后同号 pid 可能是别的程序。
/// 老记录没有创建时间时退化成只看 pid 是否存在（兼容旧数据）。
fn pid_alive(pid: u32, expected_start: Option<u64>) -> bool {
    // PID 0 是系统空闲进程，绝不是游戏进程
    if pid == 0 {
        return false;
    }
    match pid_start_time(pid) {
        Some(start) => match expected_start {
            // 允许 2 秒误差：取创建时间的时机与系统记录可能差一点点
            Some(want) => start.abs_diff(want) <= 2,
            None => true,
        },
        None => false,
    }
}

pub fn begin(instance_id: &str, task_id: &str, pid: u32) -> GameSession {
    let s = {
        let mut seq = SEQ.lock().unwrap();
        *seq += 1;
        GameSession {
            // 跨进程唯一：时间戳 + 序号（重启后序号归零，时间戳保证不撞）
            session_id: format!("sess_{:x}_{:x}", crate::time::now_unix_ms(), *seq),
            instance_id: instance_id.to_string(),
            task_id: task_id.to_string(),
            pid,
            pid_started_at: pid_start_time(pid),
            started_at: crate::task::now_iso(),
            state: "running".into(),
            exit_code: None,
            ended_at: None,
            duration_ms: None,
            reconciled: false,
        }
    };
    let mut list = store().lock().unwrap();
    list.push(s.clone());
    persist(&list);
    s
}

fn duration_ms(started_at: &str) -> u64 {
    crate::time::iso_to_unix_ms(started_at)
        .map(|t| (crate::time::now_unix_ms() - t).max(0) as u64)
        .unwrap_or(0)
}

/// 标记退出并推事件
pub fn mark_exited(session_id: &str, exit_code: Option<i32>) -> Option<GameSession> {
    let updated = {
        let mut list = store().lock().unwrap();
        let idx = list.iter().position(|s| s.session_id == session_id)?;
        let s = &mut list[idx];
        if s.state == "exited" {
            return Some(s.clone());
        }
        s.state = "exited".into();
        s.exit_code = exit_code;
        s.ended_at = Some(crate::task::now_iso());
        s.duration_ms = Some(duration_ms(&s.started_at));
        let snap = s.clone();
        persist(&list);
        snap
    };
    Some(updated)
}

pub fn emit_exit(app: &AppHandle, s: &GameSession) {
    // 退出码未知 ≠ 崩溃：只有拿到非 0 退出码才判定崩溃
    let exit_known = s.exit_code.is_some();
    let crashed = matches!(s.exit_code, Some(c) if c != 0);
    let payload = serde_json::json!({
        "sessionId": s.session_id,
        "taskId": s.task_id,
        "instanceId": s.instance_id,
        "exitCode": s.exit_code,
        "exitKnown": exit_known,
        "crashed": crashed,
        "reconciled": s.reconciled,
        "durationMs": s.duration_ms.unwrap_or(0),
        "endedAt": s.ended_at,
    });
    let _ = app.emit(EVENT, payload);
}

/// 当前**存活**会话；期间做一次 pid 对账
///
/// 进程已退出的会话会被就地收尾并跳过——不能把它当成"还在运行"返回，
/// 否则 start_launch / delete_instance 会误报 INSTANCE_BUSY。
pub fn active(instance_id: Option<&str>) -> Option<GameSession> {
    let mut list = store().lock().unwrap();
    let mut changed = false;
    for s in list.iter_mut().rev() {
        if s.state == "exited" {
            continue;
        }
        if let Some(want) = instance_id {
            if s.instance_id != want {
                continue;
            }
        }
        if !pid_alive(s.pid, s.pid_started_at) {
            // 进程已退出：收尾这条会话，但**不能**把它当作"还在运行"返回，
            // 否则 start_launch / delete_instance 会误报 INSTANCE_BUSY
            s.state = "exited".into();
            s.ended_at = Some(crate::task::now_iso());
            s.duration_ms = Some(duration_ms(&s.started_at));
            changed = true;
            continue;
        }
        return Some(s.clone());
    }
    if changed {
        persist(&list);
    }
    None
}

pub fn list(instance_id: Option<&str>, limit: usize) -> Vec<GameSession> {
    let list = store().lock().unwrap();
    list.iter()
        .rev()
        .filter(|s| instance_id.map(|v| s.instance_id == v).unwrap_or(true))
        .take(limit)
        .cloned()
        .collect()
}

/// 每个实例的统计（最近一次 + 累计时长）
pub fn stats_for(instance_id: &str) -> crate::instance::PlayStats {
    let list = store().lock().unwrap();
    let mut last: Option<String> = None;
    let mut total: u64 = 0;
    for s in list.iter() {
        if s.instance_id != instance_id {
            continue;
        }
        if s.state == "exited" {
            total += s.duration_ms.unwrap_or(0);
            if last.as_ref().map(|l| s.started_at > *l).unwrap_or(true) {
                last = Some(s.started_at.clone());
            }
        }
    }
    crate::instance::PlayStats {
        last_played: last,
        total_playtime_ms: total,
    }
}

/// 启动后把上次没写完结的 session 收尾
pub fn reconcile() {
    let mut list = store().lock().unwrap();
    let mut changed = false;
    for s in list.iter_mut() {
        if s.state != "exited" && !pid_alive(s.pid, s.pid_started_at) {
            s.state = "exited".into();
            s.exit_code = None;
            s.reconciled = true;
            s.ended_at = Some(crate::task::now_iso());
            s.duration_ms = Some(duration_ms(&s.started_at));
            changed = true;
        }
    }
    if changed {
        persist(&list);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_from_iso_is_positive() {
        let d = duration_ms("2026-01-01T00:00:00Z");
        assert!(d > 0);
    }

    #[test]
    fn stats_accumulate_exited_sessions() {
        let id = format!("inst_stats_{}", std::process::id());
        let s = begin(&id, "task_x", 0);
        mark_exited(&s.session_id, Some(0));
        let st = stats_for(&id);
        assert!(st.last_played.is_some());
    }
}
