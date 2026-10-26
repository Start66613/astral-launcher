//! 下载源测速。
//!
//! 配置里的 `source` 一直是玩家自己选的（`official` / `bmclapi`），但没人知道
//! 自己该选哪个：国内连 Mojang 有时几百毫秒、有时直接连不上，而镜像偶尔
//! 又会慢得离谱。结果是"下载慢"变成玄学。
//!
//! 这里做的事很简单：对每个源的下载主机做几次 TCP 连接，量往返时间，
//! 然后给一句建议。**测的是连接延迟，不是下载速度** —— 说清楚这点比数字
//! 好看更重要，所以返回值里带 `note`。
//!
//! 为什么不真下文件测吞吐：每个源都得下一份几 MB 的东西，玩家点一下
//! 测速就要多耗几十 MB 流量，而且慢速下载很容易把界面卡住。连接延迟
//! 已经足够区分"这个源通不通、快不快"。
//!
//! DNS 解析和 TCP 连接分开计时：解析慢和连接慢是两种完全不同的毛病，
//! 混成一个数字就看不出问题在哪了。

use serde::Serialize;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

/// 每个源的下载主机。和 `net::ALLOWED_HOSTS` 里的下载域名保持一致。
///
/// 元组是 `(source 值, 给玩家看的名字, 主机名)`。
pub const SOURCES: &[(&str, &str, &str)] = &[
    ("official", "官方源（Mojang）", "piston-meta.mojang.com"),
    ("bmclapi", "BMCLAPI 国内镜像", "bmclapi2.bangbang93.com"),
];

/// 探测端口。所有源都走 https。
const PROBE_PORT: u16 = 443;
/// 默认采样次数。取最小值，避免偶尔一次抖动把结论带偏。
pub const SAMPLES: u32 = 3;
/// 单次连接的超时。
pub const TIMEOUT_MS: u64 = 3000;
/// 两个源差距小于这个值就算"差不多"，这时优先官方源。
///
/// 理由：镜像的内容可能比官方晚几小时到几天，延迟差不多时没理由用它。
const CLOSE_MS: u64 = 20;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SourceProbe {
    /// 配置里 `source` 该填的值
    pub source: String,
    pub label: String,
    pub host: String,
    /// 连得上就是 true（只要有一次成功）
    pub reachable: bool,
    /// DNS 解析耗时（毫秒，取最快的一次）
    pub dns_ms: Option<u64>,
    /// TCP 连接耗时（毫秒，取最快的一次）
    pub connect_ms: Option<u64>,
    /// 实际尝试了几次
    pub samples: u32,
    /// 连不上时的原因，给玩家看的
    pub error: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProbeReport {
    pub sources: Vec<SourceProbe>,
    /// 建议用的源；全都不通时是 null
    pub suggested: Option<String>,
    /// 一句话说清测的是什么、别拿它当下载速度
    pub note: String,
}

/// 对 `host:port` 做几次连接，返回 `(是否可达, DNS 最快毫秒, 连接最快毫秒, 错误)`。
///
/// 拆出来是为了能用本地监听端口测，不依赖外网。
pub fn probe_addr(
    host: &str,
    port: u16,
    timeout_ms: u64,
    samples: u32,
) -> (bool, Option<u64>, Option<u64>, Option<String>) {
    let n = samples.max(1);
    let mut best_dns: Option<u64> = None;
    let mut best_conn: Option<u64> = None;
    let mut last_err: Option<String> = None;

    for _ in 0..n {
        let t0 = Instant::now();
        let addrs = match (host, port).to_socket_addrs() {
            Ok(a) => a.collect::<Vec<_>>(),
            Err(e) => {
                last_err = Some(format!("解析 {host} 失败：{e}"));
                continue;
            }
        };
        let dns = t0.elapsed().as_millis() as u64;
        best_dns = Some(best_dns.map_or(dns, |b| b.min(dns)));

        if addrs.is_empty() {
            last_err = Some(format!("{host} 没有解析出地址"));
            continue;
        }

        let mut connected = false;
        for addr in addrs {
            let t1 = Instant::now();
            match TcpStream::connect_timeout(&addr, Duration::from_millis(timeout_ms)) {
                Ok(_) => {
                    let ms = t1.elapsed().as_millis() as u64;
                    best_conn = Some(best_conn.map_or(ms, |b| b.min(ms)));
                    connected = true;
                    break;
                }
                Err(e) => last_err = Some(format!("连接 {addr} 失败：{e}")),
            }
        }
        if connected {
            // 已经成功一次就不必再试，省得玩家等
            break;
        }
    }

    (best_conn.is_some(), best_dns, best_conn, last_err)
}

/// 探一个源。
pub fn probe_one(source: &str) -> Option<SourceProbe> {
    let (key, label, host) = SOURCES.iter().find(|(k, _, _)| *k == source)?;
    let (reachable, dns_ms, connect_ms, error) = probe_addr(host, PROBE_PORT, TIMEOUT_MS, SAMPLES);
    Some(SourceProbe {
        source: key.to_string(),
        label: label.to_string(),
        host: host.to_string(),
        reachable,
        dns_ms,
        connect_ms,
        samples: SAMPLES,
        error: if reachable { None } else { error },
    })
}

/// 探全部源，并给出建议。
pub fn probe_all() -> ProbeReport {
    let sources: Vec<SourceProbe> = SOURCES
        .iter()
        .filter_map(|(k, _, _)| probe_one(k))
        .collect();
    let suggested = pick(&sources);
    ProbeReport {
        sources,
        suggested,
        note: "测的是连接延迟，不是下载速度；数字越小连得越快。".to_string(),
    }
}

/// 从探测结果里挑一个建议源。
///
/// - 全都不通 → `None`
/// - 官方源可达、且和最快的那个差距不到 `CLOSE_MS` → 用官方（内容最新）
/// - 否则用最快的
pub fn pick(sources: &[SourceProbe]) -> Option<String> {
    let mut ok: Vec<&SourceProbe> = sources.iter().filter(|s| s.reachable).collect();
    if ok.is_empty() {
        return None;
    }
    ok.sort_by_key(|s| s.connect_ms.unwrap_or(u64::MAX));
    let best = ok[0];

    if best.source != "official" {
        if let Some(official) = ok.iter().find(|s| s.source == "official") {
            // 官方比最快的慢多少？慢得不多就用官方（内容更新）
            let gap = official
                .connect_ms
                .unwrap_or(u64::MAX)
                .saturating_sub(best.connect_ms.unwrap_or(u64::MAX));
            if gap < CLOSE_MS {
                return Some(official.source.clone());
            }
        }
    }
    Some(best.source.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn probe(source: &str, reachable: bool, ms: Option<u64>) -> SourceProbe {
        SourceProbe {
            source: source.into(),
            label: source.into(),
            host: "example.com".into(),
            reachable,
            dns_ms: Some(1),
            connect_ms: ms,
            samples: 1,
            error: None,
        }
    }

    #[test]
    fn probes_an_open_port() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        // 有东西在监听，连接就能成功；不必 accept，backlog 会接住
        let (reachable, dns, conn, err) = probe_addr("127.0.0.1", port, 1000, 2);
        assert!(reachable, "本地监听端口应该连得上：{err:?}");
        assert!(dns.is_some());
        assert!(conn.is_some());
        assert!(err.is_none());
    }

    #[test]
    fn reports_a_closed_port() {
        // 绑一个端口再立刻释放，这个端口基本不会再有人监听
        let port = {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        let (reachable, _dns, conn, err) = probe_addr("127.0.0.1", port, 500, 1);
        assert!(!reachable, "没人监听的端口不该连上");
        assert!(conn.is_none());
        assert!(err.is_some(), "连不上要给原因");
    }

    #[test]
    fn reports_unresolvable_host() {
        let (reachable, _dns, _conn, err) =
            probe_addr("this-host-should-not-exist.invalid", 443, 300, 1);
        assert!(!reachable);
        assert!(err.is_some());
    }

    #[test]
    fn picks_the_fastest_reachable() {
        let list = vec![
            probe("official", true, Some(300)),
            probe("bmclapi", true, Some(40)),
        ];
        assert_eq!(pick(&list).as_deref(), Some("bmclapi"));
    }

    #[test]
    fn prefers_official_when_close() {
        let list = vec![
            probe("official", true, Some(100)),
            probe("bmclapi", true, Some(90)),
        ];
        // 只差 10 ms，用官方，内容更新
        assert_eq!(pick(&list).as_deref(), Some("official"));
    }

    #[test]
    fn skips_unreachable_sources() {
        let list = vec![
            probe("official", false, None),
            probe("bmclapi", true, Some(500)),
        ];
        assert_eq!(pick(&list).as_deref(), Some("bmclapi"));
    }

    #[test]
    fn returns_none_when_nothing_works() {
        let list = vec![
            probe("official", false, None),
            probe("bmclapi", false, None),
        ];
        assert_eq!(pick(&list), None);
    }

    #[test]
    fn every_source_has_a_label_and_host() {
        assert!(!SOURCES.is_empty());
        for (key, label, host) in SOURCES {
            assert!(!key.is_empty());
            assert!(!label.is_empty(), "{key} 没有给玩家看的名字");
            assert!(host.contains('.'), "{key} 的主机名看着不对：{host}");
            assert!(!host.contains("://"), "{key} 的主机名不该带协议");
        }
    }
}
