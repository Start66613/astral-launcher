//! 启动前预检：一次问清楚"这个实例现在能不能启动，不能的话卡在哪"。
//!
//! 后端本来就有这些判断，但散在 `start_launch` 里，失败时只报第一个错。
//! 玩家看到的是"实例正在运行"，退出游戏再点，又变成"版本文件缺失"，
//! 一次只能修一个。这个模块把它们一次全跑一遍，**只读、不修、不启动**。
//!
//! 两条原则：
//!
//! 1. **不合成一个布尔**。每一项单独给出 ok / 为什么不行 / 会不会挡住启动。
//! 2. **不替玩家做主**。内存偏小、Mod 冲突这类问题只提示，不拦。

use std::path::Path;

use serde::Serialize;

use crate::instance::PlayStats;

/// 实例内存的合理下限（MB）。低于这个值原版都可能 OOM。
pub const MIN_MEMORY_MB: u32 = 1024;

/// 建议值的上限（MB）。再往上加，GC 停顿反而更长，普通整合包也用不到。
pub const MAX_MEMORY_MB: u32 = 8192;

/// 单项检查的结果。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PreflightCheck {
    /// 稳定标识，前端可以按它决定点进去跳哪一页
    pub key: String,
    /// 给玩家看的中文名
    pub label: String,
    pub ok: bool,
    /// 不通过时是否挡住启动
    pub blocking: bool,
    /// 通过也可能带说明（比如"自动选择了 Java 17"）
    pub detail: Option<String>,
}

/// 内存建议。不是"最佳值"，是"这个机器上说得通的区间"。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MemoryAdvice {
    pub system_total_mb: u64,
    pub system_free_mb: u64,
    /// 实例当前设置（null = 用默认）
    pub instance_mb: Option<u32>,
    pub recommended_mb: u32,
    pub min_mb: u32,
    pub max_mb: u32,
    /// 当前设置是否落在区间里
    pub ok: bool,
    /// 为什么是这个数，直接显示给玩家
    pub note: String,
}

/// 预检总结果。
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Preflight {
    pub instance_id: String,
    pub instance_name: String,
    pub game_version: String,
    /// 没有任何 blocking 项
    pub ready: bool,
    pub checks: Vec<PreflightCheck>,
    pub memory: MemoryAdvice,
    /// 挡住启动的项，按出现顺序
    pub blockers: Vec<String>,
    pub collected_at: String,
}

fn item(
    key: &str,
    label: &str,
    blocking: bool,
    ok: bool,
    detail: Option<String>,
) -> PreflightCheck {
    PreflightCheck {
        key: key.to_string(),
        label: label.to_string(),
        ok,
        blocking,
        detail,
    }
}

/// 按系统内存、加载器、Mod 数量给一个说得通的内存值。
///
/// 规则写死在这里而不是拍脑袋给个 4096：
///
/// - 原版起步 2 GB；
/// - 有加载器再加 1 GB（加载器本身要占一份）；
/// - 每 25 个 Mod 加 512 MB，最多加 2 GB（100 个 Mod 封顶）；
/// - 上限取"系统内存的一半"和"系统内存减 2 GB"里更小的那个 ——
///   给 Java 太多，Windows 自己就没内存了；再夹一次 8192 MB 的绝对上限；
/// - 结果按 512 MB 取整，并夹在 [1024, 上限] 之间。
pub fn memory_advice(
    system_total_mb: u64,
    system_free_mb: u64,
    instance_mb: Option<u32>,
    loader: bool,
    mods: u32,
) -> MemoryAdvice {
    let min_mb = MIN_MEMORY_MB;
    let mut max_mb_u: u64 = (system_total_mb / 2)
        .min(system_total_mb.saturating_sub(2048))
        .max(min_mb as u64);
    max_mb_u = (max_mb_u / 512).max(2) * 512;
    max_mb_u = max_mb_u.min(MAX_MEMORY_MB as u64);
    let max_mb = max_mb_u as u32;

    let mut want: u64 = 2048;
    if loader {
        want += 1024;
    }
    let mod_bonus = ((mods / 25) as u64 * 512).min(2048);
    want += mod_bonus;

    let recommended_u = (want / 512).max(2) * 512;
    let recommended = recommended_u.clamp(min_mb as u64, max_mb as u64) as u32;

    let (ok, note) = match instance_mb {
        None => (
            true,
            format!(
                "没设过内存，启动时用默认值。这台机器建议 {recommended} MB（{}）。",
                memory_reason(loader, mods, mod_bonus)
            ),
        ),
        Some(mb) if mb < min_mb => (
            false,
            format!("{mb} MB 太少了，原版都可能内存不足。建议改成 {recommended} MB。"),
        ),
        Some(mb) if mb > max_mb => (
            false,
            format!(
                "{mb} MB 超过了这台机器适合的上限（{max_mb} MB）。\
                 给 Java 太多，系统自己会开始用虚拟内存，反而更卡。建议改成 {recommended} MB。"
            ),
        ),
        Some(mb) => (
            true,
            format!(
                "当前 {mb} MB，建议 {recommended} MB（{}）。",
                memory_reason(loader, mods, mod_bonus)
            ),
        ),
    };

    MemoryAdvice {
        system_total_mb,
        system_free_mb,
        instance_mb,
        recommended_mb: recommended,
        min_mb,
        max_mb,
        ok,
        note,
    }
}

fn memory_reason(loader: bool, mods: u32, mod_bonus: u64) -> String {
    let mut parts = vec!["原版 2 GB".to_string()];
    if loader {
        parts.push("加载器 +1 GB".into());
    }
    if mod_bonus > 0 {
        parts.push(format!("{mods} 个 Mod +{} MB", mod_bonus));
    }
    parts.join("，")
}

fn total_memory_mb() -> (u64, u64) {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    let to_mb = |b: u64| b / (1024 * 1024);
    (to_mb(sys.total_memory()), to_mb(sys.available_memory()))
}

/// 跑一遍全部检查。只读，不改任何文件，不启动进程。
pub fn check(
    game: &Path,
    instance_id: &str,
    stats: &dyn Fn(&str) -> PlayStats,
) -> Result<Preflight, String> {
    let inst = crate::instance::find(game, instance_id, stats)
        .ok_or_else(|| "INSTANCE_NOT_FOUND|实例不存在".to_string())?;
    let mut checks: Vec<PreflightCheck> = Vec::new();

    // ── 1. 现在是不是已经在跑 ──
    let running = crate::session::active(Some(instance_id)).is_some();
    checks.push(item(
        "running",
        "实例没有在运行",
        true,
        !running,
        if running {
            Some("这个实例已经启动了，先退出游戏再启动".into())
        } else {
            None
        },
    ));

    // ── 2. 有没有进行中的任务 ──
    let busy = crate::task::has_running(instance_id);
    checks.push(item(
        "tasks",
        "没有进行中的任务",
        true,
        !busy,
        if busy {
            Some("这个实例还有安装或更新任务在跑，等它结束再启动".into())
        } else {
            None
        },
    ));

    // ── 3. 用户名 ──
    let cfg = crate::core::load_config();
    let user_ok = crate::account::validate_username(&cfg.username).is_ok();
    checks.push(item(
        "username",
        "游戏用户名可用",
        true,
        user_ok,
        if user_ok {
            Some(format!("当前用户名：{}", cfg.username))
        } else {
            crate::account::validate_username(&cfg.username)
                .err()
                .map(|e| human(&e))
        },
    ));

    // ── 4. 版本文件 ──
    let launch_version = inst
        .launch_version
        .clone()
        .unwrap_or(inst.game_version.clone());
    let report = crate::verify::check(
        game,
        &launch_version,
        &crate::verify::CheckOptions::default(),
    );
    let files_ok = report.exists && report.complete;
    let files_detail = if files_ok {
        Some(format!("{} 个文件都在", report.ok))
    } else if !report.exists {
        Some(format!("版本 {launch_version} 没有安装"))
    } else {
        let mut parts = Vec::new();
        if report.missing > 0 {
            parts.push(format!("缺 {} 个文件", report.missing));
        }
        if report.corrupt > 0 {
            parts.push(format!("{} 个文件校验不通过", report.corrupt));
        }
        if parts.is_empty() {
            parts.push("版本文件不完整".into());
        }
        Some(parts.join("，") + "。可以用「检查并修复」补回来")
    };
    checks.push(item("files", "版本文件完整", true, files_ok, files_detail));

    // ── 5. Java ──
    let (java_ok, java_detail) = match crate::java_audit::check_instance(game, instance_id, stats) {
        Ok(j) if j.compatible => (
            true,
            Some(match (&j.version, j.major) {
                (Some(v), Some(m)) => format!("Java {m}（{v}），来源：{}", java_source(&j.source)),
                _ => "Java 可用".to_string(),
            }),
        ),
        Ok(j) => {
            let mut msg = j.problem.unwrap_or_else(|| "Java 版本不匹配".into());
            if let Some(s) = j.suggestion {
                msg.push('；');
                msg.push_str(&s);
            }
            (false, Some(msg))
        }
        Err(e) => (false, Some(human(&e))),
    };
    checks.push(item(
        "java",
        "Java 可用且版本匹配",
        true,
        java_ok,
        java_detail,
    ));

    // ── 6. 内存（只提示，不拦） ──
    let (total_mb, free_mb) = total_memory_mb();
    let mods_dir = Path::new(&inst.dirs.mods);
    let mod_count = std::fs::read_dir(mods_dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| {
                    let n = e.file_name().to_string_lossy().to_ascii_lowercase();
                    n.ends_with(".jar") || n.ends_with(".jar.disabled")
                })
                .count() as u32
        })
        .unwrap_or(0);
    let advice = memory_advice(
        total_mb,
        free_mb,
        inst.memory_mb,
        inst.loader.is_some(),
        mod_count,
    );
    checks.push(item(
        "memory",
        "内存设置合理",
        false,
        advice.ok,
        Some(advice.note.clone()),
    ));

    // ── 6b. 自定义 JVM 参数（-Xmx 之类写错会直接起不来，挡启动） ──
    let jvm = crate::jvm::check(inst.memory_mb, &inst.jvm_args, total_mb);
    let jvm_errors: Vec<&str> = jvm
        .issues
        .iter()
        .filter(|i| i.severity == "error")
        .map(|i| i.message.as_str())
        .collect();
    let jvm_warns: Vec<&str> = jvm
        .issues
        .iter()
        .filter(|i| i.severity == "warn")
        .map(|i| i.message.as_str())
        .collect();
    let jvm_ok = jvm_errors.is_empty();
    let jvm_detail = if !jvm_errors.is_empty() {
        Some(jvm_errors.join("；"))
    } else if !jvm_warns.is_empty() {
        Some(jvm_warns.join("；"))
    } else if jvm.args.is_empty() {
        Some("没有自定义 JVM 参数".into())
    } else {
        Some(format!("{} 条自定义参数，看起来没问题", jvm.args.len()))
    };
    checks.push(item("jvm", "JVM 参数可用", true, jvm_ok, jvm_detail));

    // ── 7. Mod 冲突（只提示，不拦） ──
    let scan = crate::modscan::scan(mods_dir);
    let errors = scan.issues.iter().filter(|i| i.severity == "error").count();
    let warns = scan.issues.iter().filter(|i| i.severity == "warn").count();
    let mods_ok = errors == 0;
    let mods_detail = if errors > 0 {
        Some(format!(
            "{errors} 个问题可能导致启动失败（还有 {warns} 条提醒）。可以先用「扫描 Mod」看详情"
        ))
    } else if warns > 0 {
        Some(format!("{warns} 条提醒，一般不挡启动"))
    } else if mod_count > 0 {
        Some(format!("{mod_count} 个 Mod，没发现问题"))
    } else {
        Some("没有 Mod".into())
    };
    checks.push(item(
        "mods",
        "Mod 没有明显冲突",
        false,
        mods_ok,
        mods_detail,
    ));

    let blockers: Vec<String> = checks
        .iter()
        .filter(|c| c.blocking && !c.ok)
        .map(|c| c.detail.clone().unwrap_or_else(|| c.label.clone()))
        .collect();
    let ready = blockers.is_empty();

    Ok(Preflight {
        instance_id: inst.id.clone(),
        instance_name: inst.name.clone(),
        game_version: inst.game_version.clone(),
        ready,
        checks,
        memory: advice,
        blockers,
        collected_at: crate::time::now_iso(),
    })
}

fn java_source(source: &str) -> &str {
    match source {
        "instance" => "实例指定",
        "auto" => "自动选择",
        other => other,
    }
}

/// `CODE|消息|细节` → 消息（给玩家看的）
fn human(encoded: &str) -> String {
    let parts: Vec<&str> = encoded.split('|').collect();
    if parts.len() >= 2 {
        parts[1].to_string()
    } else {
        encoded.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vanilla_on_a_big_machine() {
        let a = memory_advice(32768, 20000, None, false, 0);
        assert_eq!(a.recommended_mb, 2048);
        assert_eq!(a.max_mb, 8192);
        assert!(a.ok);
    }

    #[test]
    fn loader_and_mods_raise_the_number() {
        let light = memory_advice(16384, 12000, None, true, 0);
        let heavy = memory_advice(16384, 12000, None, true, 120);
        assert_eq!(light.recommended_mb, 3072);
        // 120 个 Mod 已经超过 100 的封顶，加成最多 2048
        assert_eq!(heavy.recommended_mb, 5120);
        assert!(heavy.recommended_mb > light.recommended_mb);
    }

    #[test]
    fn caps_at_half_of_system_memory() {
        // 8 GB 机器：一半是 4096，减 2 GB 是 6144，取小的
        let a = memory_advice(8192, 6000, None, true, 200);
        assert_eq!(a.max_mb, 4096);
        assert_eq!(a.recommended_mb, 4096);
    }

    #[test]
    fn small_machine_stays_sane() {
        let a = memory_advice(2048, 1500, None, false, 0);
        assert_eq!(a.max_mb, 1024);
        assert_eq!(a.recommended_mb, 1024);
    }

    #[test]
    fn flags_too_small_and_too_large() {
        let small = memory_advice(16384, 12000, Some(512), false, 0);
        assert!(!small.ok);
        assert!(small.note.contains("太少"));

        let big = memory_advice(16384, 12000, Some(16384), false, 0);
        assert!(!big.ok);
        assert!(big.note.contains("上限"));

        let fine = memory_advice(16384, 12000, Some(4096), false, 0);
        assert!(fine.ok);
    }

    #[test]
    fn rounds_to_512() {
        // 3 GB 机器：一半 1536，减 2 GB 剩 1024 → 上限 1024
        let a = memory_advice(3072, 2000, None, false, 0);
        assert_eq!(a.max_mb % 512, 0);
        assert_eq!(a.recommended_mb % 512, 0);
    }

    #[test]
    fn strips_error_code_for_display() {
        assert_eq!(human("INSTANCE_BUSY|实例正在运行|pid 123"), "实例正在运行");
        assert_eq!(human("没有编码"), "没有编码");
    }
}
