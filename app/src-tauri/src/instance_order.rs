//! 实例的置顶与手动排序。
//!
//! `instance::discover` 一直是「按上次游玩时间倒序，同时间按名字」——玩家装了
//! 十几个实例之后，常用的那个会被新装的一路挤下去，每次都要在列表里找。
//!
//! 这里给玩家两个手段：
//! - **置顶**：常玩的几个钉在最上面，顺序按钉的先后；
//! - **手动排序**：拖一次就记住，之后按这个顺序排。
//!
//! 排序偏好单独存 `<game>/.astral-instance-order.json`，不写进每个实例的
//! `instance.json`：那是实例的"身份"，排序是列表的偏好，而且兼容实例
//! （`versions/` 下的老版本）根本没有 `instance.json` 可写。
//!
//! 文件里出现已经不存在的实例 id 不是错误（实例被删了而已），排序时直接忽略；
//! 但会顺手清理，免得文件无限长大。

use serde::{Deserialize, Serialize};
use std::path::Path;

/// 排序偏好文件（放在游戏根目录，和 `config.json` 平级）
pub const FILE: &str = ".astral-instance-order.json";

/// 最多记多少个 id。正常人装不到这么多，多出来的截掉防止文件被写爆。
const MAX_IDS: usize = 500;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OrderState {
    /// 置顶的实例 id，按置顶先后
    #[serde(default)]
    pub pinned: Vec<String>,
    /// 手动排序后的完整顺序
    #[serde(default)]
    pub order: Vec<String>,
}

fn path_of(game: &Path) -> std::path::PathBuf {
    game.join(FILE)
}

/// 读排序偏好。文件不存在、读不出来、格式不对，一律当"没设过"。
pub fn load(game: &Path) -> OrderState {
    let Ok(text) = std::fs::read_to_string(path_of(game)) else {
        return OrderState::default();
    };
    let mut st = serde_json::from_str::<OrderState>(&text).unwrap_or_default();
    dedup(&mut st.pinned);
    dedup(&mut st.order);
    st
}

fn dedup(ids: &mut Vec<String>) {
    let mut seen = std::collections::HashSet::new();
    ids.retain(|id| !id.is_empty() && seen.insert(id.clone()));
    ids.truncate(MAX_IDS);
}

fn save(game: &Path, st: &OrderState) -> Result<(), String> {
    let text = serde_json::to_string_pretty(st).map_err(|e| format!("INTERNAL|序列化失败|{e}"))?;
    let path = path_of(game);
    let tmp = path.with_extension("json.part");
    std::fs::write(&tmp, text).map_err(|e| format!("PERMISSION|写入排序失败|{e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("PERMISSION|保存排序失败|{e}")
    })
}

/// 置顶 / 取消置顶。重复置顶不会重复进列表。
pub fn pin(game: &Path, id: &str, pinned: bool) -> Result<OrderState, String> {
    if id.trim().is_empty() {
        return Err("INVALID_VALUE|实例 id 不能为空".into());
    }
    let mut st = load(game);
    st.pinned.retain(|x| x != id);
    if pinned {
        st.pinned.insert(0, id.to_string());
    }
    dedup(&mut st.pinned);
    save(game, &st)?;
    Ok(st)
}

/// 记下一次手动排序。只保留实际存在的 id，避免删了实例之后文件越积越多。
pub fn reorder(game: &Path, ids: &[String], known: &[String]) -> Result<OrderState, String> {
    let mut st = load(game);
    let known_set: std::collections::HashSet<String> = known.iter().cloned().collect();
    let mut next: Vec<String> = Vec::new();
    for id in ids {
        if id.trim().is_empty() || !known_set.contains(id) {
            continue;
        }
        if !next.contains(id) {
            next.push(id.clone());
        }
    }
    // 没被提到的实例接在后面，保持它们原来的相对顺序
    for id in known {
        if !next.contains(id) {
            next.push(id.clone());
        }
    }
    dedup(&mut next);
    st.order = next;
    st.pinned.retain(|id| known.contains(id));
    save(game, &st)?;
    Ok(st)
}

/// 按偏好排序。不认识的 id 直接跳过。
///
/// 优先级：置顶（按置顶先后）→ 手动顺序 → 剩下的保持传进来的顺序。
pub fn apply<T, F>(items: Vec<T>, st: &OrderState, id_of: F) -> Vec<T>
where
    F: Fn(&T) -> String,
{
    if st.pinned.is_empty() && st.order.is_empty() {
        return items;
    }
    let mut buckets: Vec<Vec<T>> = Vec::new();
    let mut rest: Vec<T> = Vec::new();
    // 给每个置顶 id 开一个桶，保持置顶顺序
    let mut pinned_buckets: Vec<(String, Vec<T>)> = st
        .pinned
        .iter()
        .map(|id| (id.clone(), Vec::new()))
        .collect();
    let mut ordered: Vec<(String, Vec<T>)> =
        st.order.iter().map(|id| (id.clone(), Vec::new())).collect();

    for item in items {
        let id = id_of(&item);
        if let Some(slot) = pinned_buckets.iter_mut().find(|(pid, _)| *pid == id) {
            slot.1.push(item);
        } else if let Some(slot) = ordered.iter_mut().find(|(oid, _)| *oid == id) {
            slot.1.push(item);
        } else {
            rest.push(item);
        }
    }

    for (_, mut group) in pinned_buckets {
        buckets.push(std::mem::take(&mut group));
    }
    for (_, mut group) in ordered {
        if !group.is_empty() {
            buckets.push(std::mem::take(&mut group));
        }
    }
    let mut out: Vec<T> = Vec::new();
    for mut b in buckets {
        out.append(&mut b);
    }
    out.append(&mut rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d =
            std::env::temp_dir().join(format!("astral-order-{tag}-{:x}", crate::time::now_nanos()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn missing_file_is_empty() {
        let d = tmp("missing");
        assert_eq!(load(&d), OrderState::default());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn pins_move_to_the_front() {
        let d = tmp("pin");
        pin(&d, "b", true).unwrap();
        pin(&d, "c", true).unwrap();
        let st = load(&d);
        // 最近钉的排最前
        assert_eq!(st.pinned, ids(&["c", "b"]));
        let out = apply(ids(&["a", "b", "c", "d"]), &st, |s| s.clone());
        assert_eq!(out, ids(&["c", "b", "a", "d"]));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn unpin_puts_it_back_in_place() {
        let d = tmp("unpin");
        pin(&d, "b", true).unwrap();
        pin(&d, "b", false).unwrap();
        let st = load(&d);
        assert!(st.pinned.is_empty());
        assert_eq!(
            apply(ids(&["a", "b", "c"]), &st, |s| s.clone()),
            ids(&["a", "b", "c"])
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn pinning_twice_does_not_duplicate() {
        let d = tmp("twice");
        pin(&d, "x", true).unwrap();
        pin(&d, "x", true).unwrap();
        assert_eq!(load(&d).pinned, ids(&["x"]));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn reorder_keeps_unmentioned_at_the_end() {
        let d = tmp("reorder");
        let known = ids(&["a", "b", "c", "d"]);
        reorder(&d, &ids(&["d", "b"]), &known).unwrap();
        let st = load(&d);
        let out = apply(known.clone(), &st, |s| s.clone());
        assert_eq!(out, ids(&["d", "b", "a", "c"]));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn reorder_drops_unknown_ids() {
        let d = tmp("unknown");
        let known = ids(&["a", "b"]);
        reorder(&d, &ids(&["ghost", "b"]), &known).unwrap();
        assert_eq!(load(&d).order, ids(&["b", "a"]));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn broken_file_reads_as_empty() {
        let d = tmp("broken");
        std::fs::write(d.join(FILE), b"{oops").unwrap();
        assert_eq!(load(&d), OrderState::default());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn apply_is_stable_without_preferences() {
        let d = tmp("stable");
        let st = load(&d);
        let input = ids(&["z", "a", "m"]);
        assert_eq!(apply(input.clone(), &st, |s| s.clone()), input);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn pinned_wins_over_manual_order() {
        let d = tmp("both");
        let known = ids(&["a", "b", "c"]);
        reorder(&d, &ids(&["c", "a", "b"]), &known).unwrap();
        pin(&d, "b", true).unwrap();
        let st = load(&d);
        assert_eq!(
            apply(known.clone(), &st, |s| s.clone()),
            ids(&["b", "c", "a"])
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}
