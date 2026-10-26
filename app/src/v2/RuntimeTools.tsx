import { useEffect, useRef, useState } from "react";
import type { Instance, Task } from "../lib/types";
import { parseApiError } from "../lib/bridge";
import { TaskView } from "./LiveRuntime";
import { Modal, Empty } from "./App";
import { ModalPresence, MotionDetails } from "./motion";
import { usePreview } from "./state";
import { waitForTask } from "./live-api";
import * as api from "./core-api";

function useOperation() {
  const [busy, setBusy] = useState(false),
    [error, setError] = useState(""),
    [task, setTask] = useState<Task | null>(null),
    [message, setMessage] = useState("");
  const lock = useRef(false);
  async function run(action: () => Promise<void>) {
    if (lock.current) return;
    lock.current = true;
    setBusy(true);
    setError("");
    setMessage("");
    try {
      await action();
    } catch (c) {
      const e = parseApiError(c);
      setError(`${e.code} · ${e.message}${e.detail ? `\n${e.detail}` : ""}`);
    } finally {
      lock.current = false;
      setBusy(false);
    }
  }
  async function follow(t: Task) {
    setTask(t);
    return waitForTask(t, setTask);
  }
  return { busy, error, task, message, setMessage, run, follow };
}
function Feedback({ op }: { op: ReturnType<typeof useOperation> }) {
  return (
    <>
      {op.task && <TaskView task={op.task} />}
      {op.error && (
        <pre className="banner error" role="alert">
          {op.error}
        </pre>
      )}
      {op.message && (
        <p className="banner" role="status">
          {op.message}
        </p>
      )}
    </>
  );
}
export function JavaManager() {
  const { runtime } = usePreview();
  const op = useOperation();
  const [report, setReport] = useState<api.JavaReport | null>(null),
    [managed, setManaged] = useState<api.ManagedJdk[]>([]),
    [major, setMajor] = useState(21),
    [confirming, setConfirming] = useState(false);
  const supported =
    runtime?.mode === "core" && runtime.api?.features.includes("java");
  const generation = useRef(0);
  async function refresh() {
    const ticket = ++generation.current;
    const [r, m] = await Promise.all([
      api.listJavaRuntimes(runtime?.core.activeId || undefined),
      api.listManagedJdks(),
    ]);
    if (ticket === generation.current) {
      setReport(r);
      setManaged(m);
    }
  }
  useEffect(() => {
    if (supported) void op.run(refresh);
    return () => {
      ++generation.current;
    };
  }, [supported, runtime?.core.activeId]);
  return (
    <section className="runtime-tools">
      <h2>Java 运行环境</h2>
      {!supported ? (
        <Empty
          title="未连接 Java 管理能力"
          detail="本地核心连接后可检测与下载。"
        />
      ) : (
        <>
          <p className="muted">
            {report?.requiredMajor
              ? `当前版本需要 Java ${report.requiredMajor}。`
              : "选择游戏版本后显示适配建议。"}
            下载由核心执行并校验 SHA256，不修改系统环境。
          </p>
          <button
            className="button secondary"
            disabled={op.busy}
            onClick={() => void op.run(refresh)}
          >
            重新检测 Java
          </button>
          <div className="java-list">
            {report?.runtimes.map((r) => (
              <div className="file-row" key={r.path}>
                <span>
                  <strong>
                    Java {r.version} · {r.vendor}
                    {report.recommended === r.path ? " · 推荐" : ""}
                  </strong>
                  <small>
                    {r.path} · {r.bits || "未知位数"} 位 · {r.source}
                  </small>
                </span>
              </div>
            ))}
            {report && !report.runtimes.length && (
              <p>未检测到可用 Java，可下载 Astral 管理的运行环境。</p>
            )}
          </div>
          <MotionDetails open>
            <summary>Astral 下载的 Java · {managed.length}</summary>
            {managed.map((j) => (
              <p className="break" key={j.dir}>
                Java {j.major} · {j.javaPath || "未找到 java.exe"}
              </p>
            ))}
          </MotionDetails>
          <div className="action-grid">
            <label className="field">
              Java 主版本
              <select
                aria-label="Java 主版本"
                value={major}
                disabled={op.busy}
                onChange={(e) => setMajor(Number(e.target.value))}
              >
                {[8, 17, 21, 25].map((v) => (
                  <option key={v} value={v}>
                    Java {v} · Windows x64
                  </option>
                ))}
              </select>
            </label>
            <button
              className="button primary"
              disabled={op.busy}
              onClick={() => setConfirming(true)}
            >
              下载 Java
            </button>
          </div>
          <ModalPresence present={confirming}>
            <Modal
              title={`下载 Java ${major}？`}
              close={() => {
                if (!op.busy) setConfirming(false);
              }}
            >
              <p>
                从 Adoptium 下载 Windows x64 JDK，通常约 100–200
                MB。实际大小与可用版本以服务返回为准。文件保存到当前游戏目录的
                jvm 文件夹。
              </p>
              <button
                className="button primary"
                disabled={op.busy}
                onClick={() =>
                  void op.run(async () => {
                    setConfirming(false);
                    await op.follow(await api.installJava(major));
                    await refresh();
                    op.setMessage("Java 下载任务已完成，列表已重新检测。");
                  })
                }
              >
                确认下载
              </button>
            </Modal>
          </ModalPresence>
        </>
      )}
      <Feedback op={op} />
    </section>
  );
}
export function VersionMaintenance({ instance }: { instance: Instance }) {
  const { runtime } = usePreview();
  const op = useOperation(),
    [confirming, setConfirming] = useState<"repair" | "export" | null>(null),
    [includeConfig, setIncludeConfig] = useState(false);
  return (
    <div className="runtime-tools">
      <div className="action-grid">
        <button
          className="button secondary"
          disabled={!instance.managed || op.busy}
          onClick={() => setConfirming("repair")}
        >
          补全缺失文件
        </button>
        <button
          className="button secondary"
          disabled={!instance.managed || op.busy}
          onClick={() => setConfirming("export")}
        >
          导出整合包
        </button>
      </div>
      <Feedback op={op} />
      <ModalPresence present={confirming !== null}>
        <Modal
          title={
            confirming === "repair"
              ? "检查并补全游戏文件"
              : "导出 Modrinth 整合包"
          }
          close={() => {
            if (!op.busy) setConfirming(null);
          }}
        >
          <p>
            {confirming === "repair"
              ? "只补全缺失文件，不删除已有存档。游戏运行或任务写入期间核心会拒绝操作。"
              : "导出到游戏根目录的 exports 文件夹。不覆盖已有文件；分享前检查许可及个人信息。"}
          </p>
          {confirming === "export" && (
            <label className="dependency-row">
              <input
                type="checkbox"
                checked={includeConfig}
                onChange={(e) => setIncludeConfig(e.target.checked)}
              />
              包含 config（可能含服务器地址或个人信息，请先检查）
            </label>
          )}
          <button
            className="button primary"
            disabled={op.busy}
            onClick={() =>
              void op.run(async () => {
                const mode = confirming;
                setConfirming(null);
                if (mode === "repair") {
                  await op.follow(await api.repairInstance(instance.id));
                  await runtime?.core.refresh();
                  op.setMessage("补全任务完成，已重新读取健康状态。");
                } else {
                  const result = await api.exportModpack({
                    instanceId: instance.id,
                    name: instance.name,
                    includeConfig,
                  });
                  op.setMessage(
                    `已导出 ${result.path}；索引 ${result.indexedFiles} 个文件，覆盖内容 ${result.overrideFiles} 个。`,
                  );
                }
              })
            }
          >
            确认{confirming === "repair" ? "检查" : "导出"}
          </button>
        </Modal>
      </ModalPresence>
    </div>
  );
}
export function ModUpdates({
  instance,
  onUpdated,
}: {
  instance: Instance;
  onUpdated: () => Promise<void>;
}) {
  const { runtime } = usePreview();
  const op = useOperation(),
    [report, setReport] = useState<api.UpdateReport | null>(null),
    [selection, setSelection] = useState<api.ModUpdate[]>([]);
  const supported = runtime?.api?.features.includes("updates");
  async function update(rows: api.ModUpdate[]) {
    for (const row of rows) {
      await op.follow(
        await api.startModUpdate(
          instance.id,
          row.fileName,
          row.projectId,
          row.latestVersionId,
        ),
      );
      await onUpdated();
    }
    setReport(null);
    op.setMessage(`核心已完成 ${rows.length} 个更新。`);
  }
  return (
    <div className="runtime-tools">
      <button
        className="button secondary"
        disabled={op.busy || !supported}
        onClick={() =>
          void op.run(async () => {
            setReport(await api.checkModUpdates(instance.id, 20));
          })
        }
      >
        检查 Mod 更新
      </button>
      {report && (
        <>
          <p className="muted">
            已检查 {report.checked} · 无法识别 {report.unknown} · 尚未检查{" "}
            {report.skipped}。无法识别不代表已是最新版本。
          </p>
          {report.updates.map((u) => (
            <div className="file-row" key={u.fileName}>
              <span>
                <strong>{u.fileName}</strong>
                <small>
                  {u.currentVersion} → {u.latestVersion}
                </small>
              </span>
              <button
                className="button secondary"
                disabled={op.busy}
                onClick={() => setSelection([u])}
              >
                更新
              </button>
            </div>
          ))}
          {report.updates.length > 0 && (
            <button
              className="button primary"
              disabled={op.busy}
              onClick={() => setSelection(report.updates)}
            >
              更新列出的 {report.updates.length} 个 Mod
            </button>
          )}
        </>
      )}
      <Feedback op={op} />
      <ModalPresence present={selection.length > 0}>
        <Modal
          title="确认更新 Mod"
          close={() => {
            if (!op.busy) setSelection([]);
          }}
        >
          <p>
            核心先校验新文件，再替换旧文件。请先备份重要配置；运行中的游戏会拒绝更新。
          </p>
          {selection.map((s) => (
            <p key={s.fileName}>
              {s.fileName} → {s.latestVersion}
            </p>
          ))}
          <button
            className="button primary"
            disabled={op.busy}
            onClick={() =>
              void op.run(async () => {
                const rows = selection;
                setSelection([]);
                await update(rows);
              })
            }
          >
            确认更新
          </button>
        </Modal>
      </ModalPresence>
    </div>
  );
}
export function GameLogs({ instance }: { instance: Instance }) {
  const op = useOperation(),
    [logs, setLogs] = useState<api.LogFile[]>([]),
    [logId, setLogId] = useState(""),
    [text, setText] = useState(""),
    [diagnosis, setDiagnosis] = useState<api.Diagnosis | null>(null);
  useEffect(() => {
    let live = true;
    void op.run(async () => {
      const rows = await api.listLogs(instance.id);
      if (live) setLogs(rows);
    });
    return () => {
      live = false;
    };
  }, [instance.id]);
  return (
    <div className="runtime-tools">
      <label className="field">
        本地游戏日志
        <select
          aria-label="本地游戏日志"
          disabled={op.busy}
          value={logId}
          onChange={(e) => {
            setLogId(e.target.value);
            setText("");
            setDiagnosis(null);
          }}
        >
          <option value="">请选择日志</option>
          {logs.map((l) => (
            <option key={l.id} value={l.id}>
              {l.name} · {l.kind} · {l.modifiedAt}
            </option>
          ))}
        </select>
      </label>
      {!logs.length && <p className="muted">此版本没有可读取的游戏日志。</p>}
      <div className="action-grid">
        <button
          className="button secondary"
          disabled={!logId || op.busy}
          onClick={() =>
            void op.run(async () => {
              const r = await api.readLog(instance.id, logId);
              setText(`${r.truncated ? "（仅显示日志尾部）\n" : ""}${r.text}`);
            })
          }
        >
          读取尾部
        </button>
        <button
          className="button secondary"
          disabled={!logId || op.busy}
          onClick={() =>
            void op.run(async () => {
              const r = await api.prepareDiagnosis(instance.id, [logId]);
              setText(r.text);
              op.setMessage(
                `已生成本地脱敏预览，${r.redactions} 处替换。${r.warnings.join("；")}`,
              );
            })
          }
        >
          脱敏预览
        </button>
        <button
          className="button secondary"
          disabled={!logId || op.busy}
          onClick={() =>
            void op.run(async () =>
              setDiagnosis(await api.diagnoseInstance(instance.id, logId)),
            )
          }
        >
          本地规则诊断
        </button>
        <button
          className="button secondary"
          disabled={!text}
          onClick={() =>
            void op.run(async () => {
              await navigator.clipboard.writeText(text);
              op.setMessage("已复制当前显示内容；分享前请检查个人信息。");
            })
          }
        >
          复制当前内容
        </button>
      </div>
      {text && (
        <pre className="log-content" tabIndex={0}>
          {text}
        </pre>
      )}
      {diagnosis && (
        <>
          <p>本地规则诊断，不是 AI 分析。</p>
          {diagnosis.findings.map((f) => (
            <MotionDetails open key={f.id}>
              <summary>
                {f.severity} · {f.label}
              </summary>
              <pre>{f.evidence.join("\n")}</pre>
              <p>{f.hint}</p>
            </MotionDetails>
          ))}
          {!diagnosis.findings.length && (
            <p>规则未发现已知问题；不代表游戏没有错误。</p>
          )}
        </>
      )}
      <Feedback op={op} />
    </div>
  );
}
