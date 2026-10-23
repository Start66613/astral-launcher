import { useMemo, useState } from "react";
import type { LocalLauncher } from "../hooks/useLocalLauncher";
import type { LegacyLauncher } from "../hooks/useLegacyLauncher";
import type { Instance, Task } from "../lib/types";
import { isTerminal, parseApiError } from "../lib/bridge";
import Icon from "../components/Icon";
import Modal from "../components/Modal";
import InstanceHealth from "../components/InstanceHealth";

type PageProps = { launcher: LocalLauncher; onDownload: () => void; onHome: () => void };

export default function Versions(props: PageProps) {
  if (props.launcher.mode === "core") return <CoreVersions {...props} />;
  if (props.launcher.mode === "checking" || props.launcher.mode === "error") return <section className="page versions-page">
    <header className="page-head"><div><span className="eyebrow">A PLACE FOR EVERY WORLD</span><h1 tabIndex={-1}>版本管理</h1></div></header>
    <div className="empty-state"><Icon name="compass" /><h2>{props.launcher.mode === "checking" ? "正在检测核心能力…" : "暂时无法读取本地核心"}</h2>
      {props.launcher.error && <p role="alert">{props.launcher.error}</p>}<button className="btn btn-ghost" onClick={() => void props.launcher.refresh()}>重新读取</button></div>
  </section>;
  return <LegacyVersions launcher={props.launcher.legacy} onDownload={props.onDownload} onHome={props.onHome} />;
}

function TaskProgress({ task }: { task: Task }) {
  return <div className="instance-task" role="status"><strong>{task.title}</strong><span>{task.stage} · {task.label}</span>
    {!isTerminal(task.state) && <progress className="progress" aria-label="当前创建阶段进度" max={task.total > 0 ? task.total : undefined} value={task.total > 0 ? Math.min(task.done, task.total) : undefined} />}
    {task.state === "error" && <p className="error-message">{task.error?.message || "创建未完成，请检查目录后重试。"}</p>}
  </div>;
}

function CoreVersions({ launcher, onDownload, onHome }: PageProps) {
  const { core } = launcher;
  const [query, setQuery] = useState("");
  const [editTarget, setEditTarget] = useState<Instance | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<Instance | null>(null);
  const [editName, setEditName] = useState("");
  const [acknowledged, setAcknowledged] = useState(false);
  const [createOpen, setCreateOpen] = useState(false);
  const [name, setName] = useState("");
  const [gameVersion, setGameVersion] = useState("");
  const [draftTaskId, setDraftTaskId] = useState<string | null>(null);
  const [actionError, setActionError] = useState("");
  const [formError, setFormError] = useState("");
  const filtered = useMemo(() => core.instances.filter(instance => `${instance.name} ${instance.gameVersion}`.toLowerCase().includes(query.trim().toLowerCase())), [core.instances, query]);
  const draftTask = draftTaskId ? core.tasks.get(draftTaskId) : null;
  const createPending = core.creating || !!core.createTask && !isTerminal(core.createTask.state);
  const formPending = core.creating || !!draftTask && !isTerminal(draftTask.state);
  const created = draftTask?.state === "done" && draftTask.result?.kind === "create" ? draftTask.result.instance : null;
  const busy = !!core.mutating;
  async function choose(id: string, goHome = false) {
    setActionError("");
    try { await core.select(id); if (goHome) onHome(); }
    catch (cause) { setActionError(parseApiError(cause).message); }
  }
  async function saveEdit(event: React.FormEvent) {
    event.preventDefault(); if (!editTarget || editTarget.managed !== true || editName.trim() === editTarget.name) return;
    setFormError("");
    try { await core.edit(editTarget.id, { name: editName.trim() }); setEditTarget(null); }
    catch (cause) { setFormError(parseApiError(cause).message); }
  }
  async function confirmDelete() {
    if (!deleteTarget || deleteTarget.managed !== true || !acknowledged) return;
    setFormError("");
    try { await core.remove(deleteTarget.id); setDeleteTarget(null); }
    catch (cause) {
      const failure = parseApiError(cause);
      setFormError(failure.code === "INSTANCE_BUSY" || failure.code === "BUSY" ? `实例正在运行或有进行中的任务。请退出游戏或等待任务完成后重试。${failure.message}` : failure.message);
    }
  }
  async function submitCreate(event: React.FormEvent) {
    event.preventDefault(); setFormError("");
    if (!name.trim() || !/^[A-Za-z0-9][A-Za-z0-9._+-]{0,127}$/.test(gameVersion.trim())) { setFormError("请输入实例名称和有效的游戏版本 ID，例如 1.21.1。"); return; }
    try { const task = await core.create({ name: name.trim(), gameVersion: gameVersion.trim() }); setDraftTaskId(task.taskId); }
    catch (cause) { setFormError(parseApiError(cause).message); }
  }
  function openCreate() { setName(""); setGameVersion(""); setDraftTaskId(null); setFormError(""); setCreateOpen(true); }

  return <section className="page versions-page">
    <header className="page-head"><div><span className="eyebrow">A PLACE FOR EVERY WORLD</span><h1 id="page-title" tabIndex={-1}>版本管理</h1><p>每个世界，都有自己的名字和归处。</p></div>
      <div className="instance-header-actions"><button className="btn btn-ghost" onClick={onDownload}><Icon name="download" />下载游戏版本</button>
        {core.hasTasks && <button className="btn btn-primary" disabled={core.status !== "ready" || busy || createPending} onClick={openCreate}><Icon name="plus" />新建实例</button>}</div></header>
    <div className="section-banner"><Icon name="layers" /><div><strong>本地实例</strong><p>独立实例各自保存 mods、config 与 saves。兼容版本保留共享目录。</p></div><button className="icon-button" disabled={core.refreshing || busy} onClick={() => void core.refresh()} aria-label="刷新本地实例"><Icon name="refresh" /></button></div>
    {core.createTask && <TaskProgress task={core.createTask} />}
    <label className="search"><Icon name="search" /><input aria-label="搜索本地实例" placeholder="搜索实例名称或游戏版本" value={query} onChange={event => setQuery(event.target.value)} /></label>
    {(actionError || core.error) && <p className="error-message" role="alert">{actionError || core.error}</p>}
    {core.status === "loading" ? <div className="empty-state" role="status"><Icon name="compass" /><h2>正在读取本地实例…</h2></div>
      : filtered.length > 0 ? <div className="version-grid">{filtered.map(instance => <article key={instance.id} data-instance-id={instance.id} className={`version-card ${instance.id === core.activeId ? "selected" : ""}`}>
        <div className="version-art" aria-hidden="true"><Icon name={instance.managed === true ? "leaf" : "layers"} /></div><div className="version-card-main">
          <span className="eyebrow">{instance.managed === true ? "YOUR OWN WORLD" : "LEGACY WORLD"}</span><h2>{instance.name}</h2>
          <p>{instance.gameVersion} · {instance.loader ? `${instance.loader.kind} ${instance.loader.version}` : "原版"}</p>
          <p className="instance-ownership">{instance.managed === true ? "独立实例 · 数据隔离" : "兼容版本 · 共享游戏目录"}</p>
          <InstanceHealth instance={instance} /><p>{instance.modsCount} 个启用模组 · {instance.modsDisabled} 个停用</p>
          <div className="version-card-actions"><button className={`btn ${instance.id === core.activeId ? "btn-soft" : "btn-ghost"}`} disabled={busy || core.status !== "ready" || instance.id === core.activeId} onClick={() => void choose(instance.id)} aria-pressed={instance.id === core.activeId}>{instance.id === core.activeId ? <><Icon name="check" />当前选择</> : "选择实例"}</button><button className="icon-button" disabled={busy || core.status !== "ready"} aria-label={`在首页查看 ${instance.name}`} onClick={() => void choose(instance.id, true)}><Icon name="arrowUp" /></button></div>
          {instance.health?.ok !== true && <button className="text-button instance-repair" onClick={onDownload}>前往下载中心补全 <Icon name="arrow" /></button>}
          {instance.managed === true && <div className="instance-manage"><button disabled={busy || core.status !== "ready"} onClick={() => { setEditName(instance.name); setFormError(""); setEditTarget(instance); }}>编辑</button><button disabled={busy || core.status !== "ready"} onClick={() => { setFormError(""); setAcknowledged(false); setDeleteTarget(instance); }}>删除</button></div>}
        </div></article>)}</div>
      : core.status !== "error" && <div className="empty-state"><Icon name={query ? "search" : "leaf"} /><h2>{query ? "暂时没找到这个实例" : "让第一个世界，在这里生长。"}</h2><p>{query ? "试试其他关键词，或清空搜索。" : "暂未发现本地实例。创建一个独立实例，或检查游戏目录后刷新。"}</p><button className="btn btn-ghost" onClick={query ? () => setQuery("") : onDownload}>{query ? "清空搜索" : "前往下载中心"}<Icon name="arrow" /></button></div>}
    <Modal open={!!editTarget} title="编辑实例" onClose={() => { if (!busy) setEditTarget(null); }}>
      <form className="instance-form" onSubmit={event => void saveEdit(event)}><label>实例名称<input className="text-input" autoComplete="off" required maxLength={60} value={editName} onChange={event => setEditName(event.target.value)} disabled={busy} /></label><p className="modal-note">本轮只修改显示名称，不移动游戏目录。Java 路径与加载器配置待后续接入。</p>
        {formError && <p className="error-message" role="alert">{formError}</p>}<button className="btn btn-primary" disabled={busy || !editName.trim() || editName.trim() === editTarget?.name}>{busy ? "正在保存…" : "保存名称"}</button></form>
    </Modal>
    <Modal open={!!deleteTarget} title="将实例移入回收站？" onClose={() => { if (!busy) setDeleteTarget(null); }}>
      <p className="modal-lead">{deleteTarget?.name}</p><code className="instance-path">{deleteTarget?.dir}</code><p className="modal-note">该实例的 mods、config、saves 等文件会一起移入应用回收站。游戏共享资源保留；当前界面尚未提供恢复入口。</p>
      <label className="delete-ack"><input type="checkbox" checked={acknowledged} disabled={busy} onChange={event => setAcknowledged(event.target.checked)} />我了解实例数据会随目录移动</label>
      {formError && <p className="error-message" role="alert">{formError}</p>}<div className="instance-dialog-actions"><button className="btn btn-ghost" disabled={busy} onClick={() => setDeleteTarget(null)}>保留实例</button><button className="btn btn-danger" disabled={busy || !acknowledged} onClick={() => void confirmDelete()}>{busy ? "正在移入…" : "确认移入回收站"}</button></div>
    </Modal>
    <Modal open={createOpen} title="新建独立实例" onClose={() => setCreateOpen(false)}>
      {created ? <div className="instance-created"><Icon name="check" /><h3>实例已创建</h3><p>{created.name} · {created.gameVersion}</p><p className="modal-note">实例目录与元数据已创建。是否需要补全游戏文件，以列表中的基础文件检查结果为准。</p><button className="btn btn-primary modal-full-button" onClick={() => setCreateOpen(false)}>查看实例列表</button></div>
        : <form className="instance-form" onSubmit={event => void submitCreate(event)}><label>实例名称<input className="text-input" autoComplete="off" required maxLength={60} value={name} disabled={formPending} onChange={event => setName(event.target.value)} /></label><label>游戏版本 ID<input className="text-input" autoComplete="off" required placeholder="例如 1.21.1" maxLength={128} value={gameVersion} disabled={formPending} onChange={event => setGameVersion(event.target.value)} /></label><p className="modal-note">创建独立的 mods、config、saves 等目录。不会自动下载缺失版本，也不代表游戏已经安装完成。</p>
          {draftTask && <TaskProgress task={draftTask} />}{formError && <p className="error-message" role="alert">{formError}</p>}<button className="btn btn-primary" disabled={formPending || !name.trim() || !gameVersion.trim()}>{formPending ? "正在创建…" : "创建实例"}</button></form>}
    </Modal>
  </section>;
}

function LegacyVersions({ launcher, onDownload, onHome }: { launcher: LegacyLauncher; onDownload: () => void; onHome: () => void }) {
  const [query, setQuery] = useState("");
  const filtered = useMemo(() => launcher.versions.filter(id => id.toLowerCase().includes(query.trim().toLowerCase())), [launcher.versions, query]);
  return <section className="page versions-page">
    <header className="page-head"><div><span className="eyebrow">A PLACE FOR EVERY WORLD</span><h1 id="page-title" tabIndex={-1}>版本管理</h1><p>把每一段旅程，安放妥当。</p></div><button className="btn btn-primary" onClick={onDownload}><Icon name="plus" />添加游戏版本</button></header>
    <div className="section-banner"><Icon name="layers" /><div><strong>本地版本</strong><p>当前使用版本模式。独立实例的创建、复制与删除暂不可用。</p></div><button className="icon-button" disabled={launcher.status === "browser" || launcher.status === "loading" || launcher.launchBusy} onClick={() => void launcher.refresh()} aria-label="刷新本地版本"><Icon name="refresh" /></button></div>
    <label className="search"><Icon name="search" /><input aria-label="搜索本地版本" placeholder="搜索你的游戏版本" value={query} onChange={event => setQuery(event.target.value)} /></label>
    {launcher.error && <p className="error-message" role="alert">读取未完成：{launcher.error}</p>}
    {launcher.status === "browser" ? <div className="empty-state"><Icon name="folder" /><h2>你的世界，在桌面端等你。</h2><p>连接桌面端后，这里会显示游戏目录中的真实版本。<br />浏览器预览不会扫描、创建或删除你的文件。</p><button className="btn btn-ghost" onClick={onDownload}>看看下载中心 <Icon name="arrow" /></button></div>
      : launcher.status === "loading" ? <div className="empty-state" role="status"><Icon name="compass" /><h2>正在读取本地版本…</h2><p>请稍候。</p></div>
      : launcher.status === "ready" && filtered.length > 0 ? <div className="version-grid">{filtered.map(id => <article key={id} className={`version-card ${id === launcher.current ? "selected" : ""}`}><div className="version-art" aria-hidden="true"><Icon name="layers" /></div><div className="version-card-main"><span className="eyebrow">LOCAL VERSION</span><h2>{id}</h2><p>来自当前游戏目录</p><div className="version-card-actions"><button className={`btn ${id === launcher.current ? "btn-soft" : "btn-ghost"}`} disabled={launcher.launchBusy} onClick={() => launcher.select(id)} aria-pressed={id === launcher.current}>{id === launcher.current ? <><Icon name="check" />当前选择</> : "选择版本"}</button><button className="icon-button" aria-label={`在首页查看 ${id}`} disabled={launcher.launchBusy} onClick={() => { launcher.select(id); onHome(); }}><Icon name="arrowUp" /></button></div></div></article>)}</div>
      : launcher.status !== "error" && <div className="empty-state"><Icon name={query ? "search" : "leaf"} /><h2>{query ? "暂时没找到这个版本" : "让第一个世界，在这里生长。"}</h2><p>{query ? "试试其他关键词，或清空搜索。" : "暂未发现可用的本地版本。你可以检查游戏目录，或先下载一个版本。"}</p><button className="btn btn-primary" onClick={query ? () => setQuery("") : onDownload}>{query ? "清空搜索" : "前往下载中心"}<Icon name="arrow" /></button></div>}
  </section>;
}
