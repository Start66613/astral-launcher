//! 收藏的服务器列表。
//!
//! 玩家常去的几个服，每次都要重新输地址太麻烦。这里存一份自己的清单，
//! 顺便记名字和备注。
//!
//! **不动 Minecraft 自己的 `servers.dat`**：那是游戏内部格式（NBT），
//! 改了容易和游戏打架。我们用 `<game>/.astral-servers.json`，各管各的。
//!
//! 清单里的地址不存端口，`host` 和 `port` 分开存 —— 这样改端口不用重写字符串，
//! 排序也能保持稳定。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// 清单文件名。前面带点，和玩家的东西分开放。
pub const LIST_FILE: &str = ".astral-servers.json";
/// 最多收藏多少个。再多前端也没法用，还容易误操作。
pub const MAX_SERVERS: usize = 200;
/// 名字最长多少字符。
pub const MAX_NAME_LEN: usize = 48;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ServerEntry {
    /// `s_<8 位十六进制>`
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    /// 备注，玩家自己写的
    #[serde(default)]
    pub note: String,
    /// 加进来的时间（ISO）
    pub added_at: String,
    /// 最近一次成功查到状态的时间（ISO）；从没成功过就是 None
    #[serde(default)]
    pub last_seen: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
struct ListFile {
    #[serde(default)]
    servers: Vec<ServerEntry>,
}

fn list_path(game: &Path) -> std::path::PathBuf {
    game.join(LIST_FILE)
}

fn load_raw(game: &Path) -> ListFile {
    let Ok(text) = std::fs::read_to_string(list_path(game)) else {
        return ListFile::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

fn save(game: &Path, file: &ListFile) -> Result<(), String> {
    std::fs::create_dir_all(game).map_err(|e| format!("PERMISSION|创建目录失败|{e}"))?;
    let text =
        serde_json::to_string_pretty(file).map_err(|e| format!("INTERNAL|序列化失败|{e}"))?;
    let target = list_path(game);
    let part = target.with_extension("json.part");
    std::fs::write(&part, text.as_bytes()).map_err(|e| format!("PERMISSION|写入失败|{e}"))?;
    std::fs::rename(&part, &target).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        format!("PERMISSION|保存失败|{e}")
    })
}

/// 生成一个不重复的 id。
fn new_id(existing: &[ServerEntry]) -> String {
    use sha1::{Digest, Sha1};
    let mut salt = 0u32;
    loop {
        let mut h = Sha1::new();
        h.update(crate::time::now_nanos().to_le_bytes());
        h.update(salt.to_le_bytes());
        let digest = h.finalize();
        let hex: String = digest.iter().take(4).map(|b| format!("{b:02x}")).collect();
        let id = format!("s_{hex}");
        if !existing.iter().any(|e| e.id == id) {
            return id;
        }
        salt += 1;
    }
}

fn clean_name(name: &str) -> String {
    let trimmed: String = name.trim().chars().take(MAX_NAME_LEN).collect();
    trimmed
}

/// 校验并规范化地址，返回 (host, port)。
///
/// 允许玩家直接粘贴 `example.com:25566`，端口拆出来单独存。
fn normalize_addr(host: &str, port: Option<u16>) -> Result<(String, u16), String> {
    let (raw_host, inline_port) = crate::server::split_address(host);
    let h = raw_host.trim().trim_end_matches('.').to_string();
    if h.is_empty() {
        return Err("INVALID_VALUE|服务器地址不能为空".to_string());
    }
    if h.chars().count() > crate::server::MAX_HOST_LEN
        || h.chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '/' || c == '\\')
    {
        return Err("INVALID_VALUE|服务器地址不合法".to_string());
    }
    let p = port.or(inline_port).unwrap_or(crate::server::DEFAULT_PORT);
    if p == 0 {
        return Err("INVALID_VALUE|端口必须在 1-65535 之间".to_string());
    }
    Ok((h, p))
}

/// 全部收藏，按玩家排的顺序。
pub fn list(game: &Path) -> Vec<ServerEntry> {
    load_raw(game).servers
}

/// 加一个服务器。同一个地址（host + port）不重复加。
pub fn add(
    game: &Path,
    name: &str,
    host: &str,
    port: Option<u16>,
    note: &str,
) -> Result<ServerEntry, String> {
    let (h, p) = normalize_addr(host, port)?;
    let mut file = load_raw(game);
    if file.servers.len() >= MAX_SERVERS {
        return Err(format!(
            "INVALID_VALUE|最多收藏 {MAX_SERVERS} 个服务器，先删掉几个"
        ));
    }
    if file.servers.iter().any(|e| e.host == h && e.port == p) {
        return Err("INVALID_VALUE|这个服务器已经在列表里了".to_string());
    }
    let entry = ServerEntry {
        id: new_id(&file.servers),
        name: {
            let n = clean_name(name);
            if n.is_empty() {
                format!("{h}:{p}")
            } else {
                n
            }
        },
        host: h,
        port: p,
        note: note.trim().chars().take(200).collect(),
        added_at: crate::time::now_iso(),
        last_seen: None,
    };
    file.servers.push(entry.clone());
    save(game, &file)?;
    Ok(entry)
}

/// 改名字 / 地址 / 备注。传 `None` 的字段保持不变。
pub fn update(
    game: &Path,
    id: &str,
    name: Option<&str>,
    host: Option<&str>,
    port: Option<u16>,
    note: Option<&str>,
) -> Result<ServerEntry, String> {
    let mut file = load_raw(game);
    let idx = file
        .servers
        .iter()
        .position(|e| e.id == id)
        .ok_or_else(|| "INVALID_VALUE|这个服务器不在列表里".to_string())?;

    // 先算出改完之后的地址，用来查重（改回自己原来的地址不算冲突）
    let (cur_host, cur_port) = (file.servers[idx].host.clone(), file.servers[idx].port);
    let (h, p) = match host {
        Some(h) => normalize_addr(h, port)?,
        None => (cur_host.clone(), port.unwrap_or(cur_port)),
    };
    if p == 0 {
        return Err("INVALID_VALUE|端口必须在 1-65535 之间".to_string());
    }
    let dup = file
        .servers
        .iter()
        .enumerate()
        .any(|(i, e)| i != idx && e.host == h && e.port == p);
    if dup {
        return Err("INVALID_VALUE|这个服务器已经在列表里了".to_string());
    }

    {
        let e = &mut file.servers[idx];
        e.host = h;
        e.port = p;
        if let Some(n) = name {
            let cleaned = clean_name(n);
            if !cleaned.is_empty() {
                e.name = cleaned;
            }
        }
        if let Some(n) = note {
            e.note = n.trim().chars().take(200).collect();
        }
    }
    let out = file.servers[idx].clone();
    save(game, &file)?;
    Ok(out)
}

/// 删掉一个。只删清单里的记录，不碰任何文件。
pub fn remove(game: &Path, id: &str) -> Result<(), String> {
    let mut file = load_raw(game);
    let before = file.servers.len();
    file.servers.retain(|e| e.id != id);
    if file.servers.len() == before {
        return Err("INVALID_VALUE|这个服务器不在列表里".to_string());
    }
    save(game, &file)
}

/// 按给定顺序重排。`ids` 里没提到的保持原相对顺序排在后面。
pub fn reorder(game: &Path, ids: &[String]) -> Result<Vec<ServerEntry>, String> {
    let mut file = load_raw(game);
    let mut rank: BTreeMap<&str, usize> = BTreeMap::new();
    for (i, id) in ids.iter().enumerate() {
        rank.insert(id.as_str(), i);
    }
    let original: BTreeMap<String, usize> = file
        .servers
        .iter()
        .enumerate()
        .map(|(i, e)| (e.id.clone(), i))
        .collect();
    file.servers.sort_by_key(|e| {
        rank.get(e.id.as_str())
            .copied()
            .unwrap_or(usize::MAX / 2 + original.get(&e.id).copied().unwrap_or(0))
    });
    save(game, &file)?;
    Ok(file.servers)
}

/// 记一次"查到了"。查不到时不写，避免把失败也当成见过。
pub fn mark_seen(game: &Path, id: &str) -> Result<(), String> {
    let mut file = load_raw(game);
    let idx = file
        .servers
        .iter()
        .position(|e| e.id == id)
        .ok_or_else(|| "INVALID_VALUE|这个服务器不在列表里".to_string())?;
    file.servers[idx].last_seen = Some(crate::time::now_iso());
    save(game, &file)
}

/// 一次批量刷新：把清单里（或指定的几个）服务器并行查一遍。
///
/// 并发上限 8 —— 服务器列表协议很轻，但一口气开几百个连接容易被对面当攻击。
pub fn refresh(
    game: &Path,
    ids: Option<Vec<String>>,
) -> Vec<(String, crate::server::ServerStatus)> {
    const MAX_PARALLEL: usize = 8;

    let entries: Vec<ServerEntry> = match ids {
        Some(wanted) => load_raw(game)
            .servers
            .into_iter()
            .filter(|e| wanted.iter().any(|w| w == &e.id))
            .collect(),
        None => load_raw(game).servers,
    };
    if entries.is_empty() {
        return Vec::new();
    }

    let total = entries.len();
    let (tx, rx) = std::sync::mpsc::channel::<(String, crate::server::ServerStatus)>();
    let queue = std::sync::Arc::new(std::sync::Mutex::new(entries.into_iter()));
    // 线程里要用，先转成 owned，否则借用活不过 spawn
    let game_owned = game.to_path_buf();

    let workers = MAX_PARALLEL.min(total);
    for _ in 0..workers {
        let queue = queue.clone();
        let tx = tx.clone();
        let game = game_owned.clone();
        std::thread::spawn(move || loop {
            let next = {
                let mut q = match queue.lock() {
                    Ok(q) => q,
                    Err(_) => return,
                };
                q.next()
            };
            let Some(entry) = next else { return };
            let status = crate::server::ping(&entry.host, Some(entry.port), None)
                .unwrap_or_else(|e| crate::server::offline_from_error(&entry.host, entry.port, e));
            if status.online {
                let _ = mark_seen(&game, &entry.id);
            }
            if tx.send((entry.id, status)).is_err() {
                return;
            }
        });
    }
    drop(tx);

    let mut out = Vec::with_capacity(total);
    for item in rx {
        out.push(item);
    }
    // 按清单顺序输出，前端不用再排
    let order: Vec<String> = load_raw(game).servers.into_iter().map(|e| e.id).collect();
    out.sort_by_key(|(id, _)| order.iter().position(|o| o == id).unwrap_or(usize::MAX));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d =
            std::env::temp_dir().join(format!("astral-servers-{}-{tag}", crate::time::now_nanos()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn adds_and_lists_in_order() {
        let d = tmp("add");
        let a = add(&d, "主服", "play.example.com:25566", None, "朋友开的").unwrap();
        assert_eq!(a.host, "play.example.com");
        assert_eq!(a.port, 25566);
        assert_eq!(a.name, "主服");
        assert!(a.id.starts_with("s_"));
        let b = add(&d, "", "mc.example.org", None, "").unwrap();
        // 没给名字就用地址兜底
        assert_eq!(b.name, "mc.example.org:25565");
        let list = list(&d);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, a.id);
        assert_eq!(list[1].id, b.id);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn rejects_duplicates_and_bad_addresses() {
        let d = tmp("dup");
        add(&d, "a", "play.example.com", None, "").unwrap();
        // 同一个地址（带不带端口）都算重复
        assert!(add(&d, "b", "play.example.com", Some(25565), "").is_err());
        assert!(add(&d, "c", "", None, "").is_err());
        assert!(add(&d, "d", "bad host", None, "").is_err());
        assert!(add(&d, "e", "x", Some(0), "").is_err());
        assert_eq!(list(&d).len(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn updates_fields_selectively() {
        let d = tmp("update");
        let a = add(&d, "old", "one.example.com", None, "n1").unwrap();
        let b = update(&d, &a.id, Some("new"), None, Some(25599), Some("n2")).unwrap();
        assert_eq!(b.name, "new");
        assert_eq!(b.port, 25599);
        assert_eq!(b.note, "n2");
        // 只改名字时地址不能变
        let c = update(&d, &a.id, Some("newer"), None, None, None).unwrap();
        assert_eq!(c.host, "one.example.com");
        assert_eq!(c.port, 25599);
        assert_eq!(c.note, "n2");
        // 空名字不能把已有名字清掉
        let e = update(&d, &a.id, Some("   "), None, None, None).unwrap();
        assert_eq!(e.name, "newer");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn update_rejects_conflict_with_other_entry() {
        let d = tmp("conflict");
        let a = add(&d, "a", "one.example.com", None, "").unwrap();
        add(&d, "b", "two.example.com", None, "").unwrap();
        assert!(update(&d, &a.id, None, Some("two.example.com"), None, None).is_err());
        // 改回自己原来的地址不算冲突
        assert!(update(&d, &a.id, None, Some("one.example.com"), None, None).is_ok());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn removes_only_the_given_entry() {
        let d = tmp("remove");
        let a = add(&d, "a", "one.example.com", None, "").unwrap();
        let b = add(&d, "b", "two.example.com", None, "").unwrap();
        remove(&d, &a.id).unwrap();
        let list = list(&d);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, b.id);
        // 再删一次要报错，不能静默成功
        assert!(remove(&d, &a.id).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn reorders_by_given_ids() {
        let d = tmp("reorder");
        let a = add(&d, "a", "one.example.com", None, "").unwrap();
        let b = add(&d, "b", "two.example.com", None, "").unwrap();
        let c = add(&d, "c", "three.example.com", None, "").unwrap();
        let out = reorder(&d, &[c.id.clone(), a.id.clone()]).unwrap();
        // 提到的按给定顺序在前，没提到的（b）排后面
        assert_eq!(out[0].id, c.id);
        assert_eq!(out[1].id, a.id);
        assert_eq!(out[2].id, b.id);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn mark_seen_stamps_time() {
        let d = tmp("seen");
        let a = add(&d, "a", "one.example.com", None, "").unwrap();
        assert!(a.last_seen.is_none());
        mark_seen(&d, &a.id).unwrap();
        let after = list(&d);
        assert!(after[0].last_seen.is_some());
        assert!(mark_seen(&d, "s_deadbeef").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn missing_file_reads_as_empty() {
        let d = tmp("empty");
        assert!(list(&d).is_empty());
        // 坏文件不能把整个列表搞崩，读成空就行
        std::fs::write(d.join(LIST_FILE), "{not json").unwrap();
        assert!(list(&d).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn caps_name_length() {
        let d = tmp("cap");
        let long = "服".repeat(100);
        let a = add(&d, &long, "one.example.com", None, "").unwrap();
        assert_eq!(a.name.chars().count(), MAX_NAME_LEN);
        let _ = std::fs::remove_dir_all(&d);
    }
}
