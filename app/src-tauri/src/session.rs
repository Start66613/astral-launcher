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
    pub started_at: String,
    pub state: String, // starting | running | exited
    pub exit_code: Option<i32>,
    pub ended_at: Option<String>,
    pub duration_ms: Option<u64>,
}

static STORE: OnceLock<Mutex<Vec<GameSession>>> = OnceLock::new();
static SEQ: Mutex<u64> = Mutex::new(0);

fn store() -> &'static Mutex<Vec<GameSession>> {
    STORE.get_or_init(|| Mutex::new(Vec::new()))
}

fn file() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Astral")
        .join("sessions.json")
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

fn pid_alive(pid: u32) -> bool {
    let mut sys = sysinfo::System::new();
    let p = sysinfo::Pid::from_u32(pid);
    sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[p]), true);
    sys.process(p).is_some()
}

pub fn begin(instance_id: &str, task_id: &str, pid: u32) -> GameSession {
    let s = {
        let mut seq = SEQ.lock().unwrap();
        *seq += 1;
        GameSession {
            session_id: format!("sess_{:x}", *seq),
            instance_id: instance_id.to_string(),
            task_id: task_id.to_string(),
            pid,
            started_at: crate::task::now_iso(),
            state: "running".into(),
            exit_code: None,
            ended_at: None,
            duration_ms: None,
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
    let crashed = s.exit_code.map(|c| c != 0).unwrap_or(true);
    let payload = serde_json::json!({
        "sessionId": s.session_id,
        "taskId": s.task_id,
        "instanceId": s.instance_id,
        "exitCode": s.exit_code,
        "crashed": crashed,
        "durationMs": s.duration_ms.unwrap_or(0),
        "endedAt": s.ended_at,
    });
    let _ = app.emit(EVENT, payload);
}

/// 当前会话；期间做一次 pid 对账
pub fn active(instance_id: Option<&str>) -> Option<GameSession> {
    let mut list = store().lock().unwrap();
    for s in list.iter_mut().rev() {
        if s.state == "exited" {
            continue;
        }
        if let Some(want) = instance_id {
            if s.instance_id != want {
                continue;
            }
        }
        if !pid_alive(s.pid) {
            s.state = "exited".into();
            s.ended_at = Some(crate::task::now_iso());
            s.duration_ms = Some(duration_ms(&s.started_at));
            let snap = s.clone();
            persist(&list);
            return Some(snap);
        }
        return Some(s.clone());
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
        if s.state != "exited" && !pid_alive(s.pid) {
            s.state = "exited".into();
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
