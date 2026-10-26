//! 读游戏自己的 `servers.dat`，把玩家已经在游戏里存的服务器收进来。
//!
//! 玩家多半已经在「多人游戏」列表里存了一堆服。启动器这边另有一份收藏列表
//! （见 `servers.rs`），两边不通气的话，玩家得把地址一个个重新输一遍。
//!
//! 这里**只读**：`servers.dat` 是游戏内部格式（未压缩 NBT），我们读出来
//! 给界面看，玩家点了才写进自己的 `.astral-servers.json`。不碰 `servers.dat`
//! 一个字节 —— 那是游戏的地盘，写坏了游戏会丢列表。

use serde::Serialize;
use std::path::Path;

/// 游戏自带的服务器列表文件名（在实例目录根下）
pub const DAT_FILE: &str = "servers.dat";

/// `servers.dat` 里的一条
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FoundServer {
    /// 游戏里显示的名字（可能是空的）
    pub name: String,
    /// `host` 或 `host:port`
    pub ip: String,
    /// 游戏里被「隐藏」的服务器（1.20 之后有的标记）
    pub hidden: bool,
}

/// 扫描结果
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GameServerScan {
    /// `servers.dat` 的完整路径
    pub path: String,
    /// 文件在不在
    pub exists: bool,
    /// 读出来的服务器（按游戏里的顺序）
    pub found: Vec<FoundServer>,
    /// 读不出来时说明原因（文件不存在不算错）
    pub note: Option<String>,
}

fn text_of(v: &crate::nbt::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

fn flag_of(v: &crate::nbt::Value, key: &str) -> bool {
    v.get(key).and_then(|x| x.as_i64()).unwrap_or(0) != 0
}

/// 从一个已解析的 NBT 根里取服务器列表（拆出来方便单测）
pub fn servers_from_nbt(root: &crate::nbt::Value) -> Vec<FoundServer> {
    let Some(crate::nbt::Value::List(items)) = root.get("servers") else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let ip = text_of(item, "ip")?;
            if ip.trim().is_empty() {
                return None;
            }
            Some(FoundServer {
                name: text_of(item, "name").unwrap_or_default(),
                ip: ip.trim().to_string(),
                hidden: flag_of(item, "hidden"),
            })
        })
        .collect()
}

/// 扫描实例目录下的 `servers.dat`（只读）
pub fn scan(instance_dir: &Path) -> GameServerScan {
    let path = instance_dir.join(DAT_FILE);
    let mut out = GameServerScan {
        path: path.to_string_lossy().to_string(),
        exists: path.is_file(),
        found: Vec::new(),
        note: None,
    };
    if !out.exists {
        return out;
    }
    match crate::nbt::read_file(&path) {
        Ok(root) => {
            out.found = servers_from_nbt(&root);
            if out.found.is_empty() {
                out.note = Some("这个文件里没有服务器".to_string());
            }
        }
        Err(e) => {
            // 坏文件不报错，让界面提示一句就行 —— 这是游戏的格式，
            // 各版本之间可能不一样，读不懂不该挡住整个页面
            out.note = Some(format!(
                "读不出服务器列表（{}）",
                crate::task::decode(&e).message
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root_with(items: &[(&str, &str, i8)]) -> crate::nbt::Value {
        use crate::nbt::Value;
        use std::collections::BTreeMap;
        let servers: Vec<Value> = items
            .iter()
            .map(|(name, ip, hidden)| {
                let mut one = BTreeMap::new();
                one.insert("name".to_string(), Value::String(name.to_string()));
                one.insert("ip".to_string(), Value::String(ip.to_string()));
                one.insert("hidden".to_string(), Value::Byte(*hidden));
                Value::Compound(one)
            })
            .collect();
        let mut root = BTreeMap::new();
        root.insert("servers".to_string(), Value::List(servers));
        Value::Compound(root)
    }

    #[test]
    fn reads_servers_out_of_the_game_file() {
        let root = root_with(&[
            ("主服", "play.example.com", 0),
            ("", "mc.example.org:25566", 1),
        ]);
        let found = servers_from_nbt(&root);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "主服");
        assert_eq!(found[0].ip, "play.example.com");
        assert!(!found[0].hidden);
        assert_eq!(found[1].name, "");
        assert_eq!(found[1].ip, "mc.example.org:25566");
        assert!(found[1].hidden);
    }

    #[test]
    fn skips_entries_without_an_address() {
        let root = root_with(&[("空的", "", 0), ("好的", "a.example.com", 0)]);
        let found = servers_from_nbt(&root);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "好的");
    }

    #[test]
    fn missing_servers_key_is_not_an_error() {
        let root = crate::nbt::Value::Compound(std::collections::BTreeMap::new());
        assert!(servers_from_nbt(&root).is_empty());
    }

    #[test]
    fn scan_reports_missing_file_quietly() {
        let d = std::env::temp_dir().join(format!("astral-srvdat-{}", crate::time::now_nanos()));
        std::fs::create_dir_all(&d).unwrap();
        let r = scan(&d);
        assert!(!r.exists);
        assert!(r.found.is_empty());
        assert!(r.note.is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn scan_reads_a_written_file() {
        let d = std::env::temp_dir().join(format!("astral-srvdat2-{}", crate::time::now_nanos()));
        std::fs::create_dir_all(&d).unwrap();
        let root = root_with(&[("A", "a.example.com", 0)]);
        let bytes = crate::nbt::encode(&root);
        std::fs::write(d.join(DAT_FILE), bytes).unwrap();
        let r = scan(&d);
        assert!(r.exists);
        assert_eq!(r.found.len(), 1);
        assert_eq!(r.found[0].name, "A");
        assert!(r.note.is_none());
        let _ = std::fs::remove_dir_all(&d);
    }
}
