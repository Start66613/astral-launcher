//! 本地文件导入：把玩家自己下载的 jar / zip 拖进实例。
//!
//! 现在的流程是「去 Modrinth 搜 → 点安装」，但玩家手上经常已经有一个
//! 从群里、从视频简介里存下来的 jar，或者一个资源包 zip。这时候只能自己
//! 打开 `instances/<id>/mods/` 把文件拷进去 —— 大多数玩家根本找不到这个
//! 目录，更别说判断一个 zip 到底是资源包还是光影包。
//!
//! 这里做的事：
//! 1. **看内容判断类型**，不看扩展名（`.zip` 可能是资源包、光影包、
//!    数据包，甚至是整合包）；
//! 2. 复制到对应目录，重名时自动加序号，**默认不覆盖**；
//! 3. 复制后回读大小并算 sha1，报给界面；
//! 4. 一个文件出错不影响同一批里的其他文件。
//!
//! 不碰的地方：不改 `options.txt`、不改 `config/`、不动存档。

use serde::Serialize;
use std::io::Read;
use std::path::{Path, PathBuf};

/// 单个文件上限。模组和资源包基本不会超过这个数；超了大概率是搞错了文件。
pub const MAX_FILE: u64 = 512 * 1024 * 1024;
/// 一批最多导入多少个。防止一次拖进来几千个文件把界面卡死。
pub const MAX_BATCH: usize = 64;
/// 读 zip 目录时的元数据上限，防解压炸弹。
const MAX_META: u64 = 1024 * 1024;

/// 三种能直接落地的类型
pub const KINDS: [&str; 3] = ["mod", "resourcepack", "shaderpack"];

/// 导入目标目录
#[derive(Clone, Debug)]
pub struct Targets {
    pub mods: PathBuf,
    pub resourcepacks: PathBuf,
    pub shaderpacks: PathBuf,
}

impl Targets {
    fn of(&self, kind: &str) -> Option<&Path> {
        match kind {
            "mod" => Some(&self.mods),
            "resourcepack" => Some(&self.resourcepacks),
            "shaderpack" => Some(&self.shaderpacks),
            _ => None,
        }
    }
}

/// 对一个本地文件的判断结果（只读，不改任何东西）
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ImportCandidate {
    /// 玩家传进来的原始路径
    pub path: String,
    pub file_name: String,
    pub size_bytes: u64,
    /// `mod` / `resourcepack` / `shaderpack` / `modpack` / `datapack` / `unknown`
    pub kind: String,
    /// 会落到哪个目录（判断不出来时为空）
    pub target_dir: Option<String>,
    /// 会用的文件名（重名时这里还是原名，真正落地时才加序号）
    pub target_name: Option<String>,
    /// 给人看的补充说明，例如"这个包是给 1.20.1 做的"
    pub note: Option<String>,
    /// 不能导入的原因；能导入时为空
    pub problem: Option<String>,
}

impl ImportCandidate {
    pub fn ok(&self) -> bool {
        self.problem.is_none()
    }
}

/// 一批导入的结果
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ImportOutcome {
    pub path: String,
    pub file_name: String,
    pub kind: String,
    /// 真正落地的文件名（重名时会带序号）
    pub saved_as: Option<String>,
    /// 落地后的完整路径
    pub target_path: Option<String>,
    pub size_bytes: u64,
    /// 复制后回读算的 sha1，界面上可以拿它去 Modrinth 反查
    pub sha1: Option<String>,
    pub renamed: bool,
    /// 失败原因；成功时为空
    pub error: Option<String>,
}

fn ext_of(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

fn zip_names(path: &Path) -> Option<Vec<String>> {
    let file = std::fs::File::open(path).ok()?;
    let mut zip = zip::ZipArchive::new(file).ok()?;
    let mut out = Vec::with_capacity(zip.len());
    for i in 0..zip.len() {
        match zip.by_index_raw(i) {
            Ok(e) => match e.name() {
                Ok(n) => out.push(n.to_string()),
                Err(_) => return None,
            },
            Err(_) => return None,
        }
    }
    Some(out)
}

fn zip_entry_text(path: &Path, name: &str) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut zip = zip::ZipArchive::new(file).ok()?;
    let mut f = zip.by_name(name).ok()?;
    if f.size() > MAX_META {
        return None;
    }
    let mut s = String::new();
    f.read_to_string(&mut s).ok()?;
    Some(s)
}

/// `pack.mcmeta` 里的 `pack_format`（拿不到就是 None）
fn pack_format(path: &Path) -> Option<u32> {
    let text = zip_entry_text(path, "pack.mcmeta")?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v.get("pack")?
        .get("pack_format")?
        .as_u64()
        .and_then(|f| u32::try_from(f).ok())
}

/// 按内容判断类型。
///
/// 判断顺序很重要：整合包和光影包的 zip 里也可能带 `pack.mcmeta`，
/// 先认更具体的标记，最后才退回扩展名。
fn detect(path: &Path) -> (String, Option<String>) {
    let ext = ext_of(path);
    if ext == "mrpack" {
        return (
            "modpack".into(),
            Some("整合包请用「导入本地整合包」安装，不要直接拷进实例".into()),
        );
    }
    if ext == "jar" {
        return ("mod".into(), None);
    }
    if ext != "zip" {
        return ("unknown".into(), Some(format!("不认识的文件类型：.{ext}")));
    }

    let Some(names) = zip_names(path) else {
        return ("unknown".into(), Some("这不是一个能打开的 zip".into()));
    };
    let has = |want: &str| {
        names
            .iter()
            .any(|n| n == want || n.starts_with(&format!("{want}/")))
    };

    if has("modrinth.index.json") {
        return (
            "modpack".into(),
            Some("这是一个整合包，请用「导入本地整合包」安装".into()),
        );
    }
    if has("shaders") {
        return ("shaderpack".into(), None);
    }
    let meta = has("pack.mcmeta");
    let assets = has("assets");
    let data = has("data");
    if meta && assets {
        let note = pack_format(path).map(|f| {
            let vs = crate::packs::versions_for_format(f);
            if vs.is_empty() {
                format!("pack_format {f} 不在已知对照表里")
            } else {
                format!("pack_format {f}，适配 {}", vs.join("、"))
            }
        });
        return ("resourcepack".into(), note);
    }
    if meta && data {
        return (
            "datapack".into(),
            Some(
                "数据包要放进存档的 datapacks 目录，这里不支持，请手动放到 saves/<存档>/datapacks/"
                    .into(),
            ),
        );
    }
    if meta || assets {
        return (
            "resourcepack".into(),
            Some("包里结构不完整（缺少 assets 或 pack.mcmeta），游戏可能不认".into()),
        );
    }
    (
        "unknown".into(),
        Some("这个 zip 既不是资源包也不是光影包".into()),
    )
}

/// 文件本身能不能读（大小、符号链接、是不是普通文件）
///
/// 抽出来是因为「按玩家指定的类型导入」时也要过这一关，
/// 但不需要再判断内容类型。
fn check_file(path: &Path) -> Result<(String, u64), String> {
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    if file_name.is_empty() {
        return Err("文件名是空的".into());
    }
    let md = std::fs::metadata(path).map_err(|e| format!("读不到这个文件：{e}"))?;
    if !md.is_file() {
        return Err("这不是一个文件".into());
    }
    if md.len() > MAX_FILE {
        return Err(format!(
            "文件太大（{} MB），超过 {} MB 上限",
            md.len() / 1024 / 1024,
            MAX_FILE / 1024 / 1024
        ));
    }
    if crate::net::is_reparse_point(path) {
        return Err("这是一个符号链接，已拒绝导入".into());
    }
    Ok((file_name, md.len()))
}

/// 只读地看一眼这个文件能不能导入。
///
/// `targets` 只用来填 `target_dir`（界面上显示"会放到哪"），
/// 传 `None` 就不填，不影响判断结果。
pub fn inspect(path: &Path, targets: Option<&Targets>) -> ImportCandidate {
    let mut c = ImportCandidate {
        path: path.to_string_lossy().to_string(),
        file_name: path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        size_bytes: 0,
        kind: "unknown".into(),
        target_dir: None,
        target_name: None,
        note: None,
        problem: None,
    };

    let (file_name, size) = match check_file(path) {
        Ok(v) => v,
        Err(e) => {
            c.problem = Some(e);
            return c;
        }
    };
    c.file_name = file_name;
    c.size_bytes = size;

    let (kind, note) = detect(path);
    c.kind = kind.clone();
    c.note = note;
    if KINDS.contains(&kind.as_str()) {
        c.target_name = Some(c.file_name.clone());
        if let Some(t) = targets.and_then(|t| t.of(&kind)) {
            c.target_dir = Some(t.to_string_lossy().to_string());
        }
    } else {
        c.problem = c
            .note
            .clone()
            .or_else(|| Some("判断不出这是什么文件".into()));
    }
    c
}

/// 重名时依次试 `-1`、`-2`……最后退回时间戳
fn unique_target(dir: &Path, file_name: &str) -> PathBuf {
    let first = dir.join(file_name);
    if !first.exists() {
        return first;
    }
    let p = Path::new(file_name);
    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());
    let ext = p
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for i in 1..=99 {
        let candidate = dir.join(format!("{stem}-{i}{ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(format!("{stem}-{}{ext}", crate::time::now_unix_ms()))
}

/// 导入一个文件。失败返回 `Err`，成功返回落地信息。
pub fn import_one(
    targets: &Targets,
    path: &Path,
    kind_override: Option<&str>,
) -> Result<ImportOutcome, String> {
    let (file_name, size) = check_file(path).map_err(|e| format!("INVALID_VALUE|{e}"))?;

    let kind = match kind_override {
        Some(k) => {
            let k = k.trim();
            if !KINDS.contains(&k) {
                return Err(format!("INVALID_VALUE|不支持导入成 {k}"));
            }
            k.to_string()
        }
        None => {
            let (kind, note) = detect(path);
            if !KINDS.contains(&kind.as_str()) {
                let why = note.unwrap_or_else(|| "判断不出这是什么文件".into());
                return Err(format!("INVALID_VALUE|{why}"));
            }
            kind
        }
    };
    let Some(dir) = targets.of(&kind) else {
        return Err(format!("INVALID_VALUE|不支持的导入类型：{kind}"));
    };
    std::fs::create_dir_all(dir).map_err(|e| format!("PERMISSION|创建目录失败|{e}"))?;

    let dest = unique_target(dir, &file_name);
    let renamed = dest
        .file_name()
        .map(|n| n.to_string_lossy() != file_name)
        .unwrap_or(false);

    // 先写 .part 再改名：中途失败不会留下半个 jar 让游戏加载崩
    let part = dest.with_extension(format!(
        "{}.astral-import",
        dest.extension()
            .map(|e| e.to_string_lossy().to_string())
            .unwrap_or_default()
    ));
    let result = (|| -> Result<u64, String> {
        std::fs::copy(path, &part).map_err(|e| format!("PERMISSION|复制文件失败|{e}"))?;
        let written = std::fs::metadata(&part)
            .map_err(|e| format!("PERMISSION|读取复制结果失败|{e}"))?
            .len();
        if written != size {
            return Err(format!(
                "INTERNAL|复制不完整（应为 {size} 字节，实际 {written} 字节）"
            ));
        }
        std::fs::rename(&part, &dest).map_err(|e| format!("PERMISSION|保存文件失败|{e}"))?;
        Ok(written)
    })();
    let size = match result {
        Ok(n) => n,
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            return Err(e);
        }
    };

    Ok(ImportOutcome {
        path: path.to_string_lossy().to_string(),
        file_name: file_name.clone(),
        kind: kind.clone(),
        saved_as: dest.file_name().map(|n| n.to_string_lossy().to_string()),
        target_path: Some(dest.to_string_lossy().to_string()),
        size_bytes: size,
        sha1: crate::net::sha1_file(&dest).ok(),
        renamed,
        error: None,
    })
}

/// 批量导入。一个文件失败只记在它自己的 `error` 里，不影响其他文件。
pub fn import_many(
    targets: &Targets,
    paths: &[PathBuf],
    kind_override: Option<&str>,
) -> Vec<ImportOutcome> {
    let mut out = Vec::new();
    for (i, p) in paths.iter().enumerate() {
        if i >= MAX_BATCH {
            out.push(ImportOutcome {
                path: p.to_string_lossy().to_string(),
                file_name: p
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
                kind: "unknown".into(),
                saved_as: None,
                target_path: None,
                size_bytes: 0,
                sha1: None,
                renamed: false,
                error: Some(format!("一次最多导入 {MAX_BATCH} 个文件，剩下的请分批")),
            });
            continue;
        }
        match import_one(targets, p, kind_override) {
            Ok(o) => out.push(o),
            Err(e) => out.push(ImportOutcome {
                path: p.to_string_lossy().to_string(),
                file_name: p
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
                kind: "unknown".into(),
                saved_as: None,
                target_path: None,
                size_bytes: 0,
                sha1: None,
                renamed: false,
                error: Some(e),
            }),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("astral-import-{tag}-{}", crate::time::now_nanos()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn make_zip(path: &Path, entries: &[(&str, &str)]) {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).unwrap();
        }
        let f = std::fs::File::create(path).unwrap();
        let mut z = zip::ZipWriter::new(f);
        let opts = zip::write::SimpleFileOptions::default();
        for (name, body) in entries {
            z.start_file(*name, opts).unwrap();
            z.write_all(body.as_bytes()).unwrap();
        }
        z.finish().unwrap();
    }

    fn targets(root: &Path) -> Targets {
        Targets {
            mods: root.join("mods"),
            resourcepacks: root.join("resourcepacks"),
            shaderpacks: root.join("shaderpacks"),
        }
    }

    #[test]
    fn detects_kinds_by_content() {
        let d = tmp("detect");
        let rp = d.join("a.zip");
        make_zip(
            &rp,
            &[
                (
                    "pack.mcmeta",
                    r#"{"pack":{"pack_format":15,"description":"x"}}"#,
                ),
                ("assets/minecraft/lang/zh_cn.json", "{}"),
            ],
        );
        assert_eq!(detect(&rp).0, "resourcepack");

        let sp = d.join("b.zip");
        make_zip(&sp, &[("shaders/final.fsh", "void main(){}")]);
        assert_eq!(detect(&sp).0, "shaderpack");

        let dp = d.join("c.zip");
        make_zip(
            &dp,
            &[
                ("pack.mcmeta", r#"{"pack":{"pack_format":15}}"#),
                ("data/x/functions/a.mcfunction", "say hi"),
            ],
        );
        assert_eq!(detect(&dp).0, "datapack");

        let mp = d.join("d.zip");
        make_zip(&mp, &[("modrinth.index.json", "{}")]);
        assert_eq!(detect(&mp).0, "modpack");

        let jar = d.join("e.jar");
        std::fs::write(&jar, b"not really a jar").unwrap();
        assert_eq!(detect(&jar).0, "mod");

        let txt = d.join("f.txt");
        std::fs::write(&txt, b"x").unwrap();
        assert_eq!(detect(&txt).0, "unknown");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn rejects_missing_and_oversized() {
        let d = tmp("reject");
        let c = inspect(&d.join("nope.jar"), None);
        assert!(!c.ok());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn imports_into_the_right_directory() {
        let d = tmp("import");
        let t = targets(&d);
        let src = d.join("cool.zip");
        make_zip(
            &src,
            &[
                ("pack.mcmeta", r#"{"pack":{"pack_format":15}}"#),
                ("assets/minecraft/lang/zh_cn.json", "{}"),
            ],
        );
        let o = import_one(&t, &src, None).unwrap();
        assert_eq!(o.kind, "resourcepack");
        assert!(!o.renamed);
        assert!(d.join("resourcepacks/cool.zip").is_file());
        assert!(o.sha1.is_some());
        assert_eq!(o.size_bytes, std::fs::metadata(&src).unwrap().len());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn renames_on_conflict_instead_of_overwriting() {
        let d = tmp("conflict");
        let t = targets(&d);
        let src = d.join("same.jar");
        std::fs::write(&src, b"first").unwrap();
        let a = import_one(&t, &src, None).unwrap();
        assert_eq!(a.saved_as.as_deref(), Some("same.jar"));
        std::fs::write(&src, b"second").unwrap();
        let b = import_one(&t, &src, None).unwrap();
        assert_eq!(b.saved_as.as_deref(), Some("same-1.jar"));
        assert!(b.renamed);
        // 第一个文件内容没被改
        assert_eq!(
            std::fs::read(d.join("mods/same.jar")).unwrap(),
            b"first".to_vec()
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn batch_keeps_going_after_one_failure() {
        let d = tmp("batch");
        let t = targets(&d);
        let good = d.join("ok.jar");
        std::fs::write(&good, b"x").unwrap();
        let paths = vec![d.join("missing.jar"), good.clone()];
        let out = import_many(&t, &paths, None);
        assert_eq!(out.len(), 2);
        assert!(out[0].error.is_some());
        assert!(out[1].error.is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn kind_override_wins_for_ambiguous_zip() {
        let d = tmp("override");
        let t = targets(&d);
        let src = d.join("weird.zip");
        // 只有 pack.mcmeta，没有 assets：默认会被当成结构不完整的资源包
        make_zip(&src, &[("pack.mcmeta", r#"{"pack":{"pack_format":15}}"#)]);
        assert_eq!(detect(&src).0, "resourcepack");
        let o = import_one(&t, &src, Some("shaderpack")).unwrap();
        assert_eq!(o.kind, "shaderpack");
        assert!(d.join("shaderpacks/weird.zip").is_file());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn rejects_unknown_override_kind() {
        let d = tmp("badkind");
        let t = targets(&d);
        let src = d.join("x.jar");
        std::fs::write(&src, b"x").unwrap();
        assert!(import_one(&t, &src, Some("taco")).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
