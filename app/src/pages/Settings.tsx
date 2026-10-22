import { useEffect, useState } from "react";
import type { Accent, LauncherConfig, Motion, Radius, Skin, SystemInfo, Theme } from "../lib/types";
import { getConfig, openPath, setConfig } from "../lib/bridge";
import { Card, Svg } from "../components/ui";

const THEMES: { v: Theme; label: string }[] = [
  { v: "light", label: "亮色" },
  { v: "dark", label: "暗色" },
];
const ACCENTS: { v: Accent; color: string }[] = [
  { v: "blue", color: "#1e6cd6" },
  { v: "green", color: "#177f45" },
  { v: "purple", color: "#6941da" },
  { v: "orange", color: "#b85c0a" },
];
const RADII: { v: Radius; label: string }[] = [
  { v: "sharp", label: "锐利" },
  { v: "default", label: "默认" },
  { v: "soft", label: "柔和" },
];
const MOTIONS: { v: Motion; label: string }[] = [
  { v: "on", label: "标准" },
  { v: "off", label: "关闭" },
];

function Select({
  value,
  options,
  onPick,
}: {
  value: string;
  options: { v: string; label: string }[];
  onPick: (v: string) => void;
}) {
  return (
    <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
      {options.map((o) => (
        <button
          key={o.v}
          className={"opt" + (value === o.v ? " on" : "")}
          onClick={() => onPick(o.v)}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

function StaticSelect({ text }: { text: string }) {
  return (
    <div className="field">
      <span>{text}</span>
      <Svg>
        <path d="m6 9 6 6 6-6" />
      </Svg>
    </div>
  );
}

export default function Settings({
  skin,
  setSkin,
  system,
}: {
  skin: Skin;
  setSkin: (p: Partial<Skin>) => void;
  system: SystemInfo | null;
}) {
  const [cfg, setCfg] = useState<LauncherConfig | null>(null);

  useEffect(() => {
    void getConfig().then(setCfg);
  }, []);

  function update(p: Partial<LauncherConfig>) {
    setCfg((c) => {
      if (!c) return c;
      const next = { ...c, ...p };
      void setConfig(next);
      return next;
    });
  }

  return (
    <section className="page">
      <div className="page-head">
        <h1>设置</h1>
        <p>改动即时生效</p>
      </div>

      <Card title="外观" meta="主题市场的每一套皮肤都由这几项生成">
        <div className="frow">
          <div className="lbl">主题</div>
          <div className="val">
            <Select
              value={skin.theme}
              options={THEMES}
              onPick={(v) => setSkin({ theme: v as Theme })}
            />
          </div>
        </div>
        <div className="frow">
          <div className="lbl">强调色</div>
          <div className="val">
            <div style={{ display: "flex", gap: 8 }}>
              {ACCENTS.map((a) => (
                <button
                  key={a.v}
                  className={"sw" + (skin.accent === a.v ? " on" : "")}
                  onClick={() => setSkin({ accent: a.v })}
                  title={a.v}
                >
                  <i style={{ background: a.color }} />
                </button>
              ))}
            </div>
          </div>
        </div>
        <div className="frow">
          <div className="lbl">圆角</div>
          <div className="val">
            <Select
              value={skin.radius}
              options={RADII}
              onPick={(v) => setSkin({ radius: v as Radius })}
            />
          </div>
        </div>
        <div className="frow">
          <div className="lbl">动效</div>
          <div className="val">
            <Select
              value={skin.motion}
              options={MOTIONS}
              onPick={(v) => setSkin({ motion: v as Motion })}
            />
          </div>
        </div>
      </Card>

      <Card title="下载与目录">
        <div className="frow">
          <div className="lbl">下载源</div>
          <div className="val">
            <Select
              value={cfg?.source ?? "official"}
              options={[
                { v: "official", label: "官方 CDN" },
                { v: "bmclapi", label: "BMCLAPI 镜像" },
              ]}
              onPick={(v) => update({ source: v })}
            />
            <div className="note" style={{ marginTop: 6 }}>
              镜像加速依赖库与资源文件；版本元数据、客户端 jar 仍走官方。
            </div>
          </div>
        </div>
        <div className="frow">
          <div className="lbl">游戏目录</div>
          <div className="val">
            <div style={{ display: "flex", gap: 8 }}>
              <div className="field" style={{ flex: 1 }}>
                <span
                  style={{
                    fontSize: 12.5,
                    color: "var(--t2)",
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}
                >
                  {cfg?.gameDir ?? "读取中…"}
                </span>
              </div>
              <button
                className="btn btn-ghost"
                onClick={() => cfg && void openPath(cfg.gameDir)}
              >
                打开
              </button>
            </div>
          </div>
        </div>
        <div className="frow">
          <div className="lbl">离线用户名</div>
          <div className="val">
            <div className="field">
              <input
                value={cfg?.username ?? ""}
                onChange={(e) => update({ username: e.target.value })}
                placeholder="Player"
              />
            </div>
          </div>
        </div>
      </Card>

      <Card title="启动选项">
        <div className="frow">
          <div className="lbl">默认版本隔离</div>
          <div className="val">
            <StaticSelect text="隔离所有版本" />
          </div>
        </div>
        <div className="frow">
          <div className="lbl">游戏窗口标题</div>
          <div className="val">
            <div className="field">
              <input placeholder="默认" />
            </div>
          </div>
        </div>
        <div className="frow">
          <div className="lbl">自定义信息</div>
          <div className="val">
            <div className="field">
              <input defaultValue="Astral" />
            </div>
          </div>
        </div>
        <div className="frow">
          <div className="lbl">进程优先级</div>
          <div className="val">
            <StaticSelect text="中（平衡）" />
          </div>
        </div>
        <div className="frow">
          <div className="lbl">窗口大小</div>
          <div className="val">
            <StaticSelect text="默认" />
          </div>
        </div>
      </Card>

      <Card title="内存分配">
        <div className="frow">
          <div className="lbl">分配方式</div>
          <div className="val">
            <div className="radios">
              <label>
                <span className="rd on" />
                自动配置
              </label>
              <label>
                <span className="rd" />
                自定义
              </label>
              <span style={{ color: "var(--t3)", fontSize: 12, marginLeft: 8 }}>
                {system ? `4 GB / ${system.totalMemGB} GB` : "读取中…"}
              </span>
            </div>
          </div>
        </div>
        <div className="frow">
          <div className="lbl">启动前优化</div>
          <div className="val">
            <div className="radios">
              <label>
                <span className="rd on" />
                启动游戏前进行内存优化
              </label>
            </div>
          </div>
        </div>
        <div className="frow">
          <div className="lbl">系统</div>
          <div className="val">
            <span style={{ fontSize: 12, color: "var(--t3)" }}>
              {system
                ? `${system.os} · ${system.cpuCores} 核 · 可用 ${system.freeMemGB} GB`
                : "读取中…"}
              {system?.demo ? "（未在 Tauri 内运行，为演示数据）" : ""}
            </span>
          </div>
        </div>
      </Card>
    </section>
  );
}
