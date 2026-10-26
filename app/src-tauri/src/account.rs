//! 离线身份：用户名校验与 UUID 兼容。
//!
//! 背景：Minecraft 的离线 UUID 是 `UUID.nameUUIDFromBytes("OfflinePlayer:"+name)`，
//! 即 MD5(v3)。我们早期用的是 `Uuid::new_v3(NAMESPACE_OID, ...)`，多拼了命名空间
//! 前缀，算出来的 UUID 与标准不一致。
//!
//! 直接切换会让已有单人存档的 `playerdata/<uuid>.dat` 对不上（等价于换了个玩家），
//! 所以这里保留旧算法用于**检测**：实例存档里若存在旧 UUID 的 playerdata，
//! 就继续用旧身份，避免数据"丢档"；新实例则用标准算法。

use uuid::Uuid;

/// 旧版（错误）算法，仅用于识别历史存档
pub fn legacy_uuid(name: &str) -> Uuid {
    Uuid::new_v3(
        &Uuid::NAMESPACE_OID,
        format!("OfflinePlayer:{name}").as_bytes(),
    )
}

/// Minecraft 用户名规则：3–16 个 ASCII 字母/数字/下划线
pub fn validate_username(name: &str) -> Result<(), String> {
    let n = name.trim();
    if n.len() < 3 || n.len() > 16 {
        return Err("INVALID_NAME|用户名需为 3–16 个字符".into());
    }
    if !n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return Err("INVALID_NAME|用户名只能包含字母、数字和下划线".into());
    }
    Ok(())
}

/// 实例里是否存在旧 UUID 的玩家数据（存在就必须保持旧身份）
pub fn has_legacy_identity(saves_dir: &std::path::Path, username: &str) -> bool {
    let file = format!("{}.dat", legacy_uuid(username).hyphenated());
    let Ok(rd) = std::fs::read_dir(saves_dir) else {
        return false;
    };
    for e in rd.flatten() {
        let p = e.path().join("playerdata").join(&file);
        if p.is_file() {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn username_rules() {
        assert!(validate_username("Steve").is_ok());
        assert!(validate_username("a_b123").is_ok());
        assert!(validate_username("ab").is_err());
        assert!(validate_username("玩家").is_err());
        assert!(validate_username("bad name").is_err());
        assert!(validate_username("0123456789abcdefg").is_err());
    }

    #[test]
    fn legacy_uuid_is_stable() {
        assert_eq!(legacy_uuid("Steve"), legacy_uuid("Steve"));
        assert_ne!(legacy_uuid("Steve"), legacy_uuid("Alex"));
    }
}

/// Azure 应用（客户端）ID 必须是 GUID 形式
pub fn validate_client_id(id: &str) -> Result<(), String> {
    let s = id.trim();
    let parts: Vec<&str> = s.split('-').collect();
    let shape_ok = parts.len() == 5
        && parts
            .iter()
            .map(|p| p.len())
            .eq([8usize, 4, 4, 4, 12].iter().copied())
        && s.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    if shape_ok {
        Ok(())
    } else {
        Err("INVALID_VALUE|Azure 应用 ID 应为 GUID 格式".into())
    }
}

#[cfg(test)]
mod msa_tests {
    use super::*;

    #[test]
    fn client_id_shape() {
        assert!(validate_client_id("12345678-1234-1234-1234-123456789abc").is_ok());
        assert!(validate_client_id("not-a-guid").is_err());
        assert!(validate_client_id("").is_err());
    }
}
