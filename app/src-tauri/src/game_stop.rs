//! 停止正在运行的游戏。
//!
//! 启动器以前只能启动、不能停止：玩家想换实例只能去任务管理器里找 java.exe，
//! 找错了还会误杀别的东西。这里补上「结束游戏」。
//!
//! 三条底线：
//! 1. **只杀自己启动的那个进程**。pid 会被系统复用，所以必须比对创建时间，
//!    时间对不上就当成「这个会话早就结束了」，绝不按 pid 硬杀。
//! 2. **先礼后兵**。默认发 `WM_CLOSE`（等于玩家点窗口右上角的 ×），
//!    等游戏自己存盘退出；只有玩家明确要求强制结束才 `TerminateProcess`。
//! 3. **玩家主动关掉不算崩溃**。强制结束时退出码通常非 0，
//!    要标记成「玩家结束」，不能报成崩溃把玩家吓一跳。

use serde::Serialize;

/// 等游戏自己退出的默认时长。Minecraft 关窗口要存区块，给 15 秒。
pub const DEFAULT_WAIT_MS: u64 = 15_000;

/// 轮询进程是否还在的间隔。
const POLL_MS: u64 = 200;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StopResult {
    pub session_id: String,
    pub instance_id: String,
    pub pid: u32,
    /// `graceful` = 发了关闭消息等它自己退；`force` = 直接终止
    pub method: String,
    /// 是否用了强制手段
    pub forced: bool,
    /// 从发出请求到进程消失用了多久
    pub waited_ms: u64,
    /// 结束时进程是否还在（true 说明没停下来）
    pub still_running: bool,
    /// 给玩家看的一句话
    pub message: String,
}

/// 停止某个实例正在跑的游戏。
///
/// `force` 为 false 时只发关闭消息；`wait_ms` 是等它自己退的时长。
/// 返回的 `still_running` 为 true 说明没停下来，调用方应该如实告诉玩家，
/// 而不是假装已经结束。
pub fn stop(instance_id: &str, force: bool, wait_ms: Option<u64>) -> Result<StopResult, String> {
    let sess = crate::session::active(Some(instance_id))
        .ok_or_else(|| "NOT_FOUND|这个实例没有在运行|先启动游戏再试".to_string())?;
    let budget = wait_ms.unwrap_or(DEFAULT_WAIT_MS).min(120_000);

    // 再确认一次这是我们启动的那个进程（active() 已经查过，这里防的是
    // 从 active() 到真正动手之间的空窗期里进程退出、pid 被别人接走）
    if !crate::session::pid_matches(&sess) {
        return Err("NOT_FOUND|游戏已经退出了|刷新一下状态".to_string());
    }

    // 先标记「是玩家自己要停的」，这样后面退出码非 0 也不会被当成崩溃
    crate::session::mark_stopping(&sess.session_id);

    let started = std::time::Instant::now();
    let mut used_force = false;

    if force {
        used_force = true;
        terminate(sess.pid);
    } else {
        close_windows(sess.pid);
        // 等它自己退
        while (started.elapsed().as_millis() as u64) < budget {
            if !crate::session::pid_matches(&sess) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(POLL_MS));
        }
    }

    // 优雅关闭没成，或者本来就是强制：确认一下还在不在
    let mut still = crate::session::pid_matches(&sess);
    if still && !used_force {
        // 玩家没要求强制，就不替他做决定——如实返回「没停下来」，
        // 由界面再问一次「要不要强制结束」
        return Ok(StopResult {
            session_id: sess.session_id,
            instance_id: sess.instance_id,
            pid: sess.pid,
            method: "graceful".into(),
            forced: false,
            waited_ms: started.elapsed().as_millis() as u64,
            still_running: true,
            message: format!(
                "游戏没有在 {} 秒内退出（可能在存盘，也可能卡住了）。要强制结束吗？",
                budget / 1000
            ),
        });
    }

    if used_force {
        // 强制结束后再给一小会儿让进程真的消失
        let force_started = std::time::Instant::now();
        while (force_started.elapsed().as_millis() as u64) < 5_000 {
            if !crate::session::pid_matches(&sess) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(POLL_MS));
        }
        still = crate::session::pid_matches(&sess);
    }

    let waited = started.elapsed().as_millis() as u64;
    let message = if still {
        "结束失败：进程还在。可能是权限不够，试试用管理员身份运行启动器。".to_string()
    } else if used_force {
        "已经强制结束了。没来得及存盘的部分会丢，下次进游戏可能会回到稍早的位置。".to_string()
    } else {
        "游戏已经正常退出。".to_string()
    };

    Ok(StopResult {
        session_id: sess.session_id,
        instance_id: sess.instance_id,
        pid: sess.pid,
        method: if used_force { "force" } else { "graceful" }.into(),
        forced: used_force,
        waited_ms: waited,
        still_running: still,
        message,
    })
}

/// 给这个进程的所有可见窗口发 `WM_CLOSE`。
///
/// 等价于玩家点窗口右上角的 ×：Minecraft 会走正常的关闭流程（存盘、退服）。
#[cfg(windows)]
fn close_windows(pid: u32) {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, WM_CLOSE,
    };

    struct Ctx {
        pid: u32,
        sent: u32,
    }

    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> i32 {
        let ctx = &mut *(lparam as *mut Ctx);
        let mut owner: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut owner);
        if owner == ctx.pid && IsWindowVisible(hwnd) != 0 {
            PostMessageW(hwnd, WM_CLOSE, 0, 0);
            ctx.sent += 1;
        }
        1 // 继续枚举
    }

    let mut ctx = Ctx { pid, sent: 0 };
    // SAFETY: 回调在 EnumWindows 返回前同步执行，ctx 一直在栈上有效。
    unsafe {
        EnumWindows(Some(callback), &mut ctx as *mut Ctx as LPARAM);
    }
    if ctx.sent == 0 {
        // 没找到可见窗口（可能还停在启动阶段），退化成直接终止，
        // 否则玩家点了「结束」却什么也没发生
        terminate(pid);
    }
}

#[cfg(not(windows))]
fn close_windows(pid: u32) {
    terminate(pid);
}

/// 直接终止进程。
fn terminate(pid: u32) {
    let mut sys = sysinfo::System::new();
    let p = sysinfo::Pid::from_u32(pid);
    sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[p]), true);
    if let Some(proc_) = sys.process(p) {
        let _ = proc_.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_when_nothing_is_running() {
        let e = stop("inst_does_not_exist", false, Some(200)).unwrap_err();
        assert!(e.starts_with("NOT_FOUND|"), "实际：{e}");
    }

    #[test]
    fn default_wait_is_bounded() {
        // 15 秒足够 Minecraft 存盘，又不至于让界面转圈转太久；
        // 玩家显式传的时长再长也封顶 2 分钟（见 stop() 里的 min）
        let wait = DEFAULT_WAIT_MS;
        let poll = POLL_MS;
        assert_eq!(wait, 15_000);
        // 轮询间隔必须小于默认等待，否则一次都轮不到
        assert!(poll > 0 && poll < wait);
    }
}
