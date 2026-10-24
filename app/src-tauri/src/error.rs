//! 统一错误编码：`CODE|中文消息|可选细节`
//!
//! 前端 `bridge.ts` 按前两个 `|` 切开还原成 ApiError。
//! 命令一律返回 `Result<T, String>`，这里负责把错误编成那个字符串。
//!
//! 错误码常量是公开词表（文档与调用方引用），代码里多用字符串字面量拼接，
//! 因此允许部分常量暂时没有直接引用。

#![allow(dead_code)]

pub const NETWORK: &str = "NETWORK";
pub const DISK_FULL: &str = "DISK_FULL";
pub const PERMISSION: &str = "PERMISSION";
pub const INVALID_NAME: &str = "INVALID_NAME";
pub const VERSION_NOT_FOUND: &str = "VERSION_NOT_FOUND";
pub const INSTANCE_NOT_FOUND: &str = "INSTANCE_NOT_FOUND";
pub const INSTANCE_BUSY: &str = "INSTANCE_BUSY";
pub const JAVA_MISSING: &str = "JAVA_MISSING";
pub const VERIFY_FAILED: &str = "VERIFY_FAILED";
pub const AUTH_REQUIRED: &str = "AUTH_REQUIRED";
pub const CANCELLED: &str = "CANCELLED";
pub const BUSY: &str = "BUSY";
pub const UNSUPPORTED: &str = "UNSUPPORTED";
pub const INTERNAL: &str = "INTERNAL";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiError {
    pub code: String,
    pub message: String,
    pub detail: Option<String>,
}

impl ApiError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
            detail: None,
        }
    }

    pub fn with_detail(code: &str, message: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
            detail: Some(detail.into()),
        }
    }

    /// 编码成前端约定的单行字符串
    pub fn encode(&self) -> String {
        match &self.detail {
            Some(d) if !d.is_empty() => format!("{}|{}|{}", self.code, self.message, d),
            _ => format!("{}|{}", self.code, self.message),
        }
    }

    pub fn into_err<T>(self) -> Result<T, String> {
        Err(self.encode())
    }
}

pub type ApiResult<T> = Result<T, String>;

/// 快速构造 `Err`
pub fn fail<T>(code: &str, message: impl Into<String>) -> ApiResult<T> {
    ApiError::new(code, message).into_err()
}

pub fn fail_detail<T>(
    code: &str,
    message: impl Into<String>,
    detail: impl Into<String>,
) -> ApiResult<T> {
    ApiError::with_detail(code, message, detail).into_err()
}

/// IO 错误转编码错误
pub fn io<T>(r: std::io::Result<T>, message: impl Into<String>) -> ApiResult<T> {
    match r {
        Ok(v) => Ok(v),
        Err(e) => {
            let code = match e.kind() {
                std::io::ErrorKind::PermissionDenied => PERMISSION,
                std::io::ErrorKind::NotFound => INTERNAL,
                _ => INTERNAL,
            };
            ApiError::with_detail(code, message, e.to_string()).into_err()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_without_detail() {
        let e = ApiError::new(VERSION_NOT_FOUND, "找不到版本 1.20.4");
        assert_eq!(e.encode(), "VERSION_NOT_FOUND|找不到版本 1.20.4");
    }

    #[test]
    fn encode_with_detail() {
        let e = ApiError::with_detail(NETWORK, "下载失败", "io: timeout");
        assert_eq!(e.encode(), "NETWORK|下载失败|io: timeout");
    }

    #[test]
    fn detail_may_contain_separator() {
        let e = ApiError::with_detail(NETWORK, "下载失败", "a|b|c");
        let s = e.encode();
        let mut it = s.splitn(3, '|');
        assert_eq!(it.next(), Some("NETWORK"));
        assert_eq!(it.next(), Some("下载失败"));
        assert_eq!(it.next(), Some("a|b|c"));
    }
}
