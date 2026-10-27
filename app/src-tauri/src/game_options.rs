//! 游戏设置（`options.txt`）读写。
//!
//! Minecraft 把玩家的图形/声音/操作设置存在实例根目录的 `options.txt` 里，
//! 一行一个 `键:值`。这个文件由游戏自己维护，我们**只做三件事**：
//!
//! 1. 读出来，按人能看懂的分组和标签展示；
//! 2. 允许改**白名单里**的键，改之前先备份；
//! 3. 不认识的键、注释、空行、顺序，全部原样保留。
//!
//! 为什么要白名单：`options.txt` 里有 `version`、`lastServer`、
//! `key_key.*` 这类键，改错了轻则设置重置，重则游戏起不来或者把玩家的
//! 按键全冲掉。我们只碰确定语义、有明确取值范围的那些。
//!
//! 为什么改之前要备份：玩家可能调了很久的按键和灵敏度，一次误操作就没了。
//! 备份放在 `<实例>/.astral-options/`，保留最近 10 份。

use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// 备份目录（相对实例根目录）
pub const BACKUP_DIR: &str = ".astral-options";
/// 最多保留几份备份
pub const MAX_BACKUPS: usize = 10;
/// 单行长度上限，防止读到畸形文件
const MAX_LINE: usize = 4096;
/// 文件大小上限
const MAX_FILE: u64 = 4 * 1024 * 1024;

#[derive(Clone, Copy)]
enum Kind {
    Bool,
    Int { min: i64, max: i64 },
    Float { min: f64, max: f64 },
    Enum(&'static [(&'static str, &'static str)]),
}

struct Spec {
    key: &'static str,
    label: &'static str,
    group: &'static str,
    kind: Kind,
    default: &'static str,
    note: &'static str,
}

/// 白名单。只放确定语义、改错也不会毁存档的键。
const SPECS: &[Spec] = &[
    Spec {
        key: "renderDistance",
        label: "渲染距离",
        group: "画面",
        kind: Kind::Int { min: 2, max: 32 },
        default: "12",
        note: "越大看得越远，也越吃显卡",
    },
    Spec {
        key: "simulationDistance",
        label: "模拟距离",
        group: "画面",
        kind: Kind::Int { min: 5, max: 32 },
        default: "12",
        note: "远处生物和方块更新的范围",
    },
    Spec {
        key: "graphicsMode",
        label: "画质预设",
        group: "画面",
        kind: Kind::Enum(&[("0", "流畅"), ("1", "高品质"), ("2", "极佳")]),
        default: "1",
        note: "极佳需要较好的显卡",
    },
    Spec {
        key: "ao",
        label: "平滑光照",
        group: "画面",
        kind: Kind::Bool,
        default: "true",
        note: "",
    },
    Spec {
        key: "renderClouds",
        label: "云",
        group: "画面",
        kind: Kind::Enum(&[("true", "高品质"), ("fast", "流畅"), ("false", "关闭")]),
        default: "true",
        note: "",
    },
    Spec {
        key: "particles",
        label: "粒子效果",
        group: "画面",
        kind: Kind::Enum(&[("0", "全部"), ("1", "减少"), ("2", "最少")]),
        default: "0",
        note: "",
    },
    Spec {
        key: "mipmapLevels",
        label: "贴图模糊等级",
        group: "画面",
        kind: Kind::Int { min: 0, max: 4 },
        default: "4",
        note: "0 关闭，远处方块边缘会更抖",
    },
    Spec {
        key: "entityShadows",
        label: "实体阴影",
        group: "画面",
        kind: Kind::Bool,
        default: "true",
        note: "",
    },
    Spec {
        key: "bobView",
        label: "视角摇晃",
        group: "画面",
        kind: Kind::Bool,
        default: "true",
        note: "走路时画面跟着上下晃",
    },
    Spec {
        key: "fov",
        label: "视场角",
        group: "画面",
        kind: Kind::Int { min: 30, max: 110 },
        default: "70",
        note: "晕 3D 的话调小一点",
    },
    Spec {
        key: "gamma",
        label: "亮度",
        group: "画面",
        kind: Kind::Float { min: 0.0, max: 1.0 },
        default: "0.5",
        note: "调到 1.0 相当于夜视，有些服务器算作弊",
    },
    Spec {
        key: "guiScale",
        label: "界面缩放",
        group: "界面",
        kind: Kind::Enum(&[
            ("0", "自动"),
            ("1", "小"),
            ("2", "中"),
            ("3", "大"),
            ("4", "特大"),
        ]),
        default: "0",
        note: "",
    },
    Spec {
        key: "maxFps",
        label: "最大帧率",
        group: "性能",
        kind: Kind::Int { min: 10, max: 260 },
        default: "120",
        note: "260 是游戏里的上限",
    },
    Spec {
        key: "enableVsync",
        label: "垂直同步",
        group: "性能",
        kind: Kind::Bool,
        default: "true",
        note: "开了画面更稳但帧率被刷新率卡住",
    },
    Spec {
        key: "fullscreen",
        label: "全屏",
        group: "窗口",
        kind: Kind::Bool,
        default: "false",
        note: "",
    },
    Spec {
        key: "overrideWidth",
        label: "窗口宽度",
        group: "窗口",
        kind: Kind::Int { min: 0, max: 10000 },
        default: "0",
        note: "0 表示用游戏自己算的尺寸",
    },
    Spec {
        key: "overrideHeight",
        label: "窗口高度",
        group: "窗口",
        kind: Kind::Int { min: 0, max: 10000 },
        default: "0",
        note: "0 表示用游戏自己算的尺寸",
    },
    Spec {
        key: "pauseOnLostFocus",
        label: "失焦时暂停",
        group: "窗口",
        kind: Kind::Bool,
        default: "true",
        note: "切到别的窗口时单人游戏暂停",
    },
    Spec {
        key: "soundCategory_master",
        label: "主音量",
        group: "声音",
        kind: Kind::Float { min: 0.0, max: 1.0 },
        default: "1.0",
        note: "0 到 1 之间",
    },
    Spec {
        key: "soundCategory_music",
        label: "音乐音量",
        group: "声音",
        kind: Kind::Float { min: 0.0, max: 1.0 },
        default: "1.0",
        note: "",
    },
    Spec {
        key: "soundCategory_blocks",
        label: "方块音量",
        group: "声音",
        kind: Kind::Float { min: 0.0, max: 1.0 },
        default: "1.0",
        note: "",
    },
    Spec {
        key: "soundCategory_hostile",
        label: "敌对生物音量",
        group: "声音",
        kind: Kind::Float { min: 0.0, max: 1.0 },
        default: "1.0",
        note: "",
    },
    Spec {
        key: "mouseSensitivity",
        label: "鼠标灵敏度",
        group: "操作",
        kind: Kind::Float { min: 0.0, max: 1.0 },
        default: "0.5",
        note: "",
    },
    Spec {
        key: "invertYMouse",
        label: "反转鼠标 Y 轴",
        group: "操作",
        kind: Kind::Bool,
        default: "false",
        note: "",
    },
    Spec {
        key: "autoJump",
        label: "自动跳跃",
        group: "操作",
        kind: Kind::Bool,
        default: "false",
        note: "基岩版默认开，Java 版默认关",
    },
    Spec {
        key: "toggleCrouch",
        label: "潜行切换",
        group: "操作",
        kind: Kind::Bool,
        default: "false",
        note: "",
    },
    Spec {
        key: "lang",
        label: "语言",
        group: "界面",
        kind: Kind::Enum(&[
            ("zh_cn", "简体中文"),
            ("zh_tw", "繁體中文"),
            ("en_us", "English (US)"),
            ("ja_jp", "日本語"),
            ("ko_kr", "한국어"),
            ("ru_ru", "Русский"),
            ("de_de", "Deutsch"),
            ("fr_fr", "Français"),
            ("es_es", "Español"),
        ]),
        default: "zh_cn",
        note: "改完下次启动生效",
    },
    Spec {
        key: "showSubtitles",
        label: "显示字幕",
        group: "界面",
        kind: Kind::Bool,
        default: "false",
        note: "把游戏里的音效显示成字幕",
    },
    Spec {
        key: "chatVisibility",
        label: "聊天可见性",
        group: "界面",
        kind: Kind::Enum(&[("0", "全部"), ("1", "仅指令"), ("2", "隐藏")]),
        default: "0",
        note: "",
    },
    Spec {
        key: "reducedDebugInfo",
        label: "简化调试信息",
        group: "界面",
        kind: Kind::Bool,
        default: "false",
        note: "F3 调试屏少显示一些内容",
    },
    Spec {
        key: "narrator",
        label: "旁白",
        group: "界面",
        kind: Kind::Enum(&[
            ("0", "关闭"),
            ("1", "全部"),
            ("2", "聊天与系统"),
            ("3", "系统"),
        ]),
        default: "0",
        note: "",
    },
];

fn find_spec(key: &str) -> Option<&'static Spec> {
    SPECS.iter().find(|s| s.key == key)
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OptionChoice {
    pub value: String,
    pub label: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OptionField {
    pub key: String,
    pub label: String,
    pub group: String,
    /// `bool` / `int` / `float` / `enum`
    pub kind: String,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub choices: Vec<OptionChoice>,
    /// 文件里的当前值；文件里没有这个键就是 null
    pub value: Option<String>,
    pub default: String,
    pub note: Option<String>,
    /// 文件里有值但不是合法值（例如被改坏了）
    pub invalid: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GameOptions {
    pub path: String,
    pub exists: bool,
    pub modified_at: Option<String>,
    pub fields: Vec<OptionField>,
    /// 文件里不在白名单的键有多少个（原样保留，不动）
    pub other_keys: u32,
    pub raw_lines: u32,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OptionsWrite {
    pub path: String,
    pub changed: Vec<String>,
    pub backup: Option<String>,
    pub backup_dir: String,
}

/// 一行：原样保留或键值对
enum Line {
    Raw(String),
    Kv(String, String),
}

fn parse(text: &str) -> Vec<Line> {
    let mut out = Vec::new();
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            out.push(Line::Raw(line.to_string()));
            continue;
        }
        match line.split_once(':') {
            Some((k, v)) if !k.is_empty() && k.len() <= MAX_LINE => {
                out.push(Line::Kv(k.to_string(), v.to_string()));
            }
            _ => out.push(Line::Raw(line.to_string())),
        }
    }
    out
}

fn render(lines: &[Line]) -> String {
    let mut s = String::new();
    for (i, l) in lines.iter().enumerate() {
        if i > 0 {
            s.push('\n');
        }
        match l {
            Line::Raw(t) => s.push_str(t),
            Line::Kv(k, v) => {
                s.push_str(k);
                s.push(':');
                s.push_str(v);
            }
        }
    }
    s
}

/// 值是否合法；合法返回规范化后的字符串
fn validate(spec: &Spec, value: &str) -> Result<String, String> {
    let v = value.trim();
    let bad = |why: &str| format!("INVALID_VALUE|{} 的值不合法（{why}）", spec.label);
    match spec.kind {
        Kind::Bool => match v {
            "true" | "false" => Ok(v.to_string()),
            _ => Err(bad("只能是 true 或 false")),
        },
        Kind::Int { min, max } => {
            let n: i64 = v.parse().map_err(|_| bad("需要整数"))?;
            if n < min || n > max {
                return Err(bad(&format!("要在 {min} 到 {max} 之间")));
            }
            Ok(n.to_string())
        }
        Kind::Float { min, max } => {
            let n: f64 = v.parse().map_err(|_| bad("需要小数"))?;
            if !n.is_finite() || n < min || n > max {
                return Err(bad(&format!("要在 {min} 到 {max} 之间")));
            }
            Ok(format!("{n}"))
        }
        Kind::Enum(choices) => {
            if choices.iter().any(|(c, _)| *c == v) {
                Ok(v.to_string())
            } else {
                Err(bad("不在可选项里"))
            }
        }
    }
}

fn looks_valid(spec: &Spec, value: &str) -> bool {
    validate(spec, value).is_ok()
}

fn path_of(instance_dir: &Path) -> PathBuf {
    instance_dir.join("options.txt")
}

fn backup_dir_of(instance_dir: &Path) -> PathBuf {
    instance_dir.join(BACKUP_DIR)
}

fn read_text(path: &Path) -> Result<Option<String>, String> {
    match std::fs::metadata(path) {
        Ok(m) => {
            if !m.is_file() {
                return Err("INVALID_PATH|options.txt 不是文件".into());
            }
            if m.len() > MAX_FILE {
                return Err("INVALID_VALUE|options.txt 太大，可能不是游戏设置文件".into());
            }
        }
        Err(_) => return Ok(None),
    }
    let bytes = std::fs::read(path).map_err(|e| format!("PERMISSION|读取 options.txt 失败|{e}"))?;
    Ok(Some(String::from_utf8_lossy(&bytes).to_string()))
}

/// 读出设置（只读，不改任何文件）
pub fn read(instance_dir: &Path) -> Result<GameOptions, String> {
    let path = path_of(instance_dir);
    let text = read_text(&path)?;
    let exists = text.is_some();
    let lines = text.as_deref().map(parse).unwrap_or_default();

    let mut current: BTreeMap<String, String> = BTreeMap::new();
    let mut other_keys = 0u32;
    for l in &lines {
        if let Line::Kv(k, v) = l {
            if find_spec(k).is_some() {
                current.insert(k.clone(), v.clone());
            } else {
                other_keys += 1;
            }
        }
    }

    let fields = SPECS
        .iter()
        .map(|s| {
            let value = current.get(s.key).cloned();
            let invalid = value
                .as_deref()
                .map(|v| !looks_valid(s, v))
                .unwrap_or(false);
            let (kind, min, max, choices) = match s.kind {
                Kind::Bool => ("bool", None, None, Vec::new()),
                Kind::Int { min, max } => ("int", Some(min as f64), Some(max as f64), Vec::new()),
                Kind::Float { min, max } => ("float", Some(min), Some(max), Vec::new()),
                Kind::Enum(c) => (
                    "enum",
                    None,
                    None,
                    c.iter()
                        .map(|(v, l)| OptionChoice {
                            value: (*v).to_string(),
                            label: (*l).to_string(),
                        })
                        .collect(),
                ),
            };
            OptionField {
                key: s.key.to_string(),
                label: s.label.to_string(),
                group: s.group.to_string(),
                kind: kind.to_string(),
                min,
                max,
                choices,
                value,
                default: s.default.to_string(),
                note: if s.note.is_empty() {
                    None
                } else {
                    Some(s.note.to_string())
                },
                invalid,
            }
        })
        .collect();

    Ok(GameOptions {
        path: path.to_string_lossy().to_string(),
        exists,
        modified_at: std::fs::metadata(&path)
            .ok()
            .and_then(|m| m.modified().ok())
            .map(crate::time::system_time_to_iso),
        fields,
        other_keys,
        raw_lines: lines.len() as u32,
    })
}

/// 写设置：只改白名单里的键，其余行原样保留；改之前备份
pub fn write(
    instance_dir: &Path,
    changes: &BTreeMap<String, String>,
    running: bool,
) -> Result<OptionsWrite, String> {
    if running {
        return Err(
            "INSTANCE_BUSY|游戏正在运行，先退出游戏再改设置（游戏退出时会覆盖这个文件）".into(),
        );
    }
    if changes.is_empty() {
        return Err("INVALID_VALUE|没有要修改的设置".into());
    }

    // 先把所有改动校验完，有一个不合法就整批不改
    let mut wanted: Vec<(&'static Spec, String)> = Vec::new();
    for (k, v) in changes {
        let spec = find_spec(k)
            .ok_or_else(|| format!("INVALID_VALUE|不支持修改「{k}」，只允许改白名单里的设置项"))?;
        let normalized = validate(spec, v)?;
        wanted.push((spec, normalized));
    }

    let path = path_of(instance_dir);
    let text = read_text(&path)?;
    let mut lines = text.as_deref().map(parse).unwrap_or_default();

    let mut changed = Vec::new();
    for (spec, value) in &wanted {
        let mut done = false;
        for l in lines.iter_mut() {
            if let Line::Kv(k, v) = l {
                if k == spec.key {
                    if v != value {
                        *v = value.clone();
                        changed.push(spec.key.to_string());
                    }
                    done = true;
                    break;
                }
            }
        }
        if !done {
            // 文件里没有这个键：追加在末尾
            lines.push(Line::Kv(spec.key.to_string(), value.clone()));
            changed.push(spec.key.to_string());
        }
    }

    if changed.is_empty() {
        return Ok(OptionsWrite {
            path: path.to_string_lossy().to_string(),
            changed,
            backup: None,
            backup_dir: backup_dir_of(instance_dir).to_string_lossy().to_string(),
        });
    }

    // 备份现有文件（第一次改之前的那份才值钱，但每次改都留一份更保险）
    let mut backup = None;
    if text.is_some() {
        let dir = backup_dir_of(instance_dir);
        std::fs::create_dir_all(&dir).map_err(|e| format!("PERMISSION|创建备份目录失败|{e}"))?;
        let dest = free_backup_path(&dir, &format!("options-{}.txt", crate::time::now_stamp()));
        std::fs::copy(&path, &dest).map_err(|e| format!("PERMISSION|备份 options.txt 失败|{e}"))?;
        backup = Some(dest.to_string_lossy().to_string());
        prune_backups(&dir);
    }

    // 先写 .part 再替换，避免半截文件
    let body = render(&lines);
    let tmp = path.with_extension("txt.part");
    std::fs::write(&tmp, body.as_bytes())
        .map_err(|e| format!("PERMISSION|写入 options.txt 失败|{e}"))?;
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("PERMISSION|替换 options.txt 失败|{e}"));
    }

    Ok(OptionsWrite {
        path: path.to_string_lossy().to_string(),
        changed,
        backup,
        backup_dir: backup_dir_of(instance_dir).to_string_lossy().to_string(),
    })
}

/// 只保留最近 `MAX_BACKUPS` 份（按文件名里的时间戳排序，文件名天然有序）
fn prune_backups(dir: &Path) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut names: Vec<String> = rd
        .flatten()
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.starts_with("options-") && n.ends_with(".txt"))
        .collect();
    if names.len() <= MAX_BACKUPS {
        return;
    }
    names.sort();
    let drop_n = names.len() - MAX_BACKUPS;
    for n in names.into_iter().take(drop_n) {
        let _ = std::fs::remove_file(dir.join(n));
    }
}

// ── 备份的列表 / 恢复 / 删除 ──────────────────────────────────────

/// 一份 `options.txt` 备份
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OptionsBackup {
    /// 文件名，例如 `options-20261027-081500.txt`；恢复和删除都靠它
    pub file_name: String,
    pub path: String,
    pub bytes: u64,
    /// 从文件名里解析出来的时间；解析不出来就是文件名本身
    pub created_at: String,
    /// 备份里有几行键值对（能看出是不是一份正常设置）
    pub lines: u32,
    /// 这份备份里的渲染距离，用来在列表里给个直观印象；没有就是 null
    pub render_distance: Option<String>,
}

/// 恢复备份的结果
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OptionsRestore {
    pub path: String,
    /// 恢复前的当前文件被存到了哪（本来就没有文件时是 null）
    pub previous: Option<String>,
    pub restored_from: String,
}

/// 同名备份已存在时（同一秒内改了两次）加 `-1`、`-2`，绝不覆盖旧备份。
fn free_backup_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let stem = name.strip_suffix(".txt").unwrap_or(name);
    for i in 1..=20 {
        let p = dir.join(format!("{stem}-{i}.txt"));
        if !p.exists() {
            return p;
        }
    }
    dir.join(format!("{stem}-{}.txt", crate::time::now_nanos()))
}

fn backup_name_ok(file_name: &str) -> Result<(), String> {
    if file_name.is_empty() || file_name.len() > 128 {
        return Err("INVALID_NAME|备份文件名不合法".into());
    }
    if file_name.contains(['/', '\\', ':']) || file_name.contains("..") {
        return Err("INVALID_NAME|备份文件名不合法".into());
    }
    if !file_name.starts_with("options-") || !file_name.ends_with(".txt") {
        return Err("INVALID_NAME|不是 options 备份文件".into());
    }
    Ok(())
}

/// 把 `options-20261027-081500.txt` 里的时间戳转成 ISO。
///
/// 解析不出来就原样返回文件名 —— 早期版本的备份名字可能不是这个格式，
/// 显示一个怪名字也比报错强。
fn stamp_of(file_name: &str) -> String {
    let stem = file_name
        .strip_prefix("options-")
        .and_then(|s| s.strip_suffix(".txt"))
        .unwrap_or(file_name);
    let Some((date, time)) = stem.split_once('-') else {
        return file_name.to_string();
    };
    if date.len() != 8 || time.len() != 6 {
        return file_name.to_string();
    }
    let iso = format!(
        "{}-{}-{}T{}:{}:{}Z",
        &date[0..4],
        &date[4..6],
        &date[6..8],
        &time[0..2],
        &time[2..4],
        &time[4..6]
    );
    if iso.len() == 20 {
        iso
    } else {
        file_name.to_string()
    }
}

/// 列出全部备份，最新的在前（只读）
pub fn list_backups(instance_dir: &Path) -> Vec<OptionsBackup> {
    let dir = backup_dir_of(instance_dir);
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<OptionsBackup> = rd
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| {
            let file_name = e.file_name().to_string_lossy().to_string();
            backup_name_ok(&file_name).ok()?;
            let path = e.path();
            let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let text = std::fs::read(&path)
                .map(|b| String::from_utf8_lossy(&b).to_string())
                .unwrap_or_default();
            let mut lines = 0u32;
            let mut render_distance = None;
            for l in parse(&text) {
                if let Line::Kv(k, v) = l {
                    lines += 1;
                    if k == "renderDistance" {
                        render_distance = Some(v);
                    }
                }
            }
            Some(OptionsBackup {
                file_name,
                path: path.to_string_lossy().to_string(),
                bytes,
                created_at: stamp_of(&e.file_name().to_string_lossy()),
                lines,
                render_distance,
            })
        })
        .collect();
    // 文件名里带时间戳，倒序就是最新在前
    out.sort_by(|a, b| b.file_name.cmp(&a.file_name));
    out
}

/// 从备份恢复。恢复前先把当前 `options.txt` 也存一份，别让人"恢复错了"没退路。
pub fn restore_backup(
    instance_dir: &Path,
    file_name: &str,
    running: bool,
) -> Result<OptionsRestore, String> {
    if running {
        return Err(
            "INSTANCE_BUSY|游戏正在运行，先退出游戏再恢复设置（游戏退出时会覆盖这个文件）".into(),
        );
    }
    backup_name_ok(file_name)?;
    let src = backup_dir_of(instance_dir).join(file_name);
    if !src.is_file() {
        return Err("NOT_FOUND|找不到这份备份|它可能已经被清理掉了".into());
    }
    if crate::net::is_reparse_point(&src) {
        return Err("INVALID_PATH|这是一份符号链接，已拒绝读取".into());
    }

    let path = path_of(instance_dir);
    // 当前文件先留底，名字加 -before 前缀，一眼能看出来是恢复前的
    let mut previous = None;
    if path.is_file() {
        let dir = backup_dir_of(instance_dir);
        std::fs::create_dir_all(&dir).map_err(|e| format!("PERMISSION|创建备份目录失败|{e}"))?;
        let dest = free_backup_path(
            &dir,
            &format!("options-before-{}.txt", crate::time::now_stamp()),
        );
        std::fs::copy(&path, &dest).map_err(|e| format!("PERMISSION|备份当前设置失败|{e}"))?;
        previous = Some(dest.to_string_lossy().to_string());
        prune_backups(&dir);
    }

    // 先写 .part 再换，避免半截文件
    let tmp = path.with_extension("txt.part");
    std::fs::copy(&src, &tmp).map_err(|e| format!("PERMISSION|写入设置失败|{e}"))?;
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("PERMISSION|替换设置失败|{e}"));
    }

    Ok(OptionsRestore {
        path: path.to_string_lossy().to_string(),
        previous,
        restored_from: file_name.to_string(),
    })
}

/// 删掉一份备份（要确认）。只删这一个文件。
pub fn delete_backup(instance_dir: &Path, file_name: &str, confirm: bool) -> Result<(), String> {
    if !confirm {
        return Err("UNSUPPORTED|删除备份需要明确确认".into());
    }
    backup_name_ok(file_name)?;
    let p = backup_dir_of(instance_dir).join(file_name);
    if !p.is_file() {
        return Err("NOT_FOUND|找不到这份备份".into());
    }
    if crate::net::is_reparse_point(&p) {
        return Err("INVALID_PATH|这是一份符号链接，已拒绝删除".into());
    }
    std::fs::remove_file(&p).map_err(|e| format!("PERMISSION|删除备份失败|{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "astral-opts-{tag}-{:032x}",
            crate::time::now_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn changes(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn parses_key_value_lines() {
        let lines = parse("version:3465\nrenderDistance:12\n\n# comment\nbroken");
        assert_eq!(lines.len(), 5);
        assert!(matches!(&lines[0], Line::Kv(k, v) if k == "version" && v == "3465"));
        assert!(matches!(&lines[2], Line::Raw(t) if t.is_empty()));
        assert!(matches!(&lines[3], Line::Raw(t) if t == "# comment"));
        assert!(matches!(&lines[4], Line::Raw(t) if t == "broken"));
    }

    #[test]
    fn renders_round_trip() {
        let src = "version:3465\nrenderDistance:12\n\n# keep me\nlang:zh_cn";
        assert_eq!(render(&parse(src)), src);
        // 文件末尾的换行不能丢
        assert_eq!(render(&parse("a:1\n")), "a:1\n");
    }

    #[test]
    fn validates_ranges_and_enums() {
        let rd = find_spec("renderDistance").unwrap();
        assert!(validate(rd, "12").is_ok());
        assert!(validate(rd, "1").is_err());
        assert!(validate(rd, "99").is_err());
        assert!(validate(rd, "abc").is_err());

        let lang = find_spec("lang").unwrap();
        assert!(validate(lang, "zh_cn").is_ok());
        assert!(validate(lang, "klingon").is_err());

        let ao = find_spec("ao").unwrap();
        assert!(validate(ao, "true").is_ok());
        assert!(validate(ao, "1").is_err());

        let gamma = find_spec("gamma").unwrap();
        assert!(validate(gamma, "1.0").is_ok());
        assert!(validate(gamma, "1.5").is_err());
        assert!(validate(gamma, "NaN").is_err());
    }

    #[test]
    fn reads_existing_file_and_keeps_unknown_keys() {
        let d = tmp("read");
        std::fs::write(
            d.join("options.txt"),
            "version:3465\nrenderDistance:8\nlang:en_us\nkey_key.jump:space\n",
        )
        .unwrap();
        let o = read(&d).unwrap();
        assert!(o.exists);
        assert_eq!(o.other_keys, 2); // version + key_key.jump
        let rd = o.fields.iter().find(|f| f.key == "renderDistance").unwrap();
        assert_eq!(rd.value.as_deref(), Some("8"));
        assert!(!rd.invalid);
        let fov = o.fields.iter().find(|f| f.key == "fov").unwrap();
        assert!(fov.value.is_none());
    }

    #[test]
    fn flags_invalid_values_without_failing() {
        let d = tmp("invalid");
        std::fs::write(d.join("options.txt"), "renderDistance:999\n").unwrap();
        let o = read(&d).unwrap();
        let rd = o.fields.iter().find(|f| f.key == "renderDistance").unwrap();
        assert_eq!(rd.value.as_deref(), Some("999"));
        assert!(rd.invalid);
    }

    #[test]
    fn write_changes_only_whitelisted_keys() {
        let d = tmp("write");
        let src = "version:3465\nrenderDistance:8\nkey_key.jump:space\n";
        std::fs::write(d.join("options.txt"), src).unwrap();
        let out = write(&d, &changes(&[("renderDistance", "16")]), false).unwrap();
        assert_eq!(out.changed, vec!["renderDistance".to_string()]);
        assert!(out.backup.is_some());
        let now = std::fs::read_to_string(d.join("options.txt")).unwrap();
        assert!(now.contains("renderDistance:16"));
        assert!(now.contains("version:3465"));
        assert!(now.contains("key_key.jump:space"));
    }

    #[test]
    fn write_rejects_unknown_and_bad_values() {
        let d = tmp("reject");
        std::fs::write(d.join("options.txt"), "renderDistance:8\n").unwrap();
        assert!(write(&d, &changes(&[("lastServer", "evil")]), false).is_err());
        assert!(write(&d, &changes(&[("renderDistance", "999")]), false).is_err());
        // 一个不合法就整批不改
        assert!(write(
            &d,
            &changes(&[("renderDistance", "16"), ("guiScale", "9")]),
            false
        )
        .is_err());
        let now = std::fs::read_to_string(d.join("options.txt")).unwrap();
        assert!(now.contains("renderDistance:8"));
    }

    #[test]
    fn write_refuses_while_running() {
        let d = tmp("running");
        std::fs::write(d.join("options.txt"), "renderDistance:8\n").unwrap();
        assert!(write(&d, &changes(&[("renderDistance", "16")]), true).is_err());
    }

    #[test]
    fn write_creates_file_when_missing() {
        let d = tmp("create");
        let out = write(&d, &changes(&[("lang", "en_us")]), false).unwrap();
        assert!(out.backup.is_none());
        let now = std::fs::read_to_string(d.join("options.txt")).unwrap();
        assert_eq!(now, "lang:en_us");
        // 再改一次就有备份了
        let out = write(&d, &changes(&[("lang", "zh_cn")]), false).unwrap();
        assert!(out.backup.is_some());
    }

    #[test]
    fn prunes_old_backups() {
        let d = tmp("prune");
        let b = d.join(BACKUP_DIR);
        std::fs::create_dir_all(&b).unwrap();
        for i in 0..15 {
            std::fs::write(b.join(format!("options-2026010{i}-000000.txt")), b"x").unwrap();
        }
        prune_backups(&b);
        let n = std::fs::read_dir(&b).unwrap().count();
        assert_eq!(n, MAX_BACKUPS);
    }

    #[test]
    fn lists_backups_newest_first() {
        let d = tmp("listbk");
        std::fs::write(d.join("options.txt"), "renderDistance:12\nlang:zh_cn\n").unwrap();
        write(&d, &changes(&[("renderDistance", "16")]), false).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        write(&d, &changes(&[("renderDistance", "20")]), false).unwrap();

        let list = list_backups(&d);
        assert_eq!(list.len(), 2);
        // 最新在前：第二次改之前备份的是 16
        assert!(list[0].file_name > list[1].file_name);
        assert_eq!(list[0].render_distance.as_deref(), Some("16"));
        assert!(list[0].lines >= 2);
        assert!(list[0].created_at.contains('T'));
        assert!(!list[0].created_at.contains("options-"));
    }

    #[test]
    fn restores_a_backup_and_keeps_the_old_one() {
        let d = tmp("restore");
        std::fs::write(d.join("options.txt"), "renderDistance:8\n").unwrap();
        write(&d, &changes(&[("renderDistance", "16")]), false).unwrap();
        let list = list_backups(&d);
        assert_eq!(list.len(), 1);

        let out = restore_backup(&d, &list[0].file_name, false).unwrap();
        assert_eq!(
            std::fs::read_to_string(d.join("options.txt")).unwrap(),
            "renderDistance:8\n"
        );
        // 恢复前的那份（16）被留底了，玩家还能再回去
        let after = list_backups(&d);
        assert_eq!(after.len(), 2);
        assert!(
            after
                .iter()
                .any(|b| b.file_name.contains("-before")
                    && b.render_distance.as_deref() == Some("16"))
        );
        assert_eq!(out.restored_from, list[0].file_name);
        assert!(out.previous.is_some());
    }

    #[test]
    fn restore_refuses_running_and_junk() {
        let d = tmp("restorejunk");
        std::fs::write(d.join("options.txt"), "lang:zh_cn\n").unwrap();
        write(&d, &changes(&[("lang", "en_us")]), false).unwrap();
        let name = list_backups(&d)[0].file_name.clone();

        assert!(restore_backup(&d, &name, true).is_err());
        assert!(restore_backup(&d, "../options.txt", false).is_err());
        assert!(restore_backup(&d, "options-nope.txt", false).is_err());
        // 上面三次都不该改动文件
        assert_eq!(
            std::fs::read_to_string(d.join("options.txt")).unwrap(),
            "lang:en_us\n"
        );
    }

    #[test]
    fn deletes_a_backup_only_with_confirm() {
        let d = tmp("delbk");
        std::fs::write(d.join("options.txt"), "lang:zh_cn\n").unwrap();
        write(&d, &changes(&[("lang", "en_us")]), false).unwrap();
        let name = list_backups(&d)[0].file_name.clone();

        assert!(delete_backup(&d, &name, false).is_err());
        assert_eq!(list_backups(&d).len(), 1);
        assert!(delete_backup(&d, &name, true).is_ok());
        assert!(list_backups(&d).is_empty());
        // 删备份不该动当前设置
        assert_eq!(
            std::fs::read_to_string(d.join("options.txt")).unwrap(),
            "lang:en_us\n"
        );
    }
}
