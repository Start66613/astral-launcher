//! 极简时间工具：只做本地需要的 UTC ISO8601 / 时间戳转换。
//!
//! 不引入 chrono，避免它的 Windows 时间依赖影响测试二进制的加载。

use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 1970-01-01 起的天数 → (年, 月, 日)
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    ((if m <= 2 { y + 1 } else { y }), m, d)
}

/// (年, 月, 日) → 1970-01-01 起的天数
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64; // [0, 399]
    let mp = if m > 2 { m - 3 } else { m + 9 } as u64; // [0, 11]
    let doy = (153 * mp + 2) / 5 + d as u64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe as i64 - 719_468
}

pub fn unix_ms_to_iso(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, mo, d) = civil_from_days(days);
    let hh = rem / 3600;
    let mi = (rem % 3600) / 60;
    let ss = rem % 60;
    format!("{y:04}-{mo:02}-{d:02}T{hh:02}:{mi:02}:{ss:02}Z")
}

pub fn now_iso() -> String {
    unix_ms_to_iso(now_unix_ms())
}

/// 压缩时间戳，用于回收站目录名
pub fn now_stamp() -> String {
    let iso = now_iso();
    iso.replace(['-', ':'], "").replace('T', "-").replace('Z', "")
}

/// 只解析本模块产出的 `YYYY-MM-DDTHH:MM:SSZ`
pub fn iso_to_unix_ms(s: &str) -> Option<i64> {
    if s.len() < 20 {
        return None;
    }
    let num = |a: usize, l: usize| -> Option<i64> { s.get(a..a + l)?.parse().ok() };
    let y = num(0, 4)?;
    let mo = num(5, 2)? as u32;
    let d = num(8, 2)? as u32;
    let h = num(11, 2)?;
    let mi = num(14, 2)?;
    let se = num(17, 2)?;
    let days = days_from_civil(y, mo, d);
    Some((days * 86_400 + h * 3600 + mi * 60 + se) * 1000)
}

pub fn system_time_to_iso(t: SystemTime) -> String {
    let ms = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    unix_ms_to_iso(ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_roundtrip() {
        assert_eq!(unix_ms_to_iso(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_to_unix_ms("1970-01-01T00:00:00Z"), Some(0));
    }

    #[test]
    fn known_timestamp() {
        // 2026-10-23T00:00:00Z
        let iso = "2026-10-23T00:00:00Z";
        let ms = iso_to_unix_ms(iso).unwrap();
        assert_eq!(unix_ms_to_iso(ms), iso);
    }

    #[test]
    fn roundtrip_now() {
        let iso = now_iso();
        let back = iso_to_unix_ms(&iso).unwrap();
        assert!((now_unix_ms() - back).abs() < 2000);
    }

    #[test]
    fn leap_year() {
        assert_eq!(unix_ms_to_iso(iso_to_unix_ms("2024-02-29T12:00:00Z").unwrap()), "2024-02-29T12:00:00Z");
    }
}
