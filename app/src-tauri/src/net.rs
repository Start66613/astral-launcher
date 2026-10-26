//! 下载与路径安全（契约 V2 §13 / V2.5 A1–A4）。
//!
//! 规则：
//! 1. 只允许白名单主机，必须是 https，重定向每一跳都要重新过白名单；
//! 2. 解析出来的 IP 不能是内网 / 本机 / 链路本地，防 DNS 把人骗进内网；
//! 3. 只写允许目录之内；目标或 `.part` 若是符号链接 / junction 一律拒绝；
//! 4. 先落 `.part`，校验通过后才原子替换；取消时保留 `.part` 以便续传；
//! 5. 同一个目标文件同一时间只允许一个任务写，其余任务等待并复用结果。

use std::collections::HashMap;
use std::io::Read;
use std::net::{IpAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// 允许的下载来源。新增来源前先确认它是否会被重定向到别处。
pub const ALLOWED_HOSTS: &[&str] = &[
    // Modrinth
    "cdn.modrinth.com",
    "api.modrinth.com",
    // Mojang
    "piston-data.mojang.com",
    "piston-metadata.mojang.com",
    "launchermeta.mojang.com",
    "launcher.mojang.com",
    "resources.download.minecraft.net",
    "libraries.minecraft.net",
    // 加载器
    "meta.fabricmc.net",
    "maven.fabricmc.net",
    "maven.minecraftforge.net",
    "files.minecraftforge.net",
    "maven.neoforged.net",
    "meta.quiltmc.org",
    "maven.quiltmc.org",
    // JDK 下载（Adoptium；下载后另有 SHA256 校验）
    "api.adoptium.net",
    "github.com",
    // 国内镜像
    "bmclapi2.bangbang93.com",
];

fn blocked(reason: &str) -> String {
    format!("BLOCKED_URL|下载地址被拒绝|{reason}")
}

/// 校验下载地址；返回小写主机名。
///
/// 只接受 `https://`，拒绝 userinfo、IP 字面量、localhost、非白名单主机。
pub fn check_url(raw: &str) -> Result<String, String> {
    let s = raw.trim();
    let rest = s
        .strip_prefix("https://")
        .ok_or_else(|| blocked("只允许 https 地址"))?;

    let host_port = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host_port.contains('@') {
        return Err(blocked("地址不得包含用户信息"));
    }

    let host = host_port
        .split(':')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if host.is_empty() {
        return Err(blocked("缺少主机名"));
    }
    if host.parse::<std::net::IpAddr>().is_ok() || host == "localhost" || host.ends_with(".local") {
        return Err(blocked("不允许本机或内网地址"));
    }
    if !ALLOWED_HOSTS.iter().any(|h| host == *h) {
        return Err(blocked(&format!("不在允许的来源列表内：{host}")));
    }
    Ok(host)
}

/// 这个 IP 是否可以安全访问（公网、非保留段）。
pub fn is_public_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
                || v4.is_multicast()
                // 100.64.0.0/10 运营商级 NAT
                || (o[0] == 100 && (64..128).contains(&o[1]))
                // 192.0.0.0/24 保留
                || (o[0] == 192 && o[1] == 0 && o[2] == 0)
                // 198.18.0.0/15 基准测试
                || (o[0] == 198 && (o[1] == 18 || o[1] == 19))
                // 240.0.0.0/4 保留
                || o[0] >= 240)
        }
        IpAddr::V6(v6) => {
            if v6.is_loopback() || v6.is_unspecified() || v6.is_multicast() {
                return false;
            }
            let seg = v6.segments();
            // fc00::/7 唯一本地地址
            if (seg[0] & 0xfe00) == 0xfc00 {
                return false;
            }
            // fe80::/10 链路本地
            if (seg[0] & 0xffc0) == 0xfe80 {
                return false;
            }
            // IPv4 映射地址：按 v4 再判一次
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public_ip(&IpAddr::V4(v4));
            }
            true
        }
    }
}

fn dns_cache() -> &'static Mutex<HashMap<String, (u64, bool)>> {
    static C: OnceLock<Mutex<HashMap<String, (u64, bool)>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

const DNS_TTL_MS: u64 = 10 * 60 * 1000;

/// 解析主机名，确认所有解析结果都是公网地址。
///
/// 结果缓存 10 分钟，避免每个文件都查一次 DNS。
pub fn resolve_public(host: &str) -> Result<(), String> {
    let now = crate::time::now_unix_ms() as u64;
    if let Ok(cache) = dns_cache().lock() {
        if let Some((at, ok)) = cache.get(host) {
            if now.saturating_sub(*at) < DNS_TTL_MS {
                return if *ok {
                    Ok(())
                } else {
                    Err(blocked(&format!("域名解析到内网地址：{host}")))
                };
            }
        }
    }

    let addrs: Vec<IpAddr> = match (host, 443u16).to_socket_addrs() {
        Ok(it) => it.map(|a| a.ip()).collect(),
        Err(e) => {
            return Err(format!("NETWORK|域名解析失败|{host}: {e}"));
        }
    };
    let ok = !addrs.is_empty() && addrs.iter().all(is_public_ip);

    if let Ok(mut cache) = dns_cache().lock() {
        cache.insert(host.to_string(), (now, ok));
    }

    if !ok {
        return Err(blocked(&format!("域名解析到内网地址：{host}")));
    }
    Ok(())
}

/// 完整校验一个 URL：协议 / 主机 / 解析结果。下载每一跳都要调它。
pub fn validate_url(raw: &str) -> Result<(), String> {
    validate_url_with(raw, false)
}

/// 自检专用：允许连本机测试服务器（`127.0.0.1` 的 http 地址）。
///
/// 只有 `--selftest` 构造的 `DownloadOptions` 会把这个开关打开，
/// 生产代码任何路径都不会传 `true`。
fn validate_url_with(raw: &str, allow_local: bool) -> Result<(), String> {
    if allow_local {
        if let Some(rest) = raw.trim().strip_prefix("http://") {
            let host_port = rest.split(['/', '?', '#']).next().unwrap_or("");
            let host = host_port.split(':').next().unwrap_or("");
            if host == "127.0.0.1" {
                return Ok(());
            }
        }
    }
    let host = check_url(raw)?;
    resolve_public(&host)
}

/// 目标或临时文件是不是符号链接 / junction / 其他重解析点。
#[cfg(windows)]
pub fn is_reparse_point(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    match std::fs::symlink_metadata(path) {
        Ok(m) => m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0,
        Err(_) => false,
    }
}

#[cfg(not(windows))]
pub fn is_reparse_point(path: &Path) -> bool {
    match std::fs::symlink_metadata(path) {
        Ok(m) => m.file_type().is_symlink(),
        Err(_) => false,
    }
}

/// 目标必须落在 `root` 之内；返回 canonical 后的完整路径。
///
/// 不存在的父目录会被创建；目标文件本身可以不存在，但若已存在且是
/// 符号链接 / junction 则拒绝，防止被引到目录之外。
pub fn ensure_within(root: &Path, target: &Path) -> Result<PathBuf, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("INVALID_PATH|根目录不可用|{e}"))?;

    let name = target
        .file_name()
        .ok_or_else(|| "INVALID_PATH|目标文件名无效".to_string())?;
    let parent = target
        .parent()
        .ok_or_else(|| "INVALID_PATH|目标目录无效".to_string())?;

    std::fs::create_dir_all(parent).map_err(|e| format!("PERMISSION|创建目录失败|{e}"))?;
    let parent = parent
        .canonicalize()
        .map_err(|e| format!("INVALID_PATH|目录不可解析|{e}"))?;

    if !parent.starts_with(&root) {
        return Err("INVALID_PATH|目标超出允许目录".into());
    }

    let full = parent.join(name);
    if is_reparse_point(&full) {
        return Err("INVALID_PATH|目标是一个符号链接，已拒绝写入".into());
    }
    Ok(full)
}

/// 写文件前单独检查一次，`ensure_within` 之后再调用。
pub fn ensure_safe_write_target(path: &Path) -> Result<(), String> {
    if is_reparse_point(path) {
        return Err(format!(
            "INVALID_PATH|{} 是符号链接，已拒绝写入",
            path.display()
        ));
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// 文件的 SHA1（小写十六进制）
pub fn sha1_file(path: &Path) -> Result<String, String> {
    use sha1::{Digest, Sha1};
    let mut f = std::fs::File::open(path).map_err(|e| format!("INVALID_PATH|文件不可读|{e}"))?;
    let mut hasher = Sha1::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| format!("INVALID_PATH|读取失败|{e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex(&hasher.finalize()))
}

/// 文件的 SHA512（小写十六进制）
pub fn sha512_file(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha512};
    let mut f = std::fs::File::open(path).map_err(|e| format!("INVALID_PATH|文件不可读|{e}"))?;
    let mut hasher = Sha512::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| format!("INVALID_PATH|读取失败|{e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex(&hasher.finalize()))
}

/// 文件的 SHA256（小写十六进制）
pub fn sha256_file(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut f = std::fs::File::open(path).map_err(|e| format!("INVALID_PATH|文件不可读|{e}"))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| format!("INVALID_PATH|读取失败|{e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex(&hasher.finalize()))
}

// ── 取消令牌 ─────────────────────────────────────────────────────

/// 取消判断函数。任务层用 `task::is_cancelled` 包一个。
pub type CancelFn = Arc<dyn Fn() -> bool + Send + Sync>;

/// 借用的取消判断，避免为了传参再分配一个 Arc。
pub type CancelRef<'a> = &'a (dyn Fn() -> bool + Send + Sync);

/// 永不取消，给没有任务上下文的调用方（自检、修复）用。
pub fn never_cancel() -> CancelFn {
    Arc::new(|| false)
}

/// 把闭包包成取消令牌。
#[allow(dead_code)]
pub fn cancel_from(f: impl Fn() -> bool + Send + Sync + 'static) -> CancelFn {
    Arc::new(f)
}

struct CancelAdapter<'a>(CancelRef<'a>);

impl portablemc::stream::Cancel for CancelAdapter<'_> {
    fn cancelled(&self) -> bool {
        (self.0)()
    }
}

// ── 同一目标的写锁 ───────────────────────────────────────────────

static LOCKS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

fn lock_map() -> &'static Mutex<HashMap<String, String>> {
    LOCKS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock_key(dest: &Path) -> String {
    dest.to_string_lossy().to_ascii_lowercase()
}

/// 每次下载用独立的 owner，避免两个不同任务因为 owner 同名而被当成"自己重入"。
static OWNER_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn unique_owner() -> String {
    format!(
        "dl-{}",
        OWNER_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
}

struct WriteLock {
    key: String,
}

impl Drop for WriteLock {
    fn drop(&mut self) {
        if let Ok(mut m) = lock_map().lock() {
            m.remove(&self.key);
        }
    }
}

/// 拿到目标文件的写锁；被别的任务占用时轮询等待，可被取消打断。
fn acquire_write(
    dest: &Path,
    owner: &str,
    cancel: CancelRef<'_>,
    timeout: std::time::Duration,
) -> Result<WriteLock, String> {
    let key = lock_key(dest);
    let start = std::time::Instant::now();
    loop {
        {
            let mut m = lock_map()
                .lock()
                .map_err(|_| "INTERNAL|下载锁不可用".to_string())?;
            match m.get(&key) {
                None => {
                    m.insert(key.clone(), owner.to_string());
                    return Ok(WriteLock { key });
                }
                // 同一个任务重入：直接放行，避免自己等自己
                Some(o) if o == owner => return Ok(WriteLock { key: String::new() }),
                Some(_) => {}
            }
        }

        if (cancel)() {
            return Err("CANCELLED|任务已取消".into());
        }
        if start.elapsed() > timeout {
            return Err(format!(
                "BUSY|同一个文件正在被另一个任务下载|{}",
                dest.display()
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
}

// ── .part 断点元数据 ────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Default, Clone)]
struct PartMeta {
    url: String,
    #[serde(default)]
    etag: Option<String>,
    #[serde(default)]
    last_modified: Option<String>,
    #[serde(default)]
    total: Option<u64>,
    #[serde(default)]
    bytes: u64,
    #[serde(default)]
    updated_ms: u64,
}

fn part_path(dest: &Path) -> PathBuf {
    PathBuf::from(format!("{}.part", dest.to_string_lossy()))
}

fn meta_path(dest: &Path) -> PathBuf {
    PathBuf::from(format!("{}.part.json", dest.to_string_lossy()))
}

fn load_meta(dest: &Path) -> Option<PartMeta> {
    let raw = std::fs::read_to_string(meta_path(dest)).ok()?;
    serde_json::from_str(&raw).ok()
}

fn save_meta(dest: &Path, meta: &PartMeta) {
    if let Ok(raw) = serde_json::to_string(meta) {
        let _ = std::fs::write(meta_path(dest), raw);
    }
}

fn clear_partial(dest: &Path) {
    let _ = std::fs::remove_file(part_path(dest));
    let _ = std::fs::remove_file(meta_path(dest));
}

// ── 并发调度 ────────────────────────────────────────────────────

/// 全局同时下载数上限。太大容易被 CDN 限速，太小又慢。
const MAX_ACTIVE_TOTAL: usize = 6;
/// 同一个域名同时下载数上限，避免把单个站点打挂或被 429。
const MAX_ACTIVE_PER_HOST: usize = 3;

static ACTIVE_TOTAL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static ACTIVE_HOSTS: OnceLock<Mutex<HashMap<String, usize>>> = OnceLock::new();

/// 累计统计，给前端显示"已下载多少 / 当前速度"用。
#[derive(Default)]
struct Counters {
    downloads: u64,
    bytes: u64,
    resumed: u64,
    retries: u64,
}

static COUNTERS: OnceLock<Mutex<Counters>> = OnceLock::new();

fn counters() -> &'static Mutex<Counters> {
    COUNTERS.get_or_init(|| Mutex::new(Counters::default()))
}

fn host_of(url: &str) -> String {
    url.split("://")
        .nth(1)
        .and_then(|r| r.split(['/', '?', '#']).next())
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

struct Slot {
    host: String,
}

impl Drop for Slot {
    fn drop(&mut self) {
        ACTIVE_TOTAL.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        if let Ok(mut m) = ACTIVE_HOSTS
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
        {
            if let Some(n) = m.get_mut(&self.host) {
                *n = n.saturating_sub(1);
                if *n == 0 {
                    m.remove(&self.host);
                }
            }
        }
    }
}

/// 拿一个下载并发名额；满了就等，等待过程可被取消打断。
fn acquire_slot(host: &str, cancel: CancelRef<'_>) -> Result<Slot, String> {
    loop {
        {
            let hosts = ACTIVE_HOSTS.get_or_init(|| Mutex::new(HashMap::new()));
            let mut m = hosts.lock().map_err(|_| "INTERNAL|下载调度器不可用".to_string())?;
            let used = *m.get(host).unwrap_or(&0);
            let total = ACTIVE_TOTAL.load(std::sync::atomic::Ordering::SeqCst);
            if used < MAX_ACTIVE_PER_HOST && total < MAX_ACTIVE_TOTAL {
                *m.entry(host.to_string()).or_insert(0) += 1;
                ACTIVE_TOTAL.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                return Ok(Slot {
                    host: host.to_string(),
                });
            }
        }
        if (cancel)() {
            return Err("CANCELLED|任务已取消".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
    }
}

/// 当前并发与累计统计，给前端与自检看。
pub fn scheduler_stats() -> serde_json::Value {
    let active = ACTIVE_TOTAL.load(std::sync::atomic::Ordering::SeqCst);
    let hosts: HashMap<String, usize> = ACTIVE_HOSTS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map(|m| m.clone())
        .unwrap_or_default();
    let c = counters().lock().map(|c| (c.downloads, c.bytes, c.resumed, c.retries)).unwrap_or((0, 0, 0, 0));
    serde_json::json!({
        "active": active,
        "maxActive": MAX_ACTIVE_TOTAL,
        "maxPerHost": MAX_ACTIVE_PER_HOST,
        "hosts": hosts,
        "downloads": c.0,
        "bytes": c.1,
        "resumed": c.2,
        "retries": c.3,
    })
}

fn count_download(bytes: u64, resumed: bool) {
    if let Ok(mut c) = counters().lock() {
        c.downloads += 1;
        c.bytes = c.bytes.saturating_add(bytes);
        if resumed {
            c.resumed += 1;
        }
    }
}

fn count_retry() {
    if let Ok(mut c) = counters().lock() {
        c.retries += 1;
    }
}

// ── 下载 ────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct DownloadOptions {
    pub expected_sha1: Option<String>,
    pub expected_sha512: Option<String>,
    /// 最多尝试几次（含首次）。0 视作 1。
    pub max_attempts: u32,
    /// 是否允许用已存在的 `.part` 续传
    pub resume: bool,
    /// 目标已存在且哈希匹配时直接复用，不联网
    pub reuse_verified: bool,
    /// 自检专用：允许访问本机 http 测试服务器（生产路径永远为 false）
    pub allow_local_http: bool,
    /// 是否使用跨实例的共享下载缓存（需要给出哈希）
    pub use_cache: bool,
}

impl Default for DownloadOptions {
    fn default() -> Self {
        Self {
            expected_sha1: None,
            expected_sha512: None,
            max_attempts: 3,
            resume: true,
            reuse_verified: true,
            allow_local_http: false,
            use_cache: true,
        }
    }
}

impl DownloadOptions {
    #[allow(dead_code)]
    pub fn with_sha1(sha1: Option<&str>) -> Self {
        Self {
            expected_sha1: sha1.map(|s| s.to_string()),
            ..Default::default()
        }
    }
}

#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct DownloadReport {
    /// 文件最终大小
    pub bytes: u64,
    /// 是否用上了断点续传
    pub resumed: bool,
    /// 实际尝试次数
    pub attempts: u32,
    /// 目标文件已存在且校验通过，没有联网
    pub reused: bool,
}

/// 目标文件是否已存在且哈希对得上。
fn verified_existing(dest: &Path, opts: &DownloadOptions) -> bool {
    if !dest.is_file() {
        return false;
    }
    match opts.expected_sha1.as_deref() {
        Some(want) => sha1_file(dest)
            .map(|got| got.eq_ignore_ascii_case(want))
            .unwrap_or(false),
        None => match opts.expected_sha512.as_deref() {
            Some(want) => sha512_file(dest)
                .map(|got| got.eq_ignore_ascii_case(want))
                .unwrap_or(false),
            None => false,
        },
    }
}

fn is_disk_full(e: &str) -> bool {
    let l = e.to_ascii_lowercase();
    l.contains("磁盘空间不足")
        || l.contains("os error 112")
        || l.contains("os error 39")
        || l.contains("no space left")
}

/// 下载 `url` 到 `dest`，支持取消、断点续传、重试与同文件去重。
///
/// 返回 `Ok` 时 `dest` 一定是完整且（若给了哈希）校验通过的文件。
/// 取消返回 `CANCELLED|…`，`.part` 会被保留供下次续传。
pub fn download(
    url: &str,
    dest: &Path,
    opts: &DownloadOptions,
    cancel: CancelRef<'_>,
    mut on_progress: impl FnMut(u64, Option<u64>),
) -> Result<DownloadReport, String> {
    validate_url_with(url, opts.allow_local_http)?;

    let owner = unique_owner();
    let _lock = acquire_write(dest, &owner, cancel, std::time::Duration::from_secs(600))?;
    let _slot = acquire_slot(&host_of(url), cancel)?;

    if (cancel)() {
        return Err("CANCELLED|任务已取消".into());
    }

    if opts.reuse_verified && verified_existing(dest, opts) {
        let bytes = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
        on_progress(bytes, Some(bytes));
        return Ok(DownloadReport {
            bytes,
            resumed: false,
            attempts: 0,
            reused: true,
        });
    }

    // 跨实例共享缓存：同一个文件别的实例下过，直接复制过来
    if opts.use_cache {
        if let Some(bytes) = crate::cache::restore(
            opts.expected_sha1.as_deref(),
            opts.expected_sha512.as_deref(),
            dest,
        ) {
            // 复制完再验一次：缓存被外力改坏时不会把坏文件交出去
            if verified_existing(dest, opts) {
                on_progress(bytes, Some(bytes));
                return Ok(DownloadReport {
                    bytes,
                    resumed: false,
                    attempts: 0,
                    reused: true,
                });
            }
            let _ = std::fs::remove_file(dest);
        }
    }

    let part = part_path(dest);
    ensure_safe_write_target(&part)?;
    ensure_safe_write_target(&meta_path(dest))?;

    let max_attempts = opts.max_attempts.max(1);    let mut attempt = 0u32;
    let mut last_err = String::new();

    while attempt < max_attempts {
        attempt += 1;

        if (cancel)() {
            return Err("CANCELLED|任务已取消".into());
        }

        // 只有上次中断留下的 .part 才续传；URL 变了就丢掉重下
        let resume = if opts.resume {
            match load_meta(dest) {
                Some(m) if m.url == url && m.bytes > 0 => {
                    let actual = std::fs::metadata(&part).map(|x| x.len()).unwrap_or(0);
                    if actual == m.bytes {
                        Some(portablemc::stream::Resume {
                            offset: m.bytes,
                            etag: m.etag.clone(),
                            last_modified: m.last_modified.clone(),
                            total: m.total,
                        })
                    } else {
                        clear_partial(dest);
                        None
                    }
                }
                Some(_) => {
                    clear_partial(dest);
                    None
                }
                None => None,
            }
        } else {
            clear_partial(dest);
            None
        };
        let was_resuming = resume.as_ref().map(|r| r.usable()).unwrap_or(false);

        let mut last_save = std::time::Instant::now();
        let mut reported: u64 = 0;
        let save_meta_now = |bytes: u64,
                                 etag: &Option<String>,
                                 lm: &Option<String>,
                                 total: Option<u64>| {
            save_meta(
                dest,
                &PartMeta {
                    url: url.to_string(),
                    etag: etag.clone(),
                    last_modified: lm.clone(),
                    total,
                    bytes,
                    updated_ms: crate::time::now_unix_ms() as u64,
                },
            );
        };

        let mut progress = |done: u64, total: Option<u64>| {
            on_progress(done, total);
            reported = done;
            if last_save.elapsed() >= std::time::Duration::from_secs(1) {
                last_save = std::time::Instant::now();
                save_meta_now(done, &None, &None, total);
            }
        };

        let outcome = portablemc::stream::fetch_with(
            url,
            &part,
            resume.as_ref(),
            &CancelAdapter(cancel),
            {
                let allow_local = opts.allow_local_http;
                Arc::new(move |u: &str| validate_url_with(u, allow_local))
            },
            opts.allow_local_http,
            &mut progress,
        );

        match outcome {
            Ok(o) => {
                // 校验哈希；不匹配说明文件坏了，删掉重来一次
                if let Some(want) = opts.expected_sha1.as_deref() {
                    let got = sha1_file(&part)?;
                    if !got.eq_ignore_ascii_case(want) {
                        clear_partial(dest);
                        last_err =
                            format!("HASH_MISMATCH|SHA1 校验失败|期望 {want}，实际 {got}");
                        if attempt < max_attempts {
                            continue;
                        }
                        return Err(last_err);
                    }
                }
                if let Some(want) = opts.expected_sha512.as_deref() {
                    let got = sha512_file(&part)?;
                    if !got.eq_ignore_ascii_case(want) {
                        clear_partial(dest);
                        last_err = format!("HASH_MISMATCH|SHA512 校验失败|期望 {want}");
                        if attempt < max_attempts {
                            continue;
                        }
                        return Err(last_err);
                    }
                }

                if (cancel)() {
                    // 下载完了才收到取消：文件是完整的，按完成上报，由上层决定怎么说
                    save_meta_now(o.total_bytes, &o.etag, &o.last_modified, o.content_length);
                }

                let size = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(o.total_bytes);
                std::fs::rename(&part, dest).map_err(|e| {
                    format!("PERMISSION|替换文件失败|{e}")
                })?;
                let _ = std::fs::remove_file(meta_path(dest));

                if opts.use_cache {
                    crate::cache::store(
                        opts.expected_sha1.as_deref(),
                        opts.expected_sha512.as_deref(),
                        dest,
                    );
                }
                count_download(size, was_resuming);
                return Ok(DownloadReport {
                    bytes: size,
                    resumed: was_resuming,
                    attempts: attempt,
                    reused: false,
                });
            }
            Err(portablemc::stream::Error::Cancelled {
                total_bytes,
                etag,
                last_modified,
                content_length,
            }) => {
                // 关键：保留 .part 与元数据，下次可以接着下
                save_meta_now(total_bytes, &etag, &last_modified, content_length);
                return Err(format!("CANCELLED|任务已取消|已下载 {total_bytes} 字节"));
            }
            Err(e) => {
                last_err = format!("{}|{}", e.code(), e.message());
                if is_disk_full(&last_err) {
                    return Err(format!("DISK_FULL|磁盘空间不足|{url}"));
                }
                if !e.retryable() || attempt >= max_attempts {
                    // 404 之类没必要重试，也别把半截文件当成可续传
                    if !e.retryable() {
                        clear_partial(dest);
                    }
                    return Err(last_err);
                }
                count_retry();
                // 指数退避，但可以被取消打断
                let wait = std::time::Duration::from_millis(300 * 3u64.pow(attempt - 1));
                let deadline = std::time::Instant::now() + wait;
                while std::time::Instant::now() < deadline {
                    if (cancel)() {
                        return Err("CANCELLED|任务已取消".into());
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
        }
    }

    Err(if last_err.is_empty() {
        "NETWORK|下载失败|重试次数用尽".to_string()
    } else {
        last_err
    })
}


/// 原子下载：先写 `<dest>.part`，校验后替换。
pub fn download_atomic(
    url: &str,
    dest: &Path,
    expected_sha1: Option<&str>,
    on_progress: impl FnMut(u32, u32, u32, u32),
) -> Result<u64, String> {
    download_verified(url, dest, expected_sha1, None, on_progress)
}

/// 下载并校验（SHA1 + 可选 SHA512），**全部校验通过后才提交正式文件**。
///
/// 旧文件在提交前不会被改动；任何失败都只删除 `.part`，旧文件原样保留。
pub fn download_verified(
    url: &str,
    dest: &Path,
    expected_sha1: Option<&str>,
    expected_sha512: Option<&str>,
    mut on_progress: impl FnMut(u32, u32, u32, u32),
) -> Result<u64, String> {
    let opts = DownloadOptions {
        expected_sha1: expected_sha1.map(|s| s.to_string()),
        expected_sha512: expected_sha512.map(|s| s.to_string()),
        ..Default::default()
    };
    let cancel = never_cancel();
    let report = download(url, dest, &opts, &*cancel, |done, total| {
        on_progress(1, 1, done.min(u32::MAX as u64) as u32, total.unwrap_or(0).min(u32::MAX as u64) as u32);
    })?;
    Ok(report.bytes)
}

/// 带取消的下载，进度回调是 (已下载字节, 总字节)。
#[allow(dead_code)]
pub fn download_with_cancel(
    url: &str,
    dest: &Path,
    expected_sha1: Option<&str>,
    expected_sha512: Option<&str>,
    cancel: CancelRef<'_>,
    on_progress: impl FnMut(u64, Option<u64>),
) -> Result<DownloadReport, String> {
    let opts = DownloadOptions {
        expected_sha1: expected_sha1.map(|s| s.to_string()),
        expected_sha512: expected_sha512.map(|s| s.to_string()),
        ..Default::default()
    };
    download(url, dest, &opts, cancel, on_progress)
}

/// 拉取 JSON（临时文件，不缓存），用于 Modrinth API
pub fn fetch_json(url: &str) -> Result<serde_json::Value, String> {
    validate_url(url)?;
    let tmp = std::env::temp_dir().join(format!(
        "astral-api-{:x}-{:x}.json",
        crate::time::now_unix_ms(),
        std::process::id()
    ));
    let _ = std::fs::remove_file(&tmp);
    let opts = DownloadOptions {
        max_attempts: 3,
        resume: false,
        reuse_verified: false,
        ..Default::default()
    };
    let cancel = never_cancel();
    let res = download(url, &tmp, &opts, &*cancel, |_, _| {});
    if let Err(e) = res {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    let text = std::fs::read_to_string(&tmp).map_err(|e| format!("INTERNAL|读取响应失败|{e}"))?;
    let _ = std::fs::remove_file(&tmp);
    serde_json::from_str(&text).map_err(|e| format!("INTERNAL|响应不是合法 JSON|{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_known_hosts() {
        assert_eq!(
            check_url("https://cdn.modrinth.com/data/x/y.jar").unwrap(),
            "cdn.modrinth.com"
        );
        assert!(check_url("https://bmclapi2.bangbang93.com/maven/a.jar").is_ok());
    }

    #[test]
    fn rejects_unsafe_urls() {
        assert!(check_url("http://cdn.modrinth.com/a.jar").is_err());
        assert!(check_url("https://evil.example.com/a.jar").is_err());
        assert!(check_url("https://127.0.0.1/a.jar").is_err());
        assert!(check_url("https://localhost/a.jar").is_err());
        assert!(check_url("https://user@cdn.modrinth.com/a.jar").is_err());
        assert!(check_url("file:///C:/a.jar").is_err());
        assert!(check_url("https://sub.cdn.modrinth.com/a.jar").is_err());
    }

    #[test]
    fn public_ip_classification() {
        for bad in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.1.1",
            "100.64.0.1",
            "0.0.0.0",
            "224.0.0.1",
            "240.0.0.1",
            "::1",
            "fe80::1",
            "fc00::1",
        ] {
            assert!(!is_public_ip(&bad.parse().unwrap()), "{bad} 不该被当成公网");
        }
        for good in ["1.1.1.1", "104.16.0.1", "2606:4700::1"] {
            assert!(is_public_ip(&good.parse().unwrap()), "{good} 应被当成公网");
        }
    }

    #[test]
    fn part_and_meta_paths() {
        let d = Path::new("C:/x/a.jar");
        assert_eq!(part_path(d), PathBuf::from("C:/x/a.jar.part"));
        assert_eq!(meta_path(d), PathBuf::from("C:/x/a.jar.part.json"));
    }

    #[test]
    fn disk_full_detection() {
        assert!(is_disk_full("IO|磁盘空间不足|os error 112"));
        assert!(is_disk_full("No space left on device"));
        assert!(!is_disk_full("NETWORK|连接超时"));
    }
}
