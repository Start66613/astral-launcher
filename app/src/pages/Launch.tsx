import { useState } from "react";
import type { LocalLauncher } from "../hooks/useLocalLauncher";
import { parseApiError } from "../lib/bridge";
import Icon from "../components/Icon";
import Modal from "../components/Modal";
import InstanceHealth from "../components/InstanceHealth";

export default function Launch({ launcher, onAi, onDownload, onVersions, onAccount }: {
  launcher: LocalLauncher; onAi: () => void; onDownload: () => void; onVersions: () => void; onAccount: () => void;
}) {
  const [pickerOpen, setPickerOpen] = useState(false);
  const [selectionError, setSelectionError] = useState("");
  const { core, legacy, mode } = launcher;
  const isCore = mode === "core";
  const loading = mode === "checking" || (isCore ? core.status === "loading" : mode === "legacy" && legacy.status === "loading");
  const readFailed = mode === "error" || (isCore ? core.status === "error" : mode === "legacy" && legacy.status === "error");
  const worlds = isCore ? core.instances.map(instance => ({ id: instance.id, name: instance.name,
    description: `${instance.gameVersion} · ${instance.managed ? "独立实例" : "兼容版本 · 共享游戏目录"}` }))
    : mode === "legacy" ? legacy.versions.map(id => ({ id, name: id, description: "本地版本 · 兼容模式" })) : [];
  const currentId = isCore ? core.activeId : legacy.current;
  const current = worlds.find(world => world.id === currentId);
  const available = isCore ? core.status === "ready" && worlds.length > 0 : mode === "legacy" && legacy.status === "ready" && worlds.length > 0;
  const selectedText = current?.name || (loading ? "正在寻找你的世界…" : readFailed ? "本地数据未能读取" : worlds.length ? "选择一个实例" : "你的第一个世界");
  const needsFiles = isCore && !!core.current && core.current.health?.ok !== true;
  const busy = isCore ? core.launchBusy : legacy.launchBusy;
  const error = selectionError || launcher.error || (isCore ? core.error || core.launchError : mode === "legacy" ? legacy.error || legacy.launchError : "");
  const canChoose = available && !core.mutating && !core.refreshing && !legacy.launchBusy;
  let launchLabel = "进入世界";
  if (isCore && !needsFiles) {
    if (core.phase === "running") launchLabel = "游戏运行中";
    else if (core.launchBusy) launchLabel = core.phaseLabel;
    else if (core.phase === "exited") launchLabel = "再次进入世界";
  } else if (needsFiles) launchLabel = "补全游戏文件";
  else if (busy) launchLabel = "正在准备启动…";
  async function choose(id: string) {
    setSelectionError("");
    try {
      if (isCore) await core.select(id); else legacy.select(id);
      setPickerOpen(false);
    } catch (cause) { setSelectionError(parseApiError(cause).message); }
  }

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
        <div className="dock-caption"><span className="eyebrow">YOUR NEXT ADVENTURE</span><span>{mode === "browser" ? "桌面功能未连接" : loading ? "恢复查询中" : isCore ? "本地实例" : mode === "legacy" ? "本地版本 · 兼容模式" : "核心连接未完成"}</span></div>
        <div className="dock-main">
          <span className="world-emblem"><Icon name={current ? "leaf" : "compass"} /></span>
          <div className="dock-selection"><h2>{selectedText}</h2><p>{current?.description || (mode === "browser" ? "打开桌面端，连接你的游戏目录" : worlds.length ? "你的选择会保存在本机" : "让下一段旅程，从这里开始")}</p></div>
          {worlds.length > 0 && <button className="switch-version" disabled={!canChoose} onClick={() => setPickerOpen(true)}>{isCore ? "切换实例" : "切换版本"}<Icon name="down" /></button>}
          <button className="btn btn-primary launch-button" disabled={isCore ? core.status !== "ready" || !!core.mutating || (!needsFiles && !core.canLaunch) : !available || busy}
            onClick={() => { if (needsFiles) onDownload(); else if (isCore) void core.launch(); else void legacy.launch(); }}><Icon name={needsFiles ? "download" : "play"} /><span>{launchLabel}</span><Icon name="arrow" /></button>
        </div>
        {isCore && core.current && <InstanceHealth instance={core.current} />}
        {isCore && <div className="launch-feedback session-feedback" role="status" aria-live="polite" data-phase={core.phase}>
          <span className={`session-state state-${core.phase}`}><i />{core.phaseLabel}</span>
          <span>{core.phase === "starting" && core.launchTask ? `${core.launchTask.stage} · ${core.launchTask.label}`
            : core.phase === "confirming" ? "进程已创建，正在查询游戏会话…"
            : core.phase === "running" ? "已由本地核心确认游戏会话"
            : core.phase === "exited" ? core.session?.exitCode === null ? "已结束，退出码未知" : `游戏已结束 · 退出码 ${core.session?.exitCode}`
            : core.phase === "unknown" ? "请刷新确认状态，避免重复启动"
            : core.phase === "recovering" ? "正在读取任务与会话快照…" : ""}</span>
          <button className="text-button" disabled={core.refreshing || !!core.mutating} onClick={() => void core.refresh()} aria-label="刷新实例与会话"><Icon name="refresh" />刷新</button>
        </div>}
        {isCore && (!core.hasTasks || !core.hasSessions) && <p className="core-note">当前核心尚未提供完整的启动任务与会话能力，启动入口暂不可用。</p>}
        {!isCore && mode === "legacy" && busy && <div className="launch-feedback" role="status"><span>{legacy.progress?.label || "正在准备游戏进程…"}</span><span>请稍候</span></div>}
        {!isCore && mode === "legacy" && legacy.pid !== null && <p className="launch-feedback" role="status">进程已创建 · PID {legacy.pid}。兼容核心未提供持续运行状态。</p>}
        {error && <div className="error-message" role="alert">{error}<button className="text-button" onClick={() => void launcher.refresh()}>重新读取 <Icon name="refresh" /></button><button className="text-button" onClick={onAi}>查看诊断入口 <Icon name="arrow" /></button></div>}
      </section>
      <section className="home-worlds">
        <div className="section-heading"><h2>你的世界 <small>{available ? `${worlds.length} 个${isCore ? "本地实例" : "本地版本"}` : "每一段旅程，都有一个起点"}</small></h2><button className="text-button" onClick={onVersions}>查看全部 <Icon name="arrow" /></button></div>
        <div className="home-world-grid"><div className="world-collection">
          {worlds.length ? worlds.slice(0, 3).map(world => <button key={world.id} className={`world-tile ${world.id === currentId ? "selected" : ""}`} disabled={!canChoose} onClick={() => void choose(world.id)} aria-pressed={world.id === currentId}><Icon name="layers" /><span><b>{world.name}</b><small>{world.description}</small></span><Icon name={world.id === currentId ? "check" : "arrowUp"} /></button>)
            : <button className="first-world" onClick={isCore ? onVersions : onDownload}><span className="first-world-icon"><Icon name="plus" /></span><span><b>{loading ? "正在读取你的世界…" : readFailed ? "暂时无法读取你的世界" : isCore ? "暂未发现本地实例" : "让世界，从这里生长。"}</b><small>{readFailed ? "请检查上方错误并重新读取，当前不能确认本地实例数量。" : mode === "browser" ? "浏览器可预览界面，安装与启动需在桌面端完成。" : isCore ? "前往版本管理，创建第一个独立实例，或检查游戏目录。" : "前往下载中心，选择你想探索的游戏版本。"}</small></span><Icon name="arrow" /></button>}
        </div><button className="home-ai-card" onClick={onAi}><Icon name="spark" /><b>遇到问题时，<br />我在这里。</b><span>AI 诊断 <Icon name="arrow" /></span></button></div>
      </section>
    </div>
    <Modal open={pickerOpen} title="选择下一站" onClose={() => setPickerOpen(false)}><p className="modal-lead">{isCore ? "选择本机已有实例。兼容版本继续使用共享游戏目录。" : "选择来自当前游戏目录的本地版本。"}</p><div className="version-picker">{worlds.map(world => <button key={world.id} disabled={!canChoose} aria-pressed={world.id === currentId} onClick={() => void choose(world.id)}><Icon name="layers" /><span>{world.name}<small>{world.description}</small></span>{world.id === currentId && <Icon name="check" />}</button>)}</div>{selectionError && <p className="error-message" role="alert">{selectionError}</p>}</Modal>
  </section>;
}
