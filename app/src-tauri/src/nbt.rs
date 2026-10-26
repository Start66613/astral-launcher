//! 极简 NBT 读取器（只读，够用就好）。
//!
//! 用途：`level.dat` 是 gzip 压缩的 NBT，里面有这个存档是哪个版本建的、
//! 是不是硬核、什么难度、上次玩的时间。原来存档列表里这些全是 `null`，
//! 前端只能显示一个目录名，玩家分不清哪个是哪个。
//!
//! 这里不做完整实现（不写回、不碰 region 文件），只把标签树读出来，
//! 并且对所有长度字段都设了上限——`level.dat` 来自用户磁盘，可能被改坏。

use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

/// 嵌套层数上限。正常存档不到 10 层，64 已经非常宽松。
const MAX_DEPTH: u32 = 64;
/// 单个字符串上限（玩家自己起的中文名远小于这个数）
const MAX_STRING: usize = 1 << 20;
/// 一个 list / byte array 的元素上限
const MAX_ELEMENTS: usize = 1 << 22;
/// 解压后总字节上限。真实 level.dat 通常几十 KB。
const MAX_DECOMPRESSED: u64 = 64 << 20;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<u8>),
    String(String),
    List(Vec<Value>),
    Compound(BTreeMap<String, Value>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl Value {
    /// 按名字取子标签；不是 compound 就是 None
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Compound(m) => m.get(key),
            _ => None,
        }
    }

    /// 逐层取，`path(&["Data", "Version", "Name"])`
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn path(&self, keys: &[&str]) -> Option<&Value> {
        let mut cur = self;
        for k in keys {
            cur = cur.get(k)?;
        }
        Some(cur)
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Byte(v) => Some(*v as i64),
            Value::Short(v) => Some(*v as i64),
            Value::Int(v) => Some(*v as i64),
            Value::Long(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        self.as_i64().map(|v| v != 0)
    }
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    depth: u32,
}

type R<T> = Result<T, String>;

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            depth: 0,
        }
    }

    fn need(&self, n: usize) -> R<()> {
        if self.pos + n > self.data.len() {
            Err(format!(
                "INVALID_NBT|数据被截断（需要 {} 字节，剩 {}）",
                n,
                self.data.len() - self.pos
            ))
        } else {
            Ok(())
        }
    }

    fn u8(&mut self) -> R<u8> {
        self.need(1)?;
        let v = self.data[self.pos];
        self.pos += 1;
        Ok(v)
    }

    fn take(&mut self, n: usize) -> R<&'a [u8]> {
        self.need(n)?;
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    fn i16(&mut self) -> R<i16> {
        let b = self.take(2)?;
        Ok(i16::from_be_bytes([b[0], b[1]]))
    }

    fn i32(&mut self) -> R<i32> {
        let b = self.take(4)?;
        Ok(i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn i64(&mut self) -> R<i64> {
        let b = self.take(8)?;
        Ok(i64::from_be_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    fn string(&mut self) -> R<String> {
        let len = self.i16()?;
        if len < 0 {
            return Err("INVALID_NBT|字符串长度为负".into());
        }
        let len = len as usize;
        if len > MAX_STRING {
            return Err(format!("INVALID_NBT|字符串过长（{len} 字节）"));
        }
        let b = self.take(len)?;
        // 合法的修改版 UTF-8 也可能是坏字节；不 panic，坏字节替换掉即可
        Ok(String::from_utf8_lossy(b).into_owned())
    }

    fn elements(&mut self, len: i32) -> R<usize> {
        if len < 0 {
            return Err("INVALID_NBT|集合长度为负".into());
        }
        let len = len as usize;
        if len > MAX_ELEMENTS {
            return Err(format!("INVALID_NBT|集合过大（{len} 个元素）"));
        }
        Ok(len)
    }

    /// 读一个带类型标签的 payload（不含名字）
    fn payload(&mut self, tag: u8) -> R<Value> {
        match tag {
            1 => Ok(Value::Byte(self.u8()? as i8)),
            2 => Ok(Value::Short(self.i16()?)),
            3 => Ok(Value::Int(self.i32()?)),
            4 => Ok(Value::Long(self.i64()?)),
            5 => Ok(Value::Float(f32::from_bits(self.i32()? as u32))),
            6 => Ok(Value::Double(f64::from_bits(self.i64()? as u64))),
            7 => {
                let len = self.i32()?;
                let n = self.elements(len)?;
                Ok(Value::ByteArray(self.take(n)?.to_vec()))
            }
            8 => Ok(Value::String(self.string()?)),
            9 => {
                let item = self.u8()?;
                let len = self.i32()?;
                let n = self.elements(len)?;
                let mut out = Vec::with_capacity(n.min(4096));
                for _ in 0..n {
                    out.push(self.payload(item)?);
                }
                Ok(Value::List(out))
            }
            10 => {
                if self.depth >= MAX_DEPTH {
                    return Err(format!("INVALID_NBT|嵌套超过 {MAX_DEPTH} 层"));
                }
                self.depth += 1;
                let mut map = BTreeMap::new();
                loop {
                    let t = self.u8()?;
                    if t == 0 {
                        break;
                    }
                    let name = self.string()?;
                    let v = self.payload(t)?;
                    map.insert(name, v);
                }
                self.depth -= 1;
                Ok(Value::Compound(map))
            }
            11 => {
                let len = self.i32()?;
                let n = self.elements(len)?;
                let mut out = Vec::with_capacity(n.min(4096));
                for _ in 0..n {
                    out.push(self.i32()?);
                }
                Ok(Value::IntArray(out))
            }
            12 => {
                let len = self.i32()?;
                let n = self.elements(len)?;
                let mut out = Vec::with_capacity(n.min(4096));
                for _ in 0..n {
                    out.push(self.i64()?);
                }
                Ok(Value::LongArray(out))
            }
            other => Err(format!("INVALID_NBT|未知标签类型 {other}")),
        }
    }
}

/// 解析一段未压缩的 NBT 数据，返回根 compound
pub fn parse(data: &[u8]) -> R<Value> {
    let mut r = Reader::new(data);
    let tag = r.u8()?;
    if tag != 10 {
        return Err(format!("INVALID_NBT|根标签不是 compound（类型 {tag}）"));
    }
    let _root_name = r.string()?;
    r.payload(10)
}

fn gunzip(data: &[u8]) -> R<Vec<u8>> {
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(data)
        .take(MAX_DECOMPRESSED)
        .read_to_end(&mut out)
        .map_err(|e| format!("INVALID_NBT|解压失败：{e}"))?;
    Ok(out)
}

/// 读任意 NBT 文件；gzip 的自动解压，未压缩的也能读
pub fn read_file(path: &Path) -> R<Value> {
    let raw = std::fs::read(path).map_err(|e| format!("IO_ERROR|读取失败：{e}"))?;
    if raw.is_empty() {
        return Err("INVALID_NBT|文件是空的".into());
    }
    // gzip 魔数 1f 8b；不是就当成裸 NBT 试一次
    let bytes = if raw[0] == 0x1f && raw.get(1) == Some(&0x8b) {
        gunzip(&raw)?
    } else {
        raw
    };
    parse(&bytes)
}

/// 存档里我们关心的那点信息。任何一项读不到都是 `None`，不猜。
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LevelInfo {
    /// 存档自报的名字（可能和目录名不一样）
    pub level_name: Option<String>,
    /// 建这个存档时的游戏版本，例如 `1.20.1`
    pub version_name: Option<String>,
    /// 版本号（3465 这种），版本名缺失时前端可以拿它对照
    pub version_id: Option<i32>,
    /// 0 生存 / 1 创造 / 2 冒险 / 3 观战
    pub game_mode: Option<String>,
    /// 0 和平 / 1 简单 / 2 普通 / 3 困难
    pub difficulty: Option<String>,
    pub hardcore: Option<bool>,
    pub allow_commands: Option<bool>,
    /// 上次游玩时间（ISO8601，UTC）。比文件 mtime 准，mtime 会被复制/解压改掉
    pub last_played: Option<String>,
    pub data_version: Option<i32>,
}

fn game_mode_name(v: i64) -> String {
    match v {
        0 => "生存",
        1 => "创造",
        2 => "冒险",
        3 => "观战",
        other => return format!("未知({other})"),
    }
    .to_string()
}

fn difficulty_name(v: i64) -> String {
    match v {
        0 => "和平",
        1 => "简单",
        2 => "普通",
        3 => "困难",
        other => return format!("未知({other})"),
    }
    .to_string()
}

impl LevelInfo {
    /// 从根 compound 里取 `Data` 那一层
    pub fn from_nbt(root: &Value) -> Self {
        let data = root.get("Data").unwrap_or(root);
        let version = data.get("Version");
        Self {
            level_name: data
                .get("LevelName")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            version_name: version
                .and_then(|v| v.get("Name"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            version_id: version
                .and_then(|v| v.get("Id"))
                .and_then(|v| v.as_i64())
                .map(|v| v as i32),
            game_mode: data
                .get("GameType")
                .and_then(|v| v.as_i64())
                .map(game_mode_name),
            difficulty: data
                .get("Difficulty")
                .and_then(|v| v.as_i64())
                .map(difficulty_name),
            hardcore: data.get("hardcore").and_then(|v| v.as_bool()),
            allow_commands: data.get("allowCommands").and_then(|v| v.as_bool()),
            last_played: data
                .get("LastPlayed")
                .and_then(|v| v.as_i64())
                .filter(|ms| *ms > 0)
                .map(crate::time::unix_ms_to_iso),
            data_version: data
                .get("DataVersion")
                .and_then(|v| v.as_i64())
                .map(|v| v as i32),
        }
    }

    /// 读 `<world>/level.dat`。文件不存在或读不懂都返回 `None`——
    /// 存档列表不该因为一个坏文件整页失败。
    pub fn read(world_dir: &Path) -> Option<Self> {
        let root = read_file(&world_dir.join("level.dat")).ok()?;
        Some(Self::from_nbt(&root))
    }

    /// 存档是否至少像个存档（有 Data 层）
    pub fn looks_valid(&self) -> bool {
        self.version_name.is_some() || self.data_version.is_some() || self.level_name.is_some()
    }
}

// ── 写入（只给测试和自检造 fixture 用）──────────────────────────────

fn write_string(out: &mut Vec<u8>, s: &str) {
    let b = s.as_bytes();
    out.extend_from_slice(&(b.len() as i16).to_be_bytes());
    out.extend_from_slice(b);
}

fn encode_payload(out: &mut Vec<u8>, v: &Value) {
    match v {
        Value::Byte(x) => out.push(*x as u8),
        Value::Short(x) => out.extend_from_slice(&x.to_be_bytes()),
        Value::Int(x) => out.extend_from_slice(&x.to_be_bytes()),
        Value::Long(x) => out.extend_from_slice(&x.to_be_bytes()),
        Value::Float(x) => out.extend_from_slice(&x.to_bits().to_be_bytes()),
        Value::Double(x) => out.extend_from_slice(&x.to_bits().to_be_bytes()),
        Value::ByteArray(a) => {
            out.extend_from_slice(&(a.len() as i32).to_be_bytes());
            out.extend_from_slice(a);
        }
        Value::String(s) => write_string(out, s),
        Value::List(items) => {
            let tag = items.first().map(tag_of).unwrap_or(0);
            out.push(tag);
            out.extend_from_slice(&(items.len() as i32).to_be_bytes());
            for i in items {
                encode_payload(out, i);
            }
        }
        Value::Compound(m) => {
            for (k, val) in m {
                out.push(tag_of(val));
                write_string(out, k);
                encode_payload(out, val);
            }
            out.push(0);
        }
        Value::IntArray(a) => {
            out.extend_from_slice(&(a.len() as i32).to_be_bytes());
            for x in a {
                out.extend_from_slice(&x.to_be_bytes());
            }
        }
        Value::LongArray(a) => {
            out.extend_from_slice(&(a.len() as i32).to_be_bytes());
            for x in a {
                out.extend_from_slice(&x.to_be_bytes());
            }
        }
    }
}

fn tag_of(v: &Value) -> u8 {
    match v {
        Value::Byte(_) => 1,
        Value::Short(_) => 2,
        Value::Int(_) => 3,
        Value::Long(_) => 4,
        Value::Float(_) => 5,
        Value::Double(_) => 6,
        Value::ByteArray(_) => 7,
        Value::String(_) => 8,
        Value::List(_) => 9,
        Value::Compound(_) => 10,
        Value::IntArray(_) => 11,
        Value::LongArray(_) => 12,
    }
}

/// 把标签树编码成未压缩 NBT（根必须是 compound）
#[doc(hidden)]
pub fn encode(root: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(10);
    write_string(&mut out, "");
    encode_payload(&mut out, root);
    out
}

/// 编码并 gzip 压缩，和真实 `level.dat` 一样
#[doc(hidden)]
pub fn encode_gzip(root: &Value) -> Vec<u8> {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;
    let mut e = GzEncoder::new(Vec::new(), Compression::default());
    let _ = e.write_all(&encode(root));
    e.finish().unwrap_or_default()
}

/// 造一个最小可用的 level.dat，测试与自检用
#[doc(hidden)]
pub fn sample_level_dat(version: &str, game_mode: i32, hardcore: bool) -> Vec<u8> {
    let mut data = BTreeMap::new();
    data.insert("LevelName".to_string(), Value::String("测试世界".into()));
    data.insert("GameType".to_string(), Value::Int(game_mode));
    data.insert("Difficulty".to_string(), Value::Byte(2));
    data.insert("hardcore".to_string(), Value::Byte(hardcore as i8));
    data.insert("allowCommands".to_string(), Value::Byte(0));
    data.insert("LastPlayed".to_string(), Value::Long(1_700_000_000_000));
    data.insert("DataVersion".to_string(), Value::Int(3465));
    let mut version_tag = BTreeMap::new();
    version_tag.insert("Name".to_string(), Value::String(version.into()));
    version_tag.insert("Id".to_string(), Value::Int(3465));
    data.insert("Version".to_string(), Value::Compound(version_tag));
    let mut root = BTreeMap::new();
    root.insert("Data".to_string(), Value::Compound(data));
    encode_gzip(&Value::Compound(root))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(v: Value) -> Value {
        parse(&encode(&v)).unwrap()
    }

    #[test]
    fn reads_scalars_and_strings() {
        let mut m = BTreeMap::new();
        m.insert("a".into(), Value::Byte(-3));
        m.insert("b".into(), Value::Short(300));
        m.insert("c".into(), Value::Int(-70000));
        m.insert("d".into(), Value::Long(9_000_000_000));
        m.insert("e".into(), Value::String("中文 ok".into()));
        let root = roundtrip(Value::Compound(m.clone()));
        assert_eq!(root.get("a").and_then(|v| v.as_i64()), Some(-3));
        assert_eq!(root.get("b").and_then(|v| v.as_i64()), Some(300));
        assert_eq!(root.get("c").and_then(|v| v.as_i64()), Some(-70000));
        assert_eq!(root.get("d").and_then(|v| v.as_i64()), Some(9_000_000_000));
        assert_eq!(root.get("e").and_then(|v| v.as_str()), Some("中文 ok"));
    }

    #[test]
    fn reads_arrays_and_lists() {
        let mut m = BTreeMap::new();
        m.insert("ints".into(), Value::IntArray(vec![1, -2, 3]));
        m.insert("longs".into(), Value::LongArray(vec![1 << 40, -1]));
        m.insert("bytes".into(), Value::ByteArray(vec![1, 2, 3]));
        m.insert(
            "list".into(),
            Value::List(vec![Value::String("x".into()), Value::String("y".into())]),
        );
        let root = roundtrip(Value::Compound(m));
        match root.get("ints") {
            Some(Value::IntArray(a)) => assert_eq!(a, &vec![1, -2, 3]),
            other => panic!("{other:?}"),
        }
        match root.get("longs") {
            Some(Value::LongArray(a)) => assert_eq!(a, &vec![1 << 40, -1]),
            other => panic!("{other:?}"),
        }
        match root.get("list") {
            Some(Value::List(l)) => assert_eq!(l.len(), 2),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn reads_level_info() {
        let bytes = sample_level_dat("1.20.1", 1, true);
        let root = parse(&gunzip(&bytes).unwrap()).unwrap();
        // 按路径取值，前端要用的那几项都能直接拿到
        assert_eq!(
            root.path(&["Data", "Version", "Name"])
                .and_then(|v| v.as_str()),
            Some("1.20.1")
        );
        let info = LevelInfo::from_nbt(&root);
        assert_eq!(info.version_name.as_deref(), Some("1.20.1"));
        assert_eq!(info.version_id, Some(3465));
        assert_eq!(info.game_mode.as_deref(), Some("创造"));
        assert_eq!(info.difficulty.as_deref(), Some("普通"));
        assert_eq!(info.hardcore, Some(true));
        assert_eq!(info.allow_commands, Some(false));
        assert_eq!(info.level_name.as_deref(), Some("测试世界"));
        assert_eq!(info.last_played.as_deref(), Some("2023-11-14T22:13:20Z"));
        assert!(info.looks_valid());
    }

    #[test]
    fn missing_fields_stay_none() {
        let mut root = BTreeMap::new();
        root.insert("Data".into(), Value::Compound(BTreeMap::new()));
        let info = LevelInfo::from_nbt(&Value::Compound(root));
        assert!(info.version_name.is_none());
        assert!(info.game_mode.is_none());
        assert!(info.last_played.is_none());
        assert!(!info.looks_valid());
    }

    #[test]
    fn rejects_truncated_data() {
        let bytes = sample_level_dat("1.19.2", 0, false);
        let full = gunzip(&bytes).unwrap();
        let cut = &full[..full.len() / 2];
        assert!(parse(cut).is_err());
    }

    #[test]
    fn rejects_absurd_lengths() {
        // 根 compound + 名字 + 一个长度为 30000 的字符串，但没那么多数据
        let mut raw = vec![10u8, 0, 0, 8, 0, 1, b'a'];
        raw.extend_from_slice(&(30000i16).to_be_bytes());
        let err = parse(&raw).unwrap_err();
        assert!(err.starts_with("INVALID_NBT|"), "{err}");
    }

    #[test]
    fn rejects_non_compound_root() {
        let mut raw = vec![8u8, 0, 0];
        write_string(&mut raw, "hello");
        assert!(parse(&raw).is_err());
    }

    #[test]
    fn reads_uncompressed_files_too() {
        let dir = std::env::temp_dir().join(format!("astral-nbt-{}", crate::time::now_nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("level.dat");
        let mut root = BTreeMap::new();
        let mut data = BTreeMap::new();
        data.insert("GameType".into(), Value::Int(2));
        root.insert("Data".into(), Value::Compound(data));
        std::fs::write(&path, encode(&Value::Compound(root))).unwrap();
        let info = LevelInfo::read(&dir).unwrap();
        assert_eq!(info.game_mode.as_deref(), Some("冒险"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_is_none() {
        let dir =
            std::env::temp_dir().join(format!("astral-nbt-none-{}", crate::time::now_nanos()));
        assert!(LevelInfo::read(&dir).is_none());
    }
}
