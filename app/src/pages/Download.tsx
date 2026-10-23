import { useEffect, useMemo, useRef, useState } from "react";
import { inTauri, installVersion, listVersions, onProgress, scanVersions, type Progress } from "../lib/bridge";
import type { VersionSummary } from "../lib/types";
import type { Section } from "../App";
import Icon from "../components/Icon";
import Resources from "./Resources";

const LABELS: Record<string, string> = { release: "正式版", snapshot: "预览版", old_beta: "早期 Beta", old_alpha: "早期 Alpha" };
export default function Download({ section, onSection, onInstalled }: { section: Section; onSection: (section: Section) => void; onInstalled: () => void }) {
  const [visited, setVisited] = useState<Section[]>([]);
  useEffect(() => { if (section !== "vanilla") setVisited(old => old.includes(section) ? old : [...old, section]); }, [section]);
  const resources = (["mod", "modpack", "datapack", "resourcepack", "shaderpack"] as const).filter(kind => visited.includes(kind) || section === kind);
  return <><div hidden={section !== "vanilla"}><Vanilla onInstalled={onInstalled} /></div>{resources.map(kind => <div key={kind} hidden={section !== kind}><Resources kind={kind} onGoVanilla={() => onSection("vanilla")} /></div>)}</>;
}
function Vanilla({ onInstalled }: { onInstalled: () => void }) {
  const [versions, setVersions] = useState<VersionSummary[]>([]);
  const [installed, setInstalled] = useState<string[]>([]);
  const [loading, setLoading] = useState(inTauri);
  const [error, setError] = useState("");
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState("all");
  const [limit, setLimit] = useState(40);
  const [busy, setBusy] = useState<string | null>(null);
  const [progress, setProgress] = useState<Progress | null>(null);
  const busyRef = useRef<string | null>(null);
  const alive = useRef(true);
  useEffect(() => {
    let active = true;
    alive.current = true;
    if (inTauri) void Promise.all([listVersions(), scanVersions()]).then(([list, local]) => {
      if (active) { setVersions(list); setInstalled(local); }
    }).catch(cause => { if (active) setError(String(cause)); }).finally(() => { if (active) setLoading(false); });
    let unlisten: (() => void) | undefined;
    if (inTauri) void onProgress((id, update) => { if (active && id === busyRef.current) setProgress(update); }).then(un => { if (!active) un(); else unlisten = un; }).catch(cause => { if (active) setError(String(cause)); });
    return () => { active = false; alive.current = false; unlisten?.(); };
  }, []);
  const filtered = useMemo(() => versions.filter(version => (!query.trim() || version.id.toLowerCase().includes(query.trim().toLowerCase())) && (filter === "all" || (filter === "old" ? version.kind === "old_beta" || version.kind === "old_alpha" : version.kind === filter))), [versions, query, filter]);
  async function install(id: string) {
    if (!inTauri || busyRef.current) return;
    busyRef.current = id; setBusy(id); setProgress(null); setError("");
    try {
      await installVersion(id);
      if (alive.current) { setInstalled(old => old.includes(id) ? old : [...old, id]); onInstalled(); }
    } catch (cause) { if (alive.current) setError(String(cause)); }
    finally { busyRef.current = null; if (alive.current) { setBusy(null); setProgress(null); } }
  }
  return <section className="page download-page">
    <header className="page-head"><div><span className="eyebrow">THE NEXT ADVENTURE STARTS HERE</span><h1 id="page-title" tabIndex={-1}>下载中心</h1><p>选一个版本，去一个新的世界。</p></div>{inTauri && !loading && <span className="chip">{versions.length} 个可用版本</span>}</header>
    {!inTauri ? <div className="empty-state"><Icon name="download" /><h2>为下一次冒险，准备好行囊。</h2><p>游戏版本清单、安装与校验需要在桌面端完成。<br />此处不会展示虚构的已安装版本或下载进度。</p><span className="note">也可以切换上方类别，浏览公开的社区资源。</span></div> : <>
      <label className="search"><Icon name="search" /><input aria-label="搜索游戏版本" placeholder="输入版本号，如 1.21.1" value={query} onChange={event => { setQuery(event.target.value); setLimit(40); }} /></label>
      <div className="catalog-tabs" aria-label="版本类型">{[["all", "全部"], ["release", "正式版"], ["snapshot", "预览版"], ["old", "早期版本"]].map(([id, label]) => <button key={id} aria-pressed={filter === id} onClick={() => { setFilter(id); setLimit(40); }}>{label}</button>)}</div>
      {loading ? <div className="empty-state" role="status"><Icon name="compass" /><h2>正在读取版本清单…</h2></div> : <div className="card"><ul>{filtered.slice(0, limit).map(version => <li className="row" key={version.id}><span className="row-ico"><Icon name="layers" /></span><div style={{ flex: 1, minWidth: 0 }}><div className="row-name">{version.id}{installed.includes(version.id) && <span className="chip acc">已安装</span>}</div><p className="row-sub">{version.released.slice(0, 10)} · {LABELS[version.kind] || version.kind}</p>{busy === version.id && <><progress className="progress" aria-label="当前安装阶段进度" max={progress?.total && progress.total > 0 ? progress.total : undefined} value={progress?.total && progress.total > 0 ? Math.min(progress.done, progress.total) : undefined} /><p className="row-sub" role="status">{progress?.label || "正在准备安装…"} · 当前阶段</p></>}</div><button className="row-btn primary" disabled={busy !== null} onClick={() => void install(version.id)}>{busy === version.id ? "安装中…" : installed.includes(version.id) ? "校验补全" : "安装"}</button></li>)}</ul>{filtered.length === 0 && <p className="catalog-loading">{error ? "版本清单未能读取，请检查连接。" : "没有符合条件的版本，请尝试其他搜索或分类。"}</p>}</div>}
      {filtered.length > limit && <button className="btn btn-ghost catalog-more" onClick={() => setLimit(old => old + 40)}>显示更多版本</button>}
    </>}
    {error && <p className="error-message" role="alert">{error}</p>}
    <p className="note">游戏文件由桌面端从所选来源获取并校验。社区资源的下载不等于完成整合包安装。</p>
  </section>;
}
