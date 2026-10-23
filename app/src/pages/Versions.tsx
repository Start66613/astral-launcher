import { useMemo, useState } from "react";
import type { LocalLauncher } from "../hooks/useLocalLauncher";
import Icon from "../components/Icon";

export default function Versions({ launcher, onDownload, onHome }: { launcher: LocalLauncher; onDownload: () => void; onHome: () => void }) {
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
