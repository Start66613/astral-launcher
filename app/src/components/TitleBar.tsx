import { closeWindow, inTauri, minimizeWindow, toggleMaximize } from "../lib/bridge";
import type { Theme } from "../lib/types";
import Icon from "./Icon";

export default function TitleBar({ theme, onTheme, onError }: { theme: Theme; onTheme: () => void; onError: (message: string) => void }) {
  async function windowAction(action: () => Promise<void>) {
    try { await action(); } catch (error) { onError(String(error)); }
  }
  return <header className="titlebar" data-tauri-drag-region>
    <div className="brand" data-tauri-drag-region><Icon name="star" /><span>Astral Launcher</span><small>森林物语</small></div>
    <div className="titlebar-actions"><button className="icon-button theme-toggle" onClick={onTheme} aria-label={theme === "dark" ? "切换浅色主题" : "切换深色主题"}><Icon name={theme === "dark" ? "sun" : "moon"} /></button><span className="runtime-label">{inTauri ? "桌面端" : "浏览器预览"}</span><div className="window-buttons">
      <button disabled={!inTauri} onClick={() => void windowAction(minimizeWindow)} aria-label="最小化窗口"><Icon name="minimize" /></button>
      <button disabled={!inTauri} onClick={() => void windowAction(toggleMaximize)} aria-label="切换窗口最大化"><Icon name="maximize" /></button>
      <button disabled={!inTauri} className="window-close" onClick={() => void windowAction(closeWindow)} aria-label="关闭窗口"><Icon name="close" /></button>
    </div></div>
  </header>;
}
