import { useState } from "react";

const STEPS = [
  "读取 latest.log 与 crash-report",
  "命中规则：Mixin apply failed ×3",
  "抽取相关模组：optifine / embeddium",
];

export default function AiDrawer({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const [steps, setSteps] = useState<string[]>([]);
  const [stage, setStage] = useState<"idle" | "running" | "done">("idle");
  const [fixed, setFixed] = useState(false);

  function run() {
    if (stage !== "idle") return;
    setStage("running");
    STEPS.forEach((s, i) => {
      window.setTimeout(() => setSteps((prev) => [...prev, s]), 240 * (i + 1));
    });
    window.setTimeout(() => setStage("done"), 240 * STEPS.length + 320);
  }

  return (
    <aside className={"drawer" + (open ? " on" : "")} aria-hidden={!open}>
      <div className="dr-head">
        <h3>崩溃分析</h3>
        <span className="chip">deepseek-flash</span>
        <button className="dr-close" onClick={onClose} title="关闭">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
            <path d="M6 6l12 12M18 6 6 18" />
          </svg>
        </button>
      </div>

      <div className="dr-body">
        <div className="file">
          <svg viewBox="0 0 24 24">
            <path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8l-5-5Z" />
            <path d="M14 3v5h5" />
          </svg>
          <div>
            <b>crash-report-2026-10-21_19-32-11.txt</b>
            <br />
            1,284 行 · 1.8 MB
          </div>
        </div>

        {stage === "idle" && (
          <button
            className="btn btn-primary"
            style={{ justifyContent: "center", height: 34 }}
            onClick={run}
          >
            开始分析
          </button>
        )}

        {steps.length > 0 && (
          <div className="steps">
            {steps.map((s) => (
              <div key={s}>
                <svg viewBox="0 0 24 24">
                  <path d="m4 12 5 5L20 6" />
                </svg>
                <span>{s}</span>
              </div>
            ))}
          </div>
        )}

        {stage === "done" && (
          <div className="result">
            <div className="res-head">
              <div className="rh-top">
                <span className="tag">FATAL</span>
                <b>OptiFine 与 Embeddium 渲染管线冲突</b>
              </div>
              <div className="rh-sub">置信度 86% · 来自 2 条证据</div>
            </div>
            <div className="res-body">
              <div>两个模组改写了同一批渲染类，Mixin 注入顺序不确定。</div>
              <div className="mono">
                {`crash-report:1284
Mixin apply failed
  optifine.mixins.json → MinecraftMixin
  Caused by: NoSuchMethodError: getRenderType`}
              </div>
              <div className="fix">
                <p>
                  禁用 OptiFine，改用 Embeddium + Oculus
                  <small>风险低 · 已创建快照 snapshot-4f2a</small>
                </p>
                <button
                  className="btn btn-soft"
                  disabled={fixed}
                  onClick={() => setFixed(true)}
                >
                  {fixed ? "已修复" : "修复"}
                </button>
              </div>
            </div>
          </div>
        )}
      </div>

      <div className="dr-foot">
        <span>预计 8.2K tokens · ≈ 0.014 元</span>
        <div className="sp" />
        <span>用你的 Key 计费</span>
      </div>
    </aside>
  );
}
