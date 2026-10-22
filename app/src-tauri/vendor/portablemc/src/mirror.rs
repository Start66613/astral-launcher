//! [Astral patch] 下载源映射。
//!
//! 只改「文件下载」这一层：依赖库和资源文件走 BMCLAPI 国内镜像，
//! 版本清单 / 元数据 / 客户端 jar 仍走官方（体积小，且镜像路径要靠版本号才能拼）。
//!
//! 开关是全局的，由外层在每次安装前调用 `set_mirror`。

use std::sync::atomic::{AtomicBool, Ordering};

static USE_MIRROR: AtomicBool = AtomicBool::new(false);

const BMCL: &str = "https://bmclapi2.bangbang93.com";

pub fn set_mirror(on: bool) {
    USE_MIRROR.store(on, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    USE_MIRROR.load(Ordering::Relaxed)
}

/// 把官方地址改写成镜像地址；不认识的地址原样返回。
pub fn map(url: &str) -> String {
    if !enabled() {
        return url.to_string();
    }

    if let Some(rest) = url.strip_prefix("https://libraries.minecraft.net/") {
        return format!("{BMCL}/maven/{rest}");
    }

    if let Some(rest) = url.strip_prefix("https://resources.download.minecraft.net/") {
        return format!("{BMCL}/assets/{rest}");
    }

    if url.ends_with("/mc/game/version_manifest_v2.json") {
        return format!("{BMCL}/mc/game/version_manifest_v2.json");
    }

    url.to_string()
}
