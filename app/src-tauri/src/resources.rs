//! Modrinth 资源查询与定向安装（契约 V2 §5）。
//!
//! 原则：目标目录由 Rust 根据 instanceId / worldId 解析，前端不传路径；
//! 安装前有计划、有兼容性检查、有覆盖确认；下载走 net 的白名单与原子写。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const API: &str = "https://api.modrinth.com/v2";
const TTL_MS: i64 = 15 * 60 * 1000;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ResourceProject {
    pub id: String,
    pub title: String,
    pub description: String,
    pub icon_url: Option<String>,
    pub downloads: u64,
    pub updated_at: Option<String>,
    pub source_url: Option<String>,
    pub license_id: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ResourceDep {
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    pub kind: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ResourceFile {
    pub version_id: String,
    pub name: String,
    pub version_number: String,
    pub channel: String,
    pub published_at: String,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub file_name: String,
    pub url: String,
    pub size: u64,
    pub sha1: Option<String>,
    pub sha512: Option<String>,
    pub dependencies: Vec<ResourceDep>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ResourceInstallRequest {
    pub instance_id: String,
    pub kind: String, // mod | resourcepack | shaderpack | datapack
    pub project_id: String,
    pub version_id: String,
    #[serde(default)]
    pub world_id: Option<String>,
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ResourcePlan {
    pub plan_id: String,
    /// 目标实例，任务与保护逻辑都依赖它
    pub instance_id: String,
    pub revision: u32,
    pub expires_at: String,
    pub kind: String,
    pub project_id: String,
    pub version_id: String,
    pub file_name: String,
    pub target_dir: String,
    pub download_bytes: u64,
    pub replaces: Option<String>,
    pub issues: Vec<crate::plan::PlanIssue>,
    /// 解析得到的必需依赖（按顺序先装）
    pub dependencies: Vec<PlannedDep>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PlannedDep {
    pub project_id: String,
    pub version_id: String,
    pub file_name: String,
    pub size: u64,
    pub sha1: Option<String>,
    pub sha512: Option<String>,
}

pub struct StoredResourcePlan {
    pub plan: ResourcePlan,
    pub url: String,
    pub sha1: Option<String>,
    pub sha512: Option<String>,
    pub dep_urls: Vec<(String, String)>, // (url, file_name)
    pub created_ms: i64,
}

static PLANS: Mutex<Option<HashMap<String, StoredResourcePlan>>> = Mutex::new(None);

fn store() -> std::sync::MutexGuard<'static, Option<HashMap<String, StoredResourcePlan>>> {
    PLANS.lock().unwrap()
}

fn issue(code: &str, message: impl Into<String>, blocking: bool) -> crate::plan::PlanIssue {
    crate::plan::PlanIssue {
        code: code.to_string(),
        message: message.into(),
        blocking,
    }
}

fn str_of(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

fn str_list(v: &serde_json::Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str())
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default()
}

pub fn project(project_id: &str) -> Result<ResourceProject, String> {
    let v = crate::net::fetch_json(&format!("{API}/project/{project_id}"))?;
    Ok(ResourceProject {
        id: str_of(&v, "id").unwrap_or_else(|| project_id.to_string()),
        title: str_of(&v, "title").unwrap_or_default(),
        description: str_of(&v, "description").unwrap_or_default(),
        icon_url: str_of(&v, "icon_url"),
        downloads: v.get("downloads").and_then(|d| d.as_u64()).unwrap_or(0),
        updated_at: str_of(&v, "updated"),
        source_url: str_of(&v, "source_url"),
        license_id: v
            .get("license")
            .and_then(|l| l.get("id"))
            .and_then(|i| i.as_str())
            .map(|s| s.to_string()),
    })
}

fn parse_file(v: &serde_json::Value) -> Option<ResourceFile> {
    let files = v.get("files")?.as_array()?;
    // 只取主文件
    let f = files
        .iter()
        .find(|f| f.get("primary").and_then(|p| p.as_bool()).unwrap_or(false))
        .or_else(|| files.first())?;
    let hashes = f.get("hashes");
    let deps = v
        .get("dependencies")
        .and_then(|d| d.as_array())
        .map(|a| {
            a.iter()
                .map(|d| ResourceDep {
                    project_id: str_of(d, "project_id"),
                    version_id: str_of(d, "version_id"),
                    kind: str_of(d, "dependency_type").unwrap_or_else(|| "required".into()),
                })
                .collect()
        })
        .unwrap_or_default();

    Some(ResourceFile {
        version_id: str_of(v, "id")?,
        name: str_of(v, "name").unwrap_or_default(),
        version_number: str_of(v, "version_number").unwrap_or_default(),
        channel: str_of(v, "version_type").unwrap_or_else(|| "release".into()),
        published_at: str_of(v, "date_published").unwrap_or_default(),
        game_versions: str_list(v, "game_versions"),
        loaders: str_list(v, "loaders"),
        file_name: str_of(f, "filename")?,
        url: str_of(f, "url")?,
        size: f.get("size").and_then(|s| s.as_u64()).unwrap_or(0),
        sha1: hashes.and_then(|h| str_of(h, "sha1")),
        sha512: hashes.and_then(|h| str_of(h, "sha512")),
        dependencies: deps,
    })
}

/// 项目的全部可安装文件，可按 MC 版本与加载器过滤
pub fn files(
    project_id: &str,
    game_version: Option<&str>,
    loader: Option<&str>,
) -> Result<Vec<ResourceFile>, String> {
    let arr = crate::net::fetch_json(&format!("{API}/project/{project_id}/version"))?;
    let list = arr
        .as_array()
        .ok_or_else(|| "INTERNAL|文件列表格式异常".to_string())?;

    let mut out: Vec<ResourceFile> = list.iter().filter_map(parse_file).collect();

    if let Some(gv) = game_version {
        out.retain(|f| f.game_versions.iter().any(|g| g == gv));
    }
    if let Some(l) = loader {
        out.retain(|f| {
            f.loaders.is_empty() || f.loaders.iter().any(|x| x.eq_ignore_ascii_case(l))
        });
    }
    Ok(out)
}

/// 版本详情 + 它所属的项目 id（用于校验请求里的 projectId）
fn version_detail(version_id: &str) -> Result<(ResourceFile, Option<String>), String> {
    let v = crate::net::fetch_json(&format!("{API}/version/{version_id}"))?;
    let file = parse_file(&v).ok_or_else(|| "VERSION_NOT_FOUND|拿不到该文件信息".to_string())?;
    let project_id = v
        .get("project_id")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    Ok((file, project_id))
}

fn target_dir(instance: &crate::instance::Instance, kind: &str, world_id: Option<&str>) -> Result<PathBuf, String> {
    let base = PathBuf::from(&instance.dir);
    match kind {
        "mod" => Ok(PathBuf::from(&instance.dirs.mods)),
        "resourcepack" => Ok(PathBuf::from(&instance.dirs.resourcepacks)),
        "shaderpack" => Ok(PathBuf::from(&instance.dirs.shaderpacks)),
        "datapack" => {
            let wid = world_id.ok_or_else(|| "INVALID_VALUE|数据包必须指定存档".to_string())?;
            let saves = Path::new(&instance.dirs.saves);
            let world = crate::worlds::resolve(saves, wid)
                .ok_or_else(|| "TARGET_CHANGED|存档不存在（可能已被移动或删除）".to_string())?;
            Ok(world.join("datapacks"))
        }
        other => Err(format!("UNSUPPORTED|未知资源类型 {other}")),
    }
    .map(|p| {
        let _ = base;
        p
    })
}

/// 规划安装：只做校验，不下载、不写文件
pub fn prepare(
    game: &Path,
    request: ResourceInstallRequest,
    stats: &dyn Fn(&str) -> crate::instance::PlayStats,
) -> Result<ResourcePlan, String> {
    let inst = crate::instance::find(game, &request.instance_id, stats)
        .ok_or_else(|| "INSTANCE_NOT_FOUND|实例不存在".to_string())?;

    let mut issues = Vec::new();
    // 只允许托管实例：兼容实例的 mods/config/saves 指向共享目录
    if !inst.managed {
        return Err("UNSUPPORTED|兼容实例共享游戏目录，不能安装资源".into());
    }

    let (file, file_project) = version_detail(&request.version_id)?;

    // 版本必须属于请求的项目
    if let Some(vp) = &file_project {
        if vp != &request.project_id {
            return Err("INVALID_VALUE|版本与项目不匹配".into());
        }
    }

    // 文件名必须是单层相对名
    let safe_name = crate::modpack::safe_rel_path(&file.file_name)?;
    if safe_name.components().count() != 1 {
        return Err("INVALID_PATH|资源文件名不能包含目录".into());
    }

    // 兼容性
    let needs_mc = matches!(request.kind.as_str(), "mod" | "datapack");
    if needs_mc && !file.game_versions.iter().any(|g| g == &inst.game_version) {
        issues.push(issue(
            "INCOMPATIBLE",
            format!(
                "该文件不支持 Minecraft {}（支持：{}）",
                inst.game_version,
                file.game_versions.join(", ")
            ),
            true,
        ));
    }
    if needs_mc {
        if let Some(loader) = &inst.loader {
            let ok = file.loaders.is_empty()
                || file
                    .loaders
                    .iter()
                    .any(|l| l.eq_ignore_ascii_case(&loader.kind));
            if !ok {
                issues.push(issue(
                    "INCOMPATIBLE",
                    format!(
                        "该文件不支持 {}（支持：{}）",
                        loader.kind,
                        file.loaders.join(", ")
                    ),
                    true,
                ));
            }
        }
    }

    // 资源包/光影包跨版本通常可用，但不相符时给出非阻塞提示
    if !needs_mc
        && !file.game_versions.is_empty()
        && !file.game_versions.iter().any(|g| g == &inst.game_version)
    {
        issues.push(issue(
            "VERSION_HINT",
            format!(
                "该文件标注支持 {}，与实例版本 {} 不一致，通常仍可使用",
                file.game_versions.join(", "),
                inst.game_version
            ),
            false,
        ));
    }

    let dir = target_dir(&inst, &request.kind, request.world_id.as_deref())?;
    let dest = dir.join(&file.file_name);
    let replaces = if dest.exists() {
        if !request.overwrite {
            issues.push(issue(
                "FILE_EXISTS",
                format!("目标目录已有同名文件：{}", file.file_name),
                true,
            ));
        }
        Some(dest.to_string_lossy().to_string())
    } else {
        None
    };

    // 解析必需依赖：目标目录里已有同名文件就跳过（视为已安装）
    let mut deps: Vec<PlannedDep> = Vec::new();
    let mut dep_urls: Vec<(String, String)> = Vec::new();
    if needs_mc {
        let loader_kind = inst.loader.as_ref().map(|l| l.kind.clone());
        let mut checked = 0;
        for d in &file.dependencies {
            if d.kind != "required" {
                continue;
            }
            if checked >= 10 {
                break;
            }
            checked += 1;
            let Some(pid) = &d.project_id else { continue };
            let candidate = match &d.version_id {
                // 指定了版本：必须确实属于这个项目，且与实例兼容
                Some(vid) => match version_detail(vid) {
                    Ok((df, vp)) => {
                        if vp.as_deref().map(|p| p != pid).unwrap_or(false) {
                            issues.push(issue(
                                "DEPENDENCY_CONFLICT",
                                format!("依赖版本 {vid} 不属于项目 {pid}"),
                                true,
                            ));
                            continue;
                        }
                        let compat_ok = !needs_mc
                            || (df.game_versions.iter().any(|g| g == &inst.game_version)
                                && loader_kind
                                    .as_ref()
                                    .map(|k| df.loaders.iter().any(|l| l == k))
                                    .unwrap_or(true));
                        if !compat_ok {
                            issues.push(issue(
                                "DEPENDENCY_CONFLICT",
                                format!(
                                    "依赖 {pid} 的指定版本与 {} 不兼容",
                                    inst.game_version
                                ),
                                true,
                            ));
                            continue;
                        }
                        Some(df)
                    }
                    Err(_) => None,
                },
                // 没指定：取兼容版本里最稳的（release > beta > alpha）
                None => files(pid, Some(&inst.game_version), loader_kind.as_deref())
                    .ok()
                    .and_then(|mut v| {
                        v.sort_by_key(|f| match f.channel.as_str() {
                            "release" => 0,
                            "beta" => 1,
                            _ => 2,
                        });
                        v.into_iter().next()
                    }),
            };
            let Some(df) = candidate else {
                issues.push(issue(
                    "DEPENDENCY_CONFLICT",
                    format!("依赖 {pid} 找不到兼容版本，需要手动处理"),
                    true,
                ));
                continue;
            };
            if dir.join(&df.file_name).exists() {
                continue;
            }
            if deps.iter().any(|x| x.file_name == df.file_name) {
                continue;
            }
            // 同一个项目被解析成两个不同版本 → 装上去会互相覆盖，直接判冲突
            if let Some(prev) = deps.iter().find(|x| x.project_id == *pid) {
                if prev.version_id != df.version_id {
                    issues.push(issue(
                        "DEPENDENCY_CONFLICT",
                        format!(
                            "依赖 {pid} 同时被要求 {} 与 {}，需要手动选一个",
                            prev.version_id, df.version_id
                        ),
                        true,
                    ));
                    continue;
                }
            }
            dep_urls.push((df.url.clone(), df.file_name.clone()));
            deps.push(PlannedDep {
                project_id: pid.clone(),
                version_id: df.version_id.clone(),
                file_name: df.file_name.clone(),
                size: df.size,
                sha1: df.sha1.clone(),
                sha512: df.sha512.clone(),
            });
        }
    }

    let total_bytes = file.size + deps.iter().map(|d| d.size).sum::<u64>();

    let plan = ResourcePlan {
        plan_id: format!("rplan_{:x}", crate::time::now_unix_ms()),
        instance_id: request.instance_id.clone(),
        revision: 1,
        expires_at: crate::time::unix_ms_to_iso(crate::time::now_unix_ms() + TTL_MS),
        kind: request.kind.clone(),
        project_id: request.project_id.clone(),
        version_id: request.version_id.clone(),
        file_name: file.file_name.clone(),
        target_dir: dir.to_string_lossy().to_string(),
        download_bytes: total_bytes,
        replaces,
        issues,
        dependencies: deps,
    };

    let mut s = store();
    let map = s.get_or_insert_with(HashMap::new);
    let now = crate::time::now_unix_ms();
    map.retain(|_, p| now - p.created_ms < TTL_MS);
    map.insert(
        plan.plan_id.clone(),
        StoredResourcePlan {
            plan: plan.clone(),
            url: file.url.clone(),
            sha1: file.sha1.clone(),
            sha512: file.sha512.clone(),
            dep_urls,
            created_ms: now,
        },
    );
    Ok(plan)
}

pub fn take(plan_id: &str, revision: u32) -> Result<StoredResourcePlan, String> {
    let mut s = store();
    let map = s
        .as_mut()
        .ok_or_else(|| "PLAN_EXPIRED|没有可用计划".to_string())?;
    let stored = map
        .remove(plan_id)
        .ok_or_else(|| "PLAN_EXPIRED|计划不存在或已使用".to_string())?;
    let now = crate::time::now_unix_ms();
    if now - stored.created_ms >= TTL_MS {
        return Err("PLAN_EXPIRED|计划已过期".into());
    }
    if stored.plan.revision != revision {
        return Err("PLAN_EXPIRED|计划已变更".into());
    }
    Ok(stored)
}

/// 真正下载并落盘（调用方已在任务线程里）
pub fn execute(
    stored: &StoredResourcePlan,
    should_cancel: impl Fn() -> bool,
    mut on: impl FnMut(u32, u32),
) -> Result<PathBuf, String> {
    let dir = PathBuf::from(&stored.plan.target_dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("PERMISSION|创建目录失败|{e}"))?;

    if should_cancel() {
        return Err("CANCELLED|已取消".into());
    }

    // 先装必需依赖，再装本体；两者都在 .part 阶段完成 SHA1/SHA512 校验
    for (i, dep) in stored.plan.dependencies.iter().enumerate() {
        if should_cancel() {
            return Err("CANCELLED|已取消".into());
        }
        let (url, _name) = &stored.dep_urls[i];
        let dep_dest = dir.join(&dep.file_name);
        crate::net::download_verified(
            url,
            &dep_dest,
            dep.sha1.as_deref(),
            dep.sha512.as_deref(),
            |c, t, _s, _ts| {
                on(c, t);
            },
        )?;
    }

    if should_cancel() {
        return Err("CANCELLED|已取消".into());
    }
    let dest = dir.join(&stored.plan.file_name);
    crate::net::download_verified(
        &stored.url,
        &dest,
        stored.sha1.as_deref(),
        stored.sha512.as_deref(),
        |c, t, _s, _ts| {
            on(c, t);
        },
    )?;

    Ok(dest)

}

/// 测试辅助：把某个项目的一个兼容版本装进实例，返回落地路径。
/// 只在自检/诊断里用，不对外暴露命令。
pub fn live_probe(
    game: &Path,
    instance_id: &str,
    kind: &str,
    project_id: &str,
    world_id: Option<&str>,
    game_version: &str,
    loader: Option<&str>,
) -> Result<String, String> {
    // 资源包/光影包与游戏版本无关，不按版本过滤
    let version_filter = if kind == "resourcepack" || kind == "shaderpack" {
        None
    } else {
        Some(game_version)
    };
    let loader_filter = if kind == "mod" || kind == "datapack" {
        loader
    } else {
        None
    };
    let files = files(project_id, version_filter, loader_filter)?;
    let first = files
        .first()
        .ok_or_else(|| format!("VERSION_NOT_FOUND|{project_id} 没有兼容文件"))?;
    let req = ResourceInstallRequest {
        instance_id: instance_id.to_string(),
        kind: kind.to_string(),
        project_id: project_id.to_string(),
        version_id: first.version_id.clone(),
        world_id: world_id.map(|s| s.to_string()),
        overwrite: false,
    };
    let stats = |_: &str| crate::instance::PlayStats::default();
    let plan = prepare(game, req, &stats)?;
    if let Some(b) = plan.issues.iter().find(|i| i.blocking) {
        return Err(format!("{}|{}", b.code, b.message));
    }
    let stored = take(&plan.plan_id, 1)?;
    let p = execute(&stored, || false, |_, _| {})?;
    Ok(p.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_primary_file() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"id":"V1","name":"Sodium 1.0","version_number":"1.0","version_type":"release",
                "date_published":"2024-01-01","game_versions":["1.20.1"],"loaders":["fabric"],
                "files":[{"url":"https://cdn.modrinth.com/x.jar","filename":"sodium.jar","size":10,
                "primary":true,"hashes":{"sha1":"aa","sha512":"bb"}}],
                "dependencies":[{"project_id":"P2","version_id":null,"dependency_type":"required"}]}"#,
        )
        .unwrap();
        let f = parse_file(&v).unwrap();
        assert_eq!(f.version_id, "V1");
        assert_eq!(f.file_name, "sodium.jar");
        assert_eq!(f.sha512.as_deref(), Some("bb"));
        assert_eq!(f.dependencies.len(), 1);
    }
}
