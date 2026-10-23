import { useState } from "react";
import type { LocalLauncher } from "../hooks/useLocalLauncher";
import Icon from "../components/Icon";
import Modal from "../components/Modal";

export default function Launch({ launcher, onAi, onDownload, onVersions, onAccount }: {
  launcher: LocalLauncher; onAi: () => void; onDownload: () => void; onVersions: () => void; onAccount: () => void;
}) {
  const [pickerOpen, setPickerOpen] = useState(false);
  const { current, versions, status, error, launchError, launchBusy, progress, pid } = launcher;
  const available = status === "ready" && versions.length > 0;
  const selectedText = current || (status === "loading" ? "正在寻找你的世界…" : "你的第一个世界");
  return <section className="forest-home">
    <div className="home-topline">
      <span className="scene-label"><Icon name="moon" /> FOREST TALE / AFTER HOURS</span>
      <button className="account-pill" onClick={onAccount}><Icon name="user" />{launcher.config?.username ? "离线档案" : "未登录"}<Icon name="down" /></button>
    </div>
    <div className="home-hero">
      <div><span className="hero-overline">慢一点，世界会等你。</span><h1 id="page-title" tabIndex={-1}>今夜，<br />住进一片森林。</h1><p>月光落在树梢，炉火亮在窗边。<br />去一个，只属于你的地方。</p><button className="hero-link" onClick={onVersions}>寻找下一段旅程 <Icon name="arrow" /></button></div>
      <div className="scene-note"><span>01 / THE QUIET WORLD</span><i /><p>在喧嚣之外<br />给想象留一盏灯</p></div>
    </div>
    <div className="home-bottom">
      <section className="launch-dock" aria-label="启动游戏">
        <div className="dock-caption"><span className="eyebrow">YOUR NEXT ADVENTURE</span><span>{status === "browser" ? "桌面功能未连接" : status === "loading" ? "正在读取本地版本" : available ? "本地版本" : "尚未选择版本"}</span></div>
        <div className="dock-main">
          <span className="world-emblem"><Icon name={available ? "leaf" : "compass"} /></span>
          <div className="dock-selection"><h2>{selectedText}</h2><p>{available ? "来自游戏目录 · 版本模式" : status === "browser" ? "打开桌面端，连接你的游戏目录" : "安装一个版本，让故事慢慢开始"}</p></div>
          {available && <button className="switch-version" disabled={launchBusy} onClick={() => setPickerOpen(true)}>切换版本 <Icon name="down" /></button>}
          <button className="btn btn-primary launch-button" disabled={!available || launchBusy} onClick={() => void launcher.launch()}><Icon name="play" /><span>{launchBusy ? "正在准备启动…" : "进入世界"}</span><Icon name="arrow" /></button>
        </div>
        {launchBusy && <div className="launch-feedback" role="status"><span>{progress?.label || "正在准备游戏进程…"}</span><span>请稍候</span></div>}
        {pid !== null && <p className="launch-feedback" role="status">进程已创建 · PID {pid}。游戏后续运行状态尚未接入。</p>}
        {(error || launchError) && <div className="error-message" role="alert">{launchError || error}<button className="text-button" onClick={onAi}>查看诊断入口 <Icon name="arrow" /></button></div>}
      </section>
      <section className="home-worlds">
        <div className="section-heading"><h2>你的世界 <small>{available ? `${versions.length} 个本地版本` : "每一段旅程，都有一个起点"}</small></h2><button className="text-button" onClick={onVersions}>查看全部 <Icon name="arrow" /></button></div>
        <div className="home-world-grid"><div className="world-collection">
          {available ? versions.slice(0, 3).map(id => <button key={id} className={`world-tile ${id === current ? "selected" : ""}`} disabled={launchBusy} onClick={() => launcher.select(id)} aria-pressed={id === current}><Icon name="layers" /><span><b>{id}</b><small>本地版本</small></span><Icon name={id === current ? "check" : "arrowUp"} /></button>) : <button className="first-world" onClick={onDownload}><span className="first-world-icon"><Icon name="plus" /></span><span><b>让世界，从这里生长。</b><small>{status === "browser" ? "浏览器可预览界面，安装与启动需在桌面端完成。" : "前往下载中心，选择你想探索的游戏版本。"}</small></span><Icon name="arrow" /></button>}
        </div><button className="home-ai-card" onClick={onAi}><Icon name="spark" /><b>遇到问题时，<br />我在这里。</b><span>AI 诊断 <Icon name="arrow" /></span></button></div>
      </section>
    </div>
    <Modal open={pickerOpen} title="选择下一站" onClose={() => setPickerOpen(false)}><p className="modal-lead">选择来自当前游戏目录的本地版本。</p><div className="version-picker">{versions.map(id => <button key={id} aria-pressed={id === current} onClick={() => { launcher.select(id); setPickerOpen(false); }}><Icon name="layers" /><span>{id}</span>{id === current && <Icon name="check" />}</button>)}</div></Modal>
  </section>;
}
