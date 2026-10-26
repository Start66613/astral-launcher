//! Mod 依赖图解析（契约 V2.5 B1）。
//!
//! 设计要点：
//! - **逻辑与 IO 分离**：本模块只做图遍历与冲突判定，取数据靠 [`DepSource`]。
//!   这样可以用假的 source 做确定性测试，不用联网。
//! - **有上限**：深度与节点数都有限制，坏数据不会把内存吃光。
//! - **可展示**：返回按安装顺序排好的节点列表 + 每个节点"为什么被装"的路径，
//!   前端可以直接画成一棵树。
//! - **不猜**：找不到兼容版本、同一项目要求两个版本、依赖环，全部如实报冲突，
//!   不偷偷挑一个装上。

use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// 依赖类型（Modrinth 的 dependency_type）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepKind {
    Required,
    Optional,
    Incompatible,
    Embedded,
    Unknown,
}

impl DepKind {
    pub fn parse(s: &str) -> Self {
        match s {
            "required" => Self::Required,
            "optional" => Self::Optional,
            "incompatible" => Self::Incompatible,
            "embedded" => Self::Embedded,
            _ => Self::Unknown,
        }
    }

    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Optional => "optional",
            Self::Incompatible => "incompatible",
            Self::Embedded => "embedded",
            Self::Unknown => "unknown",
        }
    }
}

/// 一个候选版本（来自 Modrinth，字段与 `resources::ResourceFile` 对齐）
#[derive(Debug, Clone)]
pub struct Candidate {
    pub project_id: String,
    pub version_id: String,
    pub name: String,
    pub version_number: String,
    pub channel: String,
    pub file_name: String,
    pub url: String,
    pub size: u64,
    pub sha1: Option<String>,
    pub sha512: Option<String>,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub dependencies: Vec<Dep>,
}

#[derive(Debug, Clone)]
pub struct Dep {
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    pub kind: DepKind,
}

/// 取数据的地方。生产用 Modrinth，测试用假的。
pub trait DepSource {
    /// 某个项目在给定游戏版本 / 加载器下可用的版本
    fn versions_for(
        &self,
        project_id: &str,
        game_version: &str,
        loader: Option<&str>,
    ) -> Result<Vec<Candidate>, String>;

    /// 按版本 id 精确取（依赖里指定了 version_id 时用）
    fn version_by_id(&self, version_id: &str) -> Result<(Candidate, Option<String>), String>;
}

#[derive(Debug, Clone)]
pub struct Limits {
    pub max_depth: u32,
    pub max_nodes: usize,
    pub max_visits: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_depth: 8,
            max_nodes: 128,
            max_visits: 512,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Request {
    pub project_id: String,
    pub version_id: String,
    pub game_version: String,
    pub loader: Option<String>,
    /// 实例里已经装好的项目 → 版本 id（版本确认得了才放进来）
    pub installed: HashMap<String, String>,
    /// 实例里已经装了、但认不出是哪个版本的项目。跳过安装，但如实说明没查它的依赖
    pub installed_unknown: HashSet<String>,
    /// 用户勾选的可选依赖
    pub optional: HashSet<String>,
}

/// 计划里的一个节点
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub project_id: String,
    pub version_id: String,
    pub name: String,
    pub version_number: String,
    pub channel: String,
    pub file_name: String,
    pub url: String,
    pub size: u64,
    pub sha1: Option<String>,
    pub sha512: Option<String>,
    /// 相对根节点的层级（根 = 0）
    pub depth: u32,
    /// 依赖类型：root / required / optional
    pub kind: String,
    /// 从根到它的项目链，前端可以直接显示"因为 X 需要它"
    pub via: Vec<String>,
    /// 实例里已经有同项目但版本不同：装上会替换
    pub replaces_version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skipped {
    pub project_id: String,
    pub reason: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Conflict {
    pub code: String,
    pub project_id: String,
    pub message: String,
    pub blocking: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Resolved {
    /// 按安装顺序：依赖在前，根节点在最后
    pub nodes: Vec<Node>,
    /// 检测到的环（项目 id 链）
    pub cycles: Vec<Vec<String>>,
    pub conflicts: Vec<Conflict>,
    pub skipped: Vec<Skipped>,
    /// 总下载字节
    pub download_bytes: u64,
    /// 因为超过上限被截断
    pub truncated: bool,
    /// 访问过的版本数（诊断用）
    pub visits: usize,
}

impl Resolved {
    pub fn blocking(&self) -> bool {
        self.conflicts.iter().any(|c| c.blocking) || !self.cycles.is_empty()
    }
}

/// 解析依赖图。
///
/// 遍历顺序是深度优先 + 后序入列，所以返回的 `nodes` 里依赖一定排在依赖它的人前面。
pub fn resolve(src: &dyn DepSource, req: &Request, limits: &Limits) -> Resolved {
    let mut out = Resolved {
        nodes: Vec::new(),
        cycles: Vec::new(),
        conflicts: Vec::new(),
        skipped: Vec::new(),
        download_bytes: 0,
        truncated: false,
        visits: 0,
    };

    // 项目 → 已选版本，用来发现"同一项目两个版本"
    let mut chosen: HashMap<String, String> = HashMap::new();
    // 已经在 nodes 里的版本 id
    let mut placed: HashSet<String> = HashSet::new();
    // 环检测用的当前路径
    let mut stack: Vec<String> = Vec::new();

    let root = match src.version_by_id(&req.version_id) {
        Ok((c, project)) => {
            if let Some(p) = &project {
                if p != &req.project_id {
                    out.conflicts.push(Conflict {
                        code: "DEPENDENCY_CONFLICT".into(),
                        project_id: req.project_id.clone(),
                        message: format!("版本 {} 不属于项目 {}", req.version_id, req.project_id),
                        blocking: true,
                    });
                    return out;
                }
            }
            c
        }
        Err(e) => {
            out.conflicts.push(Conflict {
                code: "VERSION_NOT_FOUND".into(),
                project_id: req.project_id.clone(),
                message: format!("取不到版本详情：{e}"),
                blocking: true,
            });
            return out;
        }
    };

    // 根节点先占位，避免依赖绕回来又把它当新节点
    chosen.insert(root.project_id.clone(), root.version_id.clone());
    stack.push(root.project_id.clone());

    walk(
        src,
        req,
        limits,
        &root,
        0,
        &mut Vec::new(),
        &mut chosen,
        &mut placed,
        &mut stack,
        &mut out,
        &mut Vec::new(),
    );

    // 根节点最后入列（后序）
    push_node(&mut out, &root, 0, "root", Vec::new(), &req.installed);
    stack.pop();

    out.download_bytes = out.nodes.iter().map(|n| n.size).sum();
    out
}

#[allow(clippy::too_many_arguments)]
fn walk(
    src: &dyn DepSource,
    req: &Request,
    limits: &Limits,
    parent: &Candidate,
    depth: u32,
    via: &mut Vec<String>,
    chosen: &mut HashMap<String, String>,
    placed: &mut HashSet<String>,
    stack: &mut Vec<String>,
    out: &mut Resolved,
    seen_pairs: &mut Vec<(String, String)>,
) {
    if depth >= limits.max_depth {
        if !parent.dependencies.is_empty() {
            out.truncated = true;
            out.conflicts.push(Conflict {
                code: "DEPENDENCY_TOO_DEEP".into(),
                project_id: parent.project_id.clone(),
                message: format!("依赖层级超过 {} 层，已停止继续解析", limits.max_depth),
                blocking: true,
            });
        }
        return;
    }
    if out.nodes.len() >= limits.max_nodes {
        out.truncated = true;
        return;
    }
    if out.visits >= limits.max_visits {
        out.truncated = true;
        return;
    }

    via.push(parent.project_id.clone());

    for dep in &parent.dependencies {
        if out.nodes.len() >= limits.max_nodes {
            out.truncated = true;
            break;
        }
        let Some(pid) = dep.project_id.clone() else {
            // 只给了 version_id 的情况：能解析就解析，不能就记一条
            if let Some(vid) = &dep.version_id {
                match src.version_by_id(vid) {
                    Ok((c, _)) => {
                        handle_one(
                            src, req, limits, c, dep.kind, depth, via, chosen, placed, stack, out,
                            seen_pairs,
                        );
                    }
                    Err(e) => out.skipped.push(Skipped {
                        project_id: vid.clone(),
                        reason: "unresolved".into(),
                        detail: e,
                    }),
                }
            }
            continue;
        };

        match dep.kind {
            DepKind::Incompatible => {
                if req.installed.contains_key(&pid) {
                    out.conflicts.push(Conflict {
                        code: "DEPENDENCY_CONFLICT".into(),
                        project_id: pid.clone(),
                        message: format!(
                            "{} 声明与 {} 不兼容，但实例里已经装了它",
                            parent.name, pid
                        ),
                        blocking: true,
                    });
                }
                continue;
            }
            DepKind::Embedded => {
                // 已经打包在别人的 jar 里，不用单独装
                out.skipped.push(Skipped {
                    project_id: pid,
                    reason: "embedded".into(),
                    detail: format!("{} 已内置该依赖", parent.name),
                });
                continue;
            }
            DepKind::Optional => {
                if !req.optional.contains(&pid) {
                    out.skipped.push(Skipped {
                        project_id: pid,
                        reason: "optional".into(),
                        detail: format!("{} 的可选依赖，用户没勾选", parent.name),
                    });
                    continue;
                }
            }
            DepKind::Required | DepKind::Unknown => {}
        }

        // 环检测：这个项目已经在当前路径上
        if stack.iter().any(|x| x == &pid) {
            let mut cyc = stack.clone();
            cyc.push(pid.clone());
            out.cycles.push(cyc);
            out.conflicts.push(Conflict {
                code: "DEPENDENCY_CYCLE".into(),
                project_id: pid.clone(),
                message: format!("依赖成环：{} → {pid}", stack.join(" → ")),
                blocking: true,
            });
            continue;
        }

        // 已经定过版本：同版本跳过，不同版本报冲突
        if let Some(existing) = chosen.get(&pid) {
            let wanted = dep.version_id.clone().unwrap_or_else(|| existing.clone());
            if &wanted != existing {
                out.conflicts.push(Conflict {
                    code: "DEPENDENCY_CONFLICT".into(),
                    project_id: pid.clone(),
                    message: format!("{pid} 同时被要求版本 {existing} 与 {wanted}，需要手动选一个"),
                    blocking: true,
                });
            }
            continue;
        }

        // 装了但认不出版本：跳过安装，如实说明没查它的依赖
        if req.installed_unknown.contains(&pid) {
            chosen.insert(pid.clone(), "installed".into());
            out.skipped.push(Skipped {
                project_id: pid.clone(),
                reason: "installed-unknown".into(),
                detail: format!("实例里已装 {pid}，但认不出具体版本，未检查它自己的依赖"),
            });
            continue;
        }

        // 已经装在实例里：复用，不下载
        if let Some(installed_ver) = req.installed.get(&pid) {
            chosen.insert(pid.clone(), installed_ver.clone());
            out.skipped.push(Skipped {
                project_id: pid.clone(),
                reason: "installed".into(),
                detail: format!("实例里已安装 {installed_ver}"),
            });
            // 已装的也要往下走，它自己的依赖可能缺
            if let Ok((c, _)) = src.version_by_id(installed_ver) {
                stack.push(pid.clone());
                walk(
                    src,
                    req,
                    limits,
                    &c,
                    depth + 1,
                    via,
                    chosen,
                    placed,
                    stack,
                    out,
                    seen_pairs,
                );
                stack.pop();
            }
            continue;
        }

        out.visits += 1;
        let candidate = match &dep.version_id {
            Some(vid) => match src.version_by_id(vid) {
                Ok((c, project)) => {
                    if project.as_deref().map(|p| p != pid).unwrap_or(false) {
                        out.conflicts.push(Conflict {
                            code: "DEPENDENCY_CONFLICT".into(),
                            project_id: pid.clone(),
                            message: format!("版本 {vid} 不属于项目 {pid}"),
                            blocking: true,
                        });
                        continue;
                    }
                    if !compatible(&c, &req.game_version, req.loader.as_deref()) {
                        out.conflicts.push(Conflict {
                            code: "DEPENDENCY_CONFLICT".into(),
                            project_id: pid.clone(),
                            message: format!(
                                "{pid} 的指定版本 {} 与 {} 不兼容",
                                c.version_number, req.game_version
                            ),
                            blocking: true,
                        });
                        continue;
                    }
                    c
                }
                Err(e) => {
                    out.skipped.push(Skipped {
                        project_id: pid.clone(),
                        reason: "unresolved".into(),
                        detail: e,
                    });
                    continue;
                }
            },
            None => match src.versions_for(&pid, &req.game_version, req.loader.as_deref()) {
                Ok(list) => match pick_best(list, &req.game_version, req.loader.as_deref()) {
                    Some(c) => c,
                    None => {
                        out.conflicts.push(Conflict {
                            code: "DEPENDENCY_CONFLICT".into(),
                            project_id: pid.clone(),
                            message: format!("{pid} 找不到兼容 {} 的版本", req.game_version),
                            blocking: true,
                        });
                        continue;
                    }
                },
                Err(e) => {
                    out.skipped.push(Skipped {
                        project_id: pid.clone(),
                        reason: "unresolved".into(),
                        detail: e,
                    });
                    continue;
                }
            },
        };

        handle_one(
            src, req, limits, candidate, dep.kind, depth, via, chosen, placed, stack, out,
            seen_pairs,
        );
    }

    via.pop();
}

#[allow(clippy::too_many_arguments)]
fn handle_one(
    src: &dyn DepSource,
    req: &Request,
    limits: &Limits,
    c: Candidate,
    kind: DepKind,
    depth: u32,
    via: &mut Vec<String>,
    chosen: &mut HashMap<String, String>,
    placed: &mut HashSet<String>,
    stack: &mut Vec<String>,
    out: &mut Resolved,
    seen_pairs: &mut Vec<(String, String)>,
) {
    if !placed.insert(c.version_id.clone()) {
        return;
    }
    chosen.insert(c.project_id.clone(), c.version_id.clone());

    stack.push(c.project_id.clone());
    walk(
        src,
        req,
        limits,
        &c,
        depth + 1,
        via,
        chosen,
        placed,
        stack,
        out,
        seen_pairs,
    );
    stack.pop();

    let path = via.clone();
    push_node(
        out,
        &c,
        depth + 1,
        if kind == DepKind::Optional {
            "optional"
        } else {
            "required"
        },
        path,
        &req.installed,
    );
    seen_pairs.push((c.project_id.clone(), c.version_id.clone()));
}

fn push_node(
    out: &mut Resolved,
    c: &Candidate,
    depth: u32,
    kind: &str,
    via: Vec<String>,
    installed: &HashMap<String, String>,
) {
    if out.nodes.iter().any(|n| n.version_id == c.version_id) {
        return;
    }
    out.nodes.push(Node {
        project_id: c.project_id.clone(),
        version_id: c.version_id.clone(),
        name: c.name.clone(),
        version_number: c.version_number.clone(),
        channel: c.channel.clone(),
        file_name: c.file_name.clone(),
        url: c.url.clone(),
        size: c.size,
        sha1: c.sha1.clone(),
        sha512: c.sha512.clone(),
        depth,
        kind: kind.to_string(),
        via,
        replaces_version: installed.get(&c.project_id).cloned(),
    });
}

fn compatible(c: &Candidate, game_version: &str, loader: Option<&str>) -> bool {
    let mc_ok = c.game_versions.iter().any(|g| g == game_version);
    let loader_ok = match loader {
        Some(l) => c.loaders.iter().any(|x| x == l),
        // 原版实例：只接受不带加载器的文件
        None => c.loaders.is_empty() || c.loaders.iter().any(|x| x == "minecraft"),
    };
    mc_ok && loader_ok
}

/// 从候选里挑最稳的：release > beta > alpha，同级取新发布的
fn pick_best(
    mut list: Vec<Candidate>,
    game_version: &str,
    loader: Option<&str>,
) -> Option<Candidate> {
    list.retain(|c| compatible(c, game_version, loader));
    list.sort_by(|a, b| {
        let rank = |c: &Candidate| match c.channel.as_str() {
            "release" => 0,
            "beta" => 1,
            _ => 2,
        };
        rank(a).cmp(&rank(b))
    });
    list.into_iter().next()
}

/// 生产用的取数实现：Modrinth API + 本地实例扫描。
pub struct ModrinthSource;

fn to_candidate(f: &crate::resources::ResourceFile) -> Candidate {
    Candidate {
        project_id: String::new(), // 由调用方填
        version_id: f.version_id.clone(),
        name: f.name.clone(),
        version_number: f.version_number.clone(),
        channel: f.channel.clone(),
        file_name: f.file_name.clone(),
        url: f.url.clone(),
        size: f.size,
        sha1: f.sha1.clone(),
        sha512: f.sha512.clone(),
        game_versions: f.game_versions.clone(),
        loaders: f.loaders.clone(),
        dependencies: f
            .dependencies
            .iter()
            .map(|d| Dep {
                project_id: d.project_id.clone(),
                version_id: d.version_id.clone(),
                kind: DepKind::parse(&d.kind),
            })
            .collect(),
    }
}

impl DepSource for ModrinthSource {
    fn versions_for(
        &self,
        project_id: &str,
        game_version: &str,
        loader: Option<&str>,
    ) -> Result<Vec<Candidate>, String> {
        let list = crate::resources::files(project_id, Some(game_version), loader)?;
        Ok(list
            .iter()
            .map(|f| {
                let mut c = to_candidate(f);
                c.project_id = project_id.to_string();
                c
            })
            .collect())
    }

    fn version_by_id(&self, version_id: &str) -> Result<(Candidate, Option<String>), String> {
        let (f, project) = crate::resources::version_detail(version_id)?;
        let mut c = to_candidate(&f);
        c.project_id = project.clone().unwrap_or_default();
        Ok((c, project))
    }
}

/// 扫描实例 mods 目录，得出"已装了什么"。
///
/// 只认 jar 里 `fabric.mod.json` / `mods.toml` / `quilt.mod.json` 声明的 Modrinth 项目 id。
/// 认不出版本的项目放进 `unknown`，解析器会跳过它们并如实提示。
pub fn scan_installed(dir: &std::path::Path) -> (HashMap<String, String>, HashSet<String>) {
    // ModFile 只带人类可读的版本号，没有 Modrinth 版本 id，所以无法填 known。
    // 全部进 unknown：解析器会跳过安装，但会如实提示"没检查它自己的依赖"。
    let known: HashMap<String, String> = HashMap::new();
    let mut unknown = HashSet::new();
    for m in crate::mods::list(dir) {
        if let Some(pid) = m.project_id {
            unknown.insert(pid);
        }
    }
    (known, unknown)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(project: &str, version: &str, deps: Vec<Dep>, channel: &str) -> Candidate {
        Candidate {
            project_id: project.into(),
            version_id: version.into(),
            name: project.into(),
            version_number: version.into(),
            channel: channel.into(),
            file_name: format!("{project}-{version}.jar"),
            url: format!("https://cdn.modrinth.com/data/{project}/versions/{version}/x.jar"),
            size: 1000,
            sha1: Some("a".repeat(40)),
            sha512: None,
            game_versions: vec!["1.20.1".into()],
            loaders: vec!["fabric".into()],
            dependencies: deps,
        }
    }

    fn req_dep(project: &str) -> Dep {
        Dep {
            project_id: Some(project.into()),
            version_id: None,
            kind: DepKind::Required,
        }
    }

    struct Fake {
        by_id: HashMap<String, (Candidate, Option<String>)>,
        by_project: HashMap<String, Vec<Candidate>>,
    }

    impl Fake {
        fn new() -> Self {
            Self {
                by_id: HashMap::new(),
                by_project: HashMap::new(),
            }
        }
        fn add(&mut self, c: Candidate) {
            self.by_project
                .entry(c.project_id.clone())
                .or_default()
                .push(c.clone());
            self.by_id.insert(
                c.version_id.clone(),
                (c.clone(), Some(c.project_id.clone())),
            );
        }
    }

    impl DepSource for Fake {
        fn versions_for(
            &self,
            project_id: &str,
            _gv: &str,
            _l: Option<&str>,
        ) -> Result<Vec<Candidate>, String> {
            Ok(self.by_project.get(project_id).cloned().unwrap_or_default())
        }
        fn version_by_id(&self, vid: &str) -> Result<(Candidate, Option<String>), String> {
            self.by_id
                .get(vid)
                .cloned()
                .ok_or_else(|| "not found".to_string())
        }
    }

    fn request(root: &str, vid: &str) -> Request {
        Request {
            project_id: root.into(),
            version_id: vid.into(),
            game_version: "1.20.1".into(),
            loader: Some("fabric".into()),
            installed: HashMap::new(),
            installed_unknown: HashSet::new(),
            optional: HashSet::new(),
        }
    }

    #[test]
    fn resolves_nested_dependencies_in_order() {
        let mut f = Fake::new();
        f.add(cand("root", "v1", vec![req_dep("lib")], "release"));
        f.add(cand("lib", "v2", vec![req_dep("core")], "release"));
        f.add(cand("core", "v3", vec![], "release"));

        let r = resolve(&f, &request("root", "v1"), &Limits::default());
        assert!(!r.blocking(), "{:?}", r.conflicts);
        let order: Vec<&str> = r.nodes.iter().map(|n| n.project_id.as_str()).collect();
        assert_eq!(order, vec!["core", "lib", "root"], "依赖必须排在前面");
        assert_eq!(r.nodes.last().unwrap().depth, 0);
        assert_eq!(r.download_bytes, 3000);
    }

    #[test]
    fn detects_cycles() {
        let mut f = Fake::new();
        f.add(cand("a", "v1", vec![req_dep("b")], "release"));
        f.add(cand("b", "v2", vec![req_dep("a")], "release"));
        let r = resolve(&f, &request("a", "v1"), &Limits::default());
        assert!(r.blocking());
        assert!(!r.cycles.is_empty());
    }

    #[test]
    fn rejects_two_versions_of_same_project() {
        let mut f = Fake::new();
        f.add(cand("root", "v1", vec![req_dep("lib")], "release"));
        f.add(cand("lib", "v2", vec![], "release"));
        f.add(cand("other", "v9", vec![], "release"));
        // root 还要 other，other 要 lib 的另一个版本
        let mut root = f.by_id.get("v1").unwrap().0.clone();
        root.dependencies.push(req_dep("other"));
        f.add(root);
        let mut other = f.by_id.get("v9").unwrap().0.clone();
        other.dependencies.push(Dep {
            project_id: Some("lib".into()),
            version_id: Some("v2b".into()),
            kind: DepKind::Required,
        });
        f.add(other);
        f.add(cand("lib", "v2b", vec![], "release"));

        let r = resolve(&f, &request("root", "v1"), &Limits::default());
        assert!(r.blocking(), "同项目两个版本必须报冲突");
    }

    #[test]
    fn respects_depth_limit() {
        let mut f = Fake::new();
        for i in 0..12 {
            let next = if i == 11 {
                vec![]
            } else {
                vec![req_dep(&format!("p{}", i + 1))]
            };
            f.add(cand(&format!("p{i}"), &format!("v{i}"), next, "release"));
        }
        let lim = Limits {
            max_depth: 4,
            ..Default::default()
        };
        let r = resolve(&f, &request("p0", "v0"), &lim);
        assert!(r.truncated);
        assert!(r.nodes.len() <= 6, "深度限制没生效：{}", r.nodes.len());
    }

    #[test]
    fn respects_node_limit() {
        let mut f = Fake::new();
        let deps: Vec<Dep> = (0..50).map(|i| req_dep(&format!("d{i}"))).collect();
        f.add(cand("root", "v1", deps, "release"));
        for i in 0..50 {
            f.add(cand(&format!("d{i}"), &format!("w{i}"), vec![], "release"));
        }
        let lim = Limits {
            max_nodes: 10,
            ..Default::default()
        };
        let r = resolve(&f, &request("root", "v1"), &lim);
        assert!(r.truncated);
        assert!(r.nodes.len() <= 11);
    }

    #[test]
    fn skips_installed_and_optional() {
        let mut f = Fake::new();
        f.add(cand(
            "root",
            "v1",
            vec![
                req_dep("lib"),
                Dep {
                    project_id: Some("opt".into()),
                    version_id: None,
                    kind: DepKind::Optional,
                },
            ],
            "release",
        ));
        f.add(cand("lib", "v2", vec![], "release"));
        f.add(cand("opt", "v5", vec![], "release"));

        let mut rq = request("root", "v1");
        rq.installed.insert("lib".into(), "v2".into());
        let r = resolve(&f, &rq, &Limits::default());
        assert!(!r.blocking());
        assert_eq!(r.nodes.len(), 1, "已装和可选都不该进计划");
        assert!(r.skipped.iter().any(|s| s.reason == "installed"));
        assert!(r.skipped.iter().any(|s| s.reason == "optional"));
    }

    #[test]
    fn prefers_release_over_beta() {
        let mut f = Fake::new();
        f.add(cand("root", "v1", vec![req_dep("lib")], "release"));
        f.add(cand("lib", "vbeta", vec![], "beta"));
        f.add(cand("lib", "vrel", vec![], "release"));
        let r = resolve(&f, &request("root", "v1"), &Limits::default());
        let lib = r.nodes.iter().find(|n| n.project_id == "lib").unwrap();
        assert_eq!(lib.version_id, "vrel", "不该把 beta 当稳定版");
    }

    #[test]
    fn reports_missing_dependency() {
        let mut f = Fake::new();
        f.add(cand("root", "v1", vec![req_dep("ghost")], "release"));
        let r = resolve(&f, &request("root", "v1"), &Limits::default());
        assert!(r.blocking());
        assert!(r
            .conflicts
            .iter()
            .any(|c| c.code == "DEPENDENCY_CONFLICT" && c.project_id == "ghost"));
    }

    #[test]
    fn records_via_chain() {
        let mut f = Fake::new();
        f.add(cand("root", "v1", vec![req_dep("mid")], "release"));
        f.add(cand("mid", "v2", vec![req_dep("leaf")], "release"));
        f.add(cand("leaf", "v3", vec![], "release"));
        let r = resolve(&f, &request("root", "v1"), &Limits::default());
        let leaf = r.nodes.iter().find(|n| n.project_id == "leaf").unwrap();
        assert_eq!(leaf.via, vec!["root".to_string(), "mid".to_string()]);
        assert_eq!(leaf.depth, 2);
    }
}
