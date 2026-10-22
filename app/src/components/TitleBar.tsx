import type { ReactNode } from "react";
import type { Tab } from "../lib/types";
import { closeWindow, minimizeWindow, toggleMaximize } from "../lib/bridge";

const TABS: { id: Tab; label: string }[] = [
  { id: "launch", label: "启动" },
  { id: "download", label: "下载" },
  { id: "settings", label: "设置" },
  { id: "more", label: "更多" },
];

const ICONS: Record<Tab, ReactNode> = {
  launch: <path d="M8 5v14l11-7z" />,
  download: (
    <>
      <path d="M12 3v11" />
      <path d="m7.5 10 4.5 4.5L16.5 10" />
      <path d="M4 17.5V20h16v-2.5" />
    </>
  ),
  settings: (
    <>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-2.9 1.2 2 2 0 1 1-4 0 1.7 1.7 0 0 0-2.9-1.2l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1A1.7 1.7 0 0 0 4.6 15a2 2 0 1 1 0-4 1.7 1.7 0 0 0 1.2-2.9l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1A1.7 1.7 0 0 0 11.5 4a2 2 0 1 1 4 0 1.7 1.7 0 0 0 2.9 1.2l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0 1.2 2.9 2 2 0 1 1 0 4Z" />
    </>
  ),
  more: (
    <>
      <rect x="3.5" y="3.5" width="7" height="7" rx="1.5" />
      <rect x="13.5" y="3.5" width="7" height="7" rx="1.5" />
      <rect x="3.5" y="13.5" width="7" height="7" rx="1.5" />
      <rect x="13.5" y="13.5" width="7" height="7" rx="1.5" />
    </>
  ),
};

interface Props {
  tab: Tab;
  onTab: (t: Tab) => void;
  onOpenAi: () => void;
}

export default function TitleBar({ tab, onTab, onOpenAi }: Props) {
  return (
    <header className="titlebar" data-tauri-drag-region>
      <div className="brand" data-tauri-drag-region>
        <div className="mark">A</div>
        <b>Astral</b>
        <em>0.1.0</em>
      </div>

      <nav className="tb-nav">
        {TABS.map((t) => (
          <button
            key={t.id}
            className={tab === t.id ? "on" : ""}
            onClick={() => onTab(t.id)}
          >
            <svg viewBox="0 0 24 24">{ICONS[t.id]}</svg>
            {t.label}
          </button>
        ))}
      </nav>

      <div className="tb-right">
        <button className="tb-btn" onClick={onOpenAi}>
          <svg viewBox="0 0 24 24">
            <path d="M12 3v3M12 18v3M3 12h3M18 12h3M5.6 5.6l2.1 2.1M16.3 16.3l2.1 2.1M18.4 5.6l-2.1 2.1M7.7 16.3l-2.1 2.1" />
          </svg>
          AI 诊断
        </button>
        <div className="win">
          <span onClick={() => void minimizeWindow()} title="最小化">
            —
          </span>
          <span onClick={() => void toggleMaximize()} title="最大化">
            ▢
          </span>
          <span className="close" onClick={() => void closeWindow()} title="关闭">
            ✕
          </span>
        </div>
      </div>
    </header>
  );
}
