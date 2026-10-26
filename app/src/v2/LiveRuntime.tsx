import { ModalPresence, MotionRegion, MotionDetails } from "./motion";
import { useEffect, useRef, useState } from "react";
import { Badge, Icon } from "./icons";
import { usePreview } from "./state";
import { Empty, Modal, Toggle, Effect } from "./App";
import {
  JavaManager,
  VersionMaintenance,
  ModUpdates,
  GameLogs,
} from "./RuntimeTools";
import { cancelledLate, retryTask, type TaskV2 } from "./core-api";
import {
  hasCooperativeCancellation,
  requestTaskCancellation,
  waitForTask,
} from "./live-api";
import {
  deleteMod,
  getStats,
  inTauri,
  listMods,
  listSaves,
  openPathChecked,
  parseApiError,
  patchConfig,
  setModEnabled,
} from "../lib/bridge";
import type {
  CoreSaveEntry,
  Instance,
  InstancePatch,
  ModFile,
  Stats,
  Task,
} from "../lib/types";

export function TaskView({ task }: { task: Task }) {
  const { runtime } = usePreview();
  const [requesting, setRequesting] = useState(false);
  const [accepted, setAccepted] = useState(false);
  const [cancelError, setCancelError] = useState("");
  const requestLock = useRef(false);
  useEffect(() => {
    setAccepted(false);
    setCancelError("");
  }, [task.taskId]);
  const supportsCancel = hasCooperativeCancellation(task);
  const active = task.state === "queued" || task.state === "running";
  const cancelling =
    active && (accepted || (supportsCancel && task.cancelling));
  async function cancel() {
    if (requestLock.current || cancelling || !active || !supportsCancel) return;
    requestLock.current = true;
    setRequesting(true);
    setCancelError("");
    try {
      const result = await requestTaskCancellation(task.taskId);
      setAccepted(result.accepted);
      if (!result.accepted)
        setCancelError(
          "核心未受理取消请求，任务可能已经结束，请查看最新状态。",
        );
      await runtime?.core.refresh();
    } catch (cause) {
      setCancelError(parseApiError(cause).message);
    } finally {
      requestLock.current = false;
      setRequesting(false);
    }
  }
  return (
    <article
      className={`task-item task-${task.state}`}
      data-task-id={task.taskId}
    >
      <Badge
        kind={
          task.state === "error"
            ? "warning"
            : task.kind === "loader"
              ? "fabric"
              : "download"
        }
      />
      <div>
        <h3>{task.title}</h3>
        <p>
          {cancelling
            ? "正在取消…等待当前阶段停止"
            : task.error?.code === "INTERRUPTED"
              ? "上次退出时未完成"
              : task.state === "done"
                ? cancelledLate(task)
                  ? "已完成 · 取消请求到达时任务已经完成"
                  : "完成"
                : task.state === "error"
                  ? "失败"
                  : task.state === "queued"
                    ? "排队中"
                    : task.state === "cancelled"
                      ? "已取消"
                      : `${(task as TaskV2).stageLabel || task.stage} · ${task.label}`}
        </p>
        {["running", "queued"].includes(task.state) && (
          <progress
            aria-label="当前任务阶段进度"
            max={task.total > 0 ? task.total : undefined}
            value={task.total > 0 ? Math.min(task.done, task.total) : undefined}
          />
        )}
        {task.error && (
          <MotionDetails open>
            <summary>
              {task.error.code} · {task.error.message}
            </summary>
            <pre>{task.error.detail || "核心未提供更多详情"}</pre>
          </MotionDetails>
        )}
        <small className="muted">
          任务 {task.taskId} · 修订 {task.revision}
        </small>
        {supportsCancel && active && (
          <div className="task-cancel">
            <button
              className="button secondary"
              disabled={requesting || cancelling}
              onClick={() => void cancel()}
            >
              {cancelling
                ? "正在取消…"
                : requesting
                  ? "正在请求取消…"
                  : "取消任务"}
            </button>
            <small className="muted">
              在阶段边界停止；已下载文件保留。取消任务不会关闭已启动的游戏。
            </small>
          </div>
        )}
        {cancelError && (
          <p className="error-text" role="alert">
            {cancelError}
          </p>
        )}
        {task.state === "error" && (task as TaskV2).retrySpec != null && (
          <button
            className="button secondary"
            disabled={requesting}
            onClick={async () => {
              if (requestLock.current) return;
              requestLock.current = true;
              setRequesting(true);
              setCancelError("");
              try {
                await retryTask(task.taskId);
                await runtime?.core.refresh();
              } catch (cause) {
                setCancelError(parseApiError(cause).message);
              } finally {
                requestLock.current = false;
                setRequesting(false);
              }
            }}
          >
            重试任务
          </button>
        )}
      </div>
    </article>
  );
}
export function LiveTasks({ close }: { close: () => void }) {
  const { runtime } = usePreview();
  const tasks = runtime
    ? [...runtime.core.tasks.values()].sort((a, b) =>
        b.startedAt.localeCompare(a.startedAt),
      )
    : [];
  return (
    <Modal title="下载与任务" close={close} wide>
      {!tasks.length ? (
        <Empty
          title="暂时没有任务"
          detail={
            inTauri
              ? "安装和启动任务会在这里保留真实进度与错误。"
              : "浏览器预览 · 未连接，不生成示例任务。"
          }
        />
      ) : (
        tasks.map((task) => <TaskView key={task.taskId} task={task} />)
      )}
      <p className="muted">
        支持新契约的任务可请求取消，收到核心“已取消”才表示停止；旧核心不开放取消。历史未结束任务显示“上次退出时未完成”，不会自动续传。关闭列表不会停止任务。
      </p>
    </Modal>
  );
}
export function LiveDiagnostics({ close }: { close: () => void }) {
  const { runtime } = usePreview();
  const [text, setText] = useState("");
  const [error, setError] = useState("");
  const [copied, setCopied] = useState(false);
  return (
    <Modal title="日志与诊断" close={close} wide>
      <div className="diagnostic-content">
        {runtime?.core.launchError && (
          <MotionDetails open>
            <summary>当前启动错误</summary>
            <p className="error-text">{runtime.core.launchError}</p>
          </MotionDetails>
        )}
        <p className="muted">
          选择你要检查的日志，在本地脱敏预览；不会上传。AI 分析服务尚未接入。
        </p>
        <label className="field">
          选择 .log / .txt（最多 2 MB）
          <input
            type="file"
            accept=".log,.txt"
            onChange={async (event) => {
              const file = event.target.files?.[0];
              if (!file) return;
              setError("");
              setCopied(false);
              if (file.size > 2 * 1024 * 1024) {
                setError("文件超过 2 MB，请先截取相关片段。");
                return;
              }
              try {
                const raw = await file.text();
                setText(
                  raw
                    .replace(
                      /(Bearer\s+|access[_-]?token["'\s:=]+|refresh[_-]?token["'\s:=]+)[\w.\-/+]+/gi,
                      "$1[已隐藏]",
                    )
                    .replace(/[A-Z]:\\Users\\[^\\\s]+/gi, "[用户目录]")
                    .replace(/\b(?:\d{1,3}\.){3}\d{1,3}\b/g, "[IP 已隐藏]"),
                );
              } catch (cause) {
                setError(String(cause));
              }
            }}
          />
        </label>
        <label className="field">
          脱敏预览
          <textarea
            className="log-preview"
            readOnly
            value={text}
            placeholder="选择日志后在这里预览。自动规则不能保证识别所有个人信息，请再次检查。"
          />
        </label>
        {error && (
          <p className="error-text" role="alert">
            {error}
          </p>
        )}
        <div className="action-grid">
          <button
            className="button secondary"
            disabled={!text}
            onClick={async () => {
              try {
                await navigator.clipboard.writeText(text);
                setCopied(true);
              } catch (cause) {
                setError(String(cause));
              }
            }}
          >
            {copied ? "脱敏内容已复制" : "复制脱敏内容"}
          </button>
          <button className="button primary" disabled>
            AI 分析暂不可用
          </button>
        </div>
      </div>
    </Modal>
  );
}
export function LiveMore() {
  const { runtime, navigate, goBack, notify } = usePreview();
  const [diagnose, setDiagnose] = useState(false);
  const [advanced, setAdvanced] = useState(false);
  const [name, setName] = useState("");
  const [version, setVersion] = useState("");
  const [task, setTask] = useState<Task | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  return (
    <section className="content-page">
      <header className="page-title">
        <div>
          <h1 tabIndex={-1}>更多</h1>
          <p>日志、诊断与高级版本配置。</p>
        </div>
      </header>
      <div className="more-grid">
        <button onClick={() => setDiagnose(true)}>
          <Badge kind="log" />
          <h2>日志与诊断</h2>
          <p>本地脱敏预览，分享由你决定。</p>
        </button>
        <button onClick={() => navigate("versions")}>
          <Badge kind="folder" />
          <h2>游戏文件与存档</h2>
          <p>在所选版本下打开真实目录。</p>
        </button>
        <button
          disabled={runtime?.mode !== "core"}
          onClick={() => setAdvanced(true)}
        >
          <Badge kind="settings" />
          <h2>手动创建版本配置</h2>
          <p>高级入口；仅建立配置与隔离目录，不代表安装完成。</p>
        </button>
        <button
          onClick={() =>
            notify(
              "Astral Launcher · Forest Tale V2。原创森林插画与 SVG 图标；开源项目仅参考交互组织，没有移植受限代码或品牌素材。",
            )
          }
        >
          <Badge kind="star" />
          <h2>关于 Astral</h2>
          <p>给每个世界，留一个位置。</p>
        </button>
      </div>
      <ModalPresence present={diagnose}>
        <LiveDiagnostics close={() => setDiagnose(false)} />
      </ModalPresence>
      <ModalPresence present={advanced}>
        <Modal
          title="手动创建版本配置"
          close={() => {
            if (!busy) setAdvanced(false);
          }}
        >
          <label className="field">
            名称
            <input
              value={name}
              maxLength={80}
              onChange={(event) => setName(event.target.value)}
            />
          </label>
          <label className="field">
            Minecraft 版本号
            <input
              value={version}
              placeholder="例如 1.21.1"
              onChange={(event) => setVersion(event.target.value)}
            />
          </label>
          <p className="muted">只创建独立配置。游戏文件仍需从下载页补全。</p>
          {task && <TaskView key={task.taskId} task={task} />}
          {error && (
            <p className="error-text" role="alert">
              {error}
            </p>
          )}
          <button
            className="button primary full"
            disabled={busy || !name.trim() || !version.trim()}
            onClick={async () => {
              setBusy(true);
              setError("");
              try {
                await waitForTask(
                  await runtime!.core.create({
                    name: name.trim(),
                    gameVersion: version.trim(),
                  }),
                  setTask,
                );
                await runtime!.core.refresh();
              } catch (cause) {
                setError(parseApiError(cause).message);
              } finally {
                setBusy(false);
              }
            }}
          >
            创建配置
          </button>
        </Modal>
      </ModalPresence>
    </section>
  );
}
export function Health({ instance }: { instance: Instance }) {
  const { navigate } = usePreview();
  return !instance.health.ok ? (
    <div className="banner warn">
      <Icon name="warning" />
      <div>
        <strong>{instance.health.reasons[0] || "游戏文件需要检查"}</strong>
        <MotionDetails>
          <summary>查看原因</summary>
          {instance.health.reasons.map((reason, index) => (
            <p key={index}>{reason}</p>
          ))}
        </MotionDetails>
        <button
          className="text-button"
          onClick={() =>
            navigate(`game/${encodeURIComponent(instance.gameVersion)}`)
          }
        >
          查看此游戏的安装选项
          <Icon name="arrow" />
        </button>
      </div>
    </div>
  ) : null;
}
export function LiveLaunch() {
  const { runtime, prefs, notify, openDiagnostics } = usePreview();
  if (!runtime) return null;
  const { core } = runtime;
  const hasState =
    (core.phase !== "idle" && core.phase !== "recovering") ||
    !!core.launchError;
  if (runtime.mode !== "core")
    return runtime.mode === "legacy" ? (
      <div className="banner">
        旧核心仅支持准备进程，无法确认会话；建议使用带 sessions/tasks
        能力的核心。
      </div>
    ) : null;
  const failed = core.phase === "error" || !!core.launchError;
  const active =
    !failed && ["submitting", "starting", "confirming"].includes(core.phase);
  const label = failed
    ? "启动未完成"
    : core.phase === "running"
      ? "游戏进程运行中"
      : core.phase === "confirming"
        ? "进程已创建，正在查询会话"
        : core.phase === "unknown"
          ? "无法确认游戏会话"
          : core.phase === "exited"
            ? "游戏进程已退出"
            : core.phaseLabel;
  return (
    <>
      {core.current && <Health instance={core.current} />}
      {core.error && (
        <p className="error-text" role="alert">
          {core.error}
        </p>
      )}
      {hasState && (
        <section
          className={`real-preparation ${failed ? "failed" : ""}`}
          role="status"
          data-launch-phase={failed ? "error" : core.phase}
        >
          <Effect
            variant={prefs.launchEffect}
            active={active}
            phase={failed ? "error" : core.phase}
          />
          <div className="real-preparation-copy">
            <h3>{label}</h3>
            <p>
              {core.launchError ||
                (active
                  ? core.launchTask?.label || "等待核心返回准备阶段"
                  : core.phase === "running"
                    ? "会话进程已确认；当前接口不检测游戏窗口或主菜单。"
                    : core.phase === "exited"
                      ? `退出码 ${core.session?.exitCode ?? "未知"}`
                      : "等待核心会话查询，不根据 PID 缓存猜测。")}
            </p>
            {active && core.launchTask && (
              <progress
                aria-label="当前启动阶段进度"
                max={
                  core.launchTask.total > 0 ? core.launchTask.total : undefined
                }
                value={
                  core.launchTask.total > 0
                    ? Math.min(core.launchTask.done, core.launchTask.total)
                    : undefined
                }
              />
            )}
            {failed && (
              <div className="failure-actions">
                <button
                  onClick={async () => {
                    const text = [
                      core.launchError,
                      core.launchTask?.error?.detail,
                    ]
                      .filter(Boolean)
                      .join("\n");
                    try {
                      await navigator.clipboard.writeText(text);
                    } catch {
                      notify(text);
                    }
                  }}
                >
                  复制错误
                </button>
                <button onClick={openDiagnostics}>诊断</button>
                <button
                  disabled={!core.canLaunch}
                  onClick={() => void core.launch()}
                >
                  重新启动
                </button>
              </div>
            )}
          </div>
        </section>
      )}
    </>
  );
}
export function LiveSettings() {
  const { runtime, notify, prefs, setPrefs } = usePreview();
  const [username, setUsername] = useState(runtime?.config?.username || "");
  const [saving, setSaving] = useState(false);
  const [afterLaunch, setAfterLaunch] = useState(() => {
    try {
      return localStorage.getItem("astral.v2.afterLaunch") || "keep";
    } catch {
      return "keep";
    }
  });
  useEffect(
    () => setUsername(runtime?.config?.username || ""),
    [runtime?.config?.username],
  );
  const available = runtime?.mode === "core";
  async function save(patch: {
    username?: string;
    source?: "official" | "bmclapi";
  }) {
    setSaving(true);
    try {
      await patchConfig(patch);
      await runtime?.refresh();
    } catch (cause) {
      notify(parseApiError(cause).message);
    } finally {
      setSaving(false);
    }
  }
  return (
    <>
      <section id="runtime-settings">
        <h2>启动与运行</h2>
        <div className="setting-row">
          <div>
            <strong>启动游戏的动效</strong>
            <p>三种方向都保留；效果由真实 Task / GameSession 驱动。</p>
          </div>
          <select
            aria-label="启动动效"
            value={prefs.launchEffect}
            onChange={(event) =>
              setPrefs((old) => ({
                ...old,
                launchEffect: event.target.value as typeof prefs.launchEffect,
              }))
            }
          >
            <option value="fireflies">森林萤火</option>
            <option value="voxel">像素方块组合</option>
            <option value="light">极简光影</option>
          </select>
        </div>
        <div className="setting-row">
          <div>
            <strong>Java、内存与 JVM 参数</strong>
            <p>按游戏版本配置；自定义 Java 路径尚未由核心消费。</p>
          </div>
          <button
            className="button secondary"
            onClick={() => (location.hash = "versions")}
          >
            版本运行设置
          </button>
        </div>
        <div className="setting-row">
          <div>
            <strong>启动游戏后</strong>
            <p>最小化不会关闭游戏；自动关闭与托盘策略尚未实现。</p>
          </div>
          <select
            aria-label="启动后行为"
            value={afterLaunch}
            onChange={(event) => {
              try {
                localStorage.setItem(
                  "astral.v2.afterLaunch",
                  event.target.value,
                );
                setAfterLaunch(event.target.value);
              } catch {
                notify("无法保存此设置，请检查本地存储权限。");
              }
            }}
          >
            <option value="keep">保持打开</option>
            <option value="minimize">最小化启动器</option>
          </select>
        </div>
      </section>
      <section id="network-settings">
        <h2 id="account-settings">账号与下载</h2>
        <label className="field">
          离线用户名
          <input
            aria-label="离线用户名"
            maxLength={16}
            value={username}
            disabled={!available || saving}
            onChange={(event) => setUsername(event.target.value)}
          />
        </label>
        <button
          className="button secondary"
          disabled={
            !available ||
            saving ||
            !username.trim() ||
            username === runtime?.config?.username
          }
          onClick={() => void save({ username: username.trim() })}
        >
          保存离线档案
        </button>
        <p className="muted">Microsoft 正版授权尚未提供，不显示已验证身份。</p>
        <div className="setting-row">
          <div>
            <strong>下载源</strong>
            <p>下载由 Rust 完成，并校验游戏文件。</p>
          </div>
          <select
            aria-label="下载源"
            disabled={!available || saving}
            value={runtime?.config?.source || "official"}
            onChange={(event) =>
              void save({
                source: event.target.value as "official" | "bmclapi",
              })
            }
          >
            <option value="official">官方</option>
            <option value="bmclapi">BMCLAPI</option>
          </select>
        </div>
        <div className="setting-row">
          <div>
            <strong>下载并发、限速与重试</strong>
            <p>并发与限速设置尚未提供；任务支持在文件边界请求取消。</p>
          </div>
          <span className="tag">待核心支持</span>
        </div>
      </section>
      <JavaManager />
    </>
  );
}
export function LiveVersions() {
  const { runtime, navigate, goBack, notify, setCurrentId } = usePreview();
  const core = runtime!.core;
  const [selected, setSelected] = useState(core.activeId || "");
  const [tab, setTab] = useState("overview");
  const [query, setQuery] = useState("");
  const current =
    core.instances.find((item) => item.id === selected) ||
    core.current ||
    core.instances[0];
  const [editing, setEditing] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [ack, setAck] = useState(false);
  const [name, setName] = useState("");
  const [memory, setMemory] = useState("");
  const [jvm, setJvm] = useState("");
  const [error, setError] = useState("");
  const [pending, setPending] = useState(false);
  const [mods, setMods] = useState<ModFile[]>([]);
  const [saves, setSaves] = useState<CoreSaveEntry[]>([]);
  const generation = useRef(0);
  const hasMods = runtime!.api?.features.includes("mods");
  const hasSaves = runtime!.api?.features.includes("saves");
  useEffect(() => {
    const ticket = ++generation.current;
    setMods([]);
    setSaves([]);
    setError("");
    if (!current) return;
    const request =
      tab === "mods" && current.managed && hasMods
        ? listMods(current.id).then((rows) => {
            if (ticket === generation.current) setMods(rows);
          })
        : tab === "saves" && hasSaves
          ? listSaves(current.id).then((rows) => {
              if (ticket === generation.current) setSaves(rows);
            })
          : Promise.resolve();
    request.catch((cause) => {
      if (ticket === generation.current) setError(parseApiError(cause).message);
    });
    return () => {
      ++generation.current;
    };
  }, [current, tab, hasMods, hasSaves]);
  async function action(run: () => Promise<unknown>) {
    if (pending) return;
    setError("");
    setPending(true);
    try {
      await run();
    } catch (cause) {
      const e = parseApiError(cause);
      setError(
        e.code === "INSTANCE_BUSY"
          ? `游戏正在运行或任务正在写入，请退出或等待完成。${e.message}`
          : e.message,
      );
    } finally {
      setPending(false);
    }
  }
  function openEdit() {
    setName(current.name);
    setMemory(current.memoryMb === null ? "" : String(current.memoryMb));
    setJvm(current.jvmArgs.join("\n"));
    setEditing(true);
    setError("");
  }
  async function save() {
    const patch: InstancePatch = {};
    if (name.trim() !== current.name) patch.name = name.trim();
    const mem = memory.trim() ? Number(memory) : null;
    if (mem !== null && (!Number.isInteger(mem) || mem < 512 || mem > 131072))
      throw Error("内存应为 512–131072 MB 的整数，或留空自动分配");
    if (mem !== current.memoryMb) patch.memoryMb = mem;
    const args = jvm
      .split("\n")
      .map((value) => value.trim())
      .filter(Boolean);
    if (JSON.stringify(args) !== JSON.stringify(current.jvmArgs))
      patch.jvmArgs = args;
    if (Object.keys(patch).length) await core.edit(current.id, patch);
    setEditing(false);
  }
  if (runtime!.mode === "legacy")
    return (
      <section className="content-page">
        <h1>我的版本</h1>
        <p className="muted">
          当前核心只提供共享游戏目录，编辑与删除暂不可用。
        </p>
        <div className="choice-list">
          {runtime!.legacy.versions.map((id) => (
            <button
              key={id}
              onClick={() => {
                setCurrentId(id);
                navigate("home");
              }}
            >
              <Badge kind="game" />
              <span>
                <strong>{id}</strong>
                <small>共享游戏目录</small>
              </span>
              <Icon name="arrow" />
            </button>
          ))}
        </div>
      </section>
    );
  return (
    <section className="content-page">
      <button className="breadcrumb" onClick={() => goBack("home")}>
        <Icon name="back" />
        启动 / 我的版本
      </button>
      <header className="page-title">
        <div>
          <h1 tabIndex={-1}>版本管理</h1>
          <p>Mod、存档和运行配置，跟随你选中的游戏。</p>
        </div>
        <button
          className="button primary"
          onClick={() => navigate("download/game")}
        >
          下载游戏
        </button>
      </header>
      {!current ? (
        <Empty
          title={
            runtime!.mode === "browser"
              ? "浏览器预览 · 未连接"
              : "还没有游戏版本"
          }
          detail="连接核心后读取真实列表；一般通过下载游戏自动创建配置。"
        />
      ) : (
        <div className="version-manager">
          <aside>
            <label className="search">
              <Icon name="search" />
              <input
                aria-label="搜索我的版本"
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                placeholder="搜索我的版本"
              />
            </label>
            {core.instances
              .filter((item) =>
                (item.name + item.gameVersion)
                  .toLowerCase()
                  .includes(query.toLowerCase()),
              )
              .map((item) => (
                <button
                  key={item.id}
                  className={item.id === current.id ? "active" : ""}
                  onClick={() => setSelected(item.id)}
                >
                  <Badge kind={item.loader?.kind || "game"} />
                  <span>
                    <strong>{item.name}</strong>
                    <small>
                      {item.gameVersion} · {item.loader?.kind || "原版"}
                    </small>
                  </span>
                </button>
              ))}
          </aside>
          <div className="version-detail">
            <header>
              <Badge kind={current.loader?.kind || "game"} large />
              <div>
                <h2>{current.name}</h2>
                <p>
                  {current.gameVersion} ·{" "}
                  {current.loader
                    ? `${current.loader.kind} ${current.loader.version}`
                    : "纯净原版"}
                </p>
              </div>
              <button
                className="button primary"
                disabled={pending}
                onClick={() => {
                  setCurrentId(current.id);
                  navigate("home");
                }}
              >
                前往启动
              </button>
            </header>
            <div className="tabs">
              {[
                ["overview", "总览"],
                ["components", "安装组件"],
                ["mods", "Mod"],
                ["saves", "存档与资源"],
                ["runtime", "运行设置"],
                ["logs", "日志"],
              ].map(([id, label]) => (
                <button
                  key={id}
                  aria-pressed={tab === id}
                  onClick={() => setTab(id)}
                >
                  {label}
                </button>
              ))}
            </div>
            <MotionRegion changeKey={tab}>
              {!current.managed && (
                <div className="banner warn">
                  共享游戏目录：为保护其他版本，不提供改名、删除或目录内写入。
                </div>
              )}
              <Health instance={current} />
              {error && !editing && !deleting && (
                <p className="error-text" role="alert">
                  {error}
                </p>
              )}
              {tab === "overview" && (
                <>
                  <div className="detail-grid">
                    <div>
                      <span>游戏版本</span>
                      <strong>{current.gameVersion}</strong>
                    </div>
                    <div>
                      <span>已启用 Mod</span>
                      <strong>{current.modsCount}</strong>
                    </div>
                    <div>
                      <span>停用 Mod</span>
                      <strong>{current.modsDisabled}</strong>
                    </div>
                    <div>
                      <span>目录</span>
                      <strong>
                        {current.managed ? "独立保存" : "与其他版本共享"}
                      </strong>
                    </div>
                  </div>
                  <div className="action-grid">
                    <button
                      className="button secondary"
                      onClick={() =>
                        void action(() => openPathChecked(current.dir))
                      }
                    >
                      打开游戏目录
                    </button>
                    <button
                      className="button secondary"
                      disabled={!current.managed || pending}
                      onClick={openEdit}
                    >
                      编辑名称与配置
                    </button>
                    <button className="button secondary" disabled>
                      复制配置 · 待核心支持
                    </button>
                    <button
                      className="button danger"
                      disabled={!current.managed || pending}
                      onClick={() => {
                        setDeleting(true);
                        setAck(false);
                        setError("");
                      }}
                    >
                      删除与数据保护
                    </button>
                  </div>
                  <VersionMaintenance key={current.id} instance={current} />
                </>
              )}
              {tab === "components" && (
                <>
                  <div className="file-row">
                    <Badge kind={current.loader?.kind || "game"} />
                    <span>
                      <strong>Minecraft {current.gameVersion}</strong>
                      <small>
                        {current.loader
                          ? `${current.loader.kind} ${current.loader.version}`
                          : "纯净原版"}
                      </small>
                    </span>
                    <button
                      className="button secondary"
                      onClick={() =>
                        navigate(
                          `game/${encodeURIComponent(current.gameVersion)}`,
                        )
                      }
                    >
                      查看可用组件
                    </button>
                  </div>
                  <p className="muted">
                    更换已有游戏的 Loader
                    需要独立兼容性检查；此入口创建新配置，不直接覆盖现有世界。
                  </p>
                </>
              )}
              {tab === "mods" &&
                (!hasMods || !current.managed ? (
                  <Empty
                    title="当前版本不能管理 Mod"
                    detail="需要独立游戏目录与核心 mods 能力。"
                  />
                ) : (
                  <>
                    <ModUpdates
                      key={current.id}
                      instance={current}
                      onUpdated={async () => {
                        setMods(await listMods(current.id));
                        await core.refresh();
                      }}
                    />
                    <button
                      className="text-button"
                      onClick={() => navigate("download/mod")}
                    >
                      发现 Mod
                      <Icon name="arrow" />
                    </button>
                    {mods.map((mod) => (
                      <div className="file-row" key={mod.fileName}>
                        <Badge kind="mod" />
                        <span>
                          <strong>{mod.name}</strong>
                          <small>
                            {mod.version || "版本未知"} · {mod.fileName}
                          </small>
                        </span>
                        <Toggle
                          disabled={pending}
                          label={`启用 ${mod.name}`}
                          checked={mod.enabled}
                          onChange={() =>
                            void action(async () => {
                              await setModEnabled(
                                current.id,
                                mod.fileName,
                                !mod.enabled,
                              );
                              setMods(await listMods(current.id));
                              await core.refresh();
                            })
                          }
                        />
                        <button
                          className="button danger"
                          disabled={pending}
                          onClick={() => {
                            if (
                              confirm(
                                `永久删除 ${mod.fileName}？此操作没有回收站。`,
                              )
                            )
                              void action(async () => {
                                await deleteMod(current.id, mod.fileName);
                                setMods(await listMods(current.id));
                                await core.refresh();
                              });
                          }}
                        >
                          删除
                        </button>
                      </div>
                    ))}
                    {!mods.length && (
                      <p className="muted">没有扫描到 Mod 文件。</p>
                    )}
                  </>
                ))}
              {tab === "saves" && (
                <>
                  {saves.map((save) => (
                    <div className="file-row" key={save.dir}>
                      <Badge kind="folder" />
                      <span>
                        <strong>{save.name}</strong>
                        <small>
                          {save.gameVersion || "版本未知"} ·{" "}
                          {save.lastModified.slice(0, 10)}
                        </small>
                      </span>
                      <button
                        className="button secondary"
                        onClick={() =>
                          void action(() => openPathChecked(save.dir))
                        }
                      >
                        打开目录
                      </button>
                    </div>
                  ))}
                  <div className="action-grid">
                    {(
                      [
                        "saves",
                        "resourcepacks",
                        "shaderpacks",
                        "screenshots",
                      ] as const
                    ).map((key) => (
                      <button
                        className="button secondary"
                        key={key}
                        onClick={() =>
                          void action(() => openPathChecked(current.dirs[key]))
                        }
                      >
                        打开
                        {
                          {
                            saves: "存档",
                            resourcepacks: "资源包",
                            shaderpacks: "光影包",
                            screenshots: "截图",
                          }[key]
                        }
                        目录
                      </button>
                    ))}
                  </div>
                </>
              )}
              {tab === "runtime" && (
                <>
                  <p className="muted">
                    内存{" "}
                    {current.memoryMb === null
                      ? "自动"
                      : `${current.memoryMb} MB`}
                    ；JVM 参数按行保存。自定义 Java 尚未生效。
                  </p>
                  <button
                    className="button secondary"
                    disabled={!current.managed || pending}
                    onClick={openEdit}
                  >
                    编辑运行设置
                  </button>
                </>
              )}
              {tab === "logs" && (
                <>
                  <GameLogs key={current.id} instance={current} />
                  <p className="muted">
                    日志不会自动上传；先在本地查看，分享前检查个人信息。
                  </p>
                  <button
                    className="button secondary"
                    onClick={() =>
                      void action(() => openPathChecked(current.dirs.logs))
                    }
                  >
                    打开日志目录
                  </button>
                </>
              )}
            </MotionRegion>
          </div>
        </div>
      )}
      <ModalPresence present={editing}>
        <Modal
          title="编辑版本配置"
          close={() => {
            if (!pending) setEditing(false);
          }}
        >
          <label className="field">
            名称
            <input
              value={name}
              onChange={(event) => setName(event.target.value)}
              maxLength={80}
            />
          </label>
          <label className="field">
            内存 MB（留空自动）
            <input
              aria-label="游戏内存"
              value={memory}
              onChange={(event) => setMemory(event.target.value)}
              inputMode="numeric"
            />
          </label>
          <label className="field">
            JVM 参数（每行一项）
            <textarea
              value={jvm}
              onChange={(event) => setJvm(event.target.value)}
            />
          </label>
          {error && (
            <p className="error-text" role="alert">
              {error}
            </p>
          )}
          <button
            className="button primary full"
            disabled={pending || !name.trim()}
            onClick={() => void action(save)}
          >
            保存修改
          </button>
        </Modal>
      </ModalPresence>
      <ModalPresence present={deleting}>
        <Modal
          title="确认删除这个版本？"
          close={() => {
            if (!pending) setDeleting(false);
          }}
        >
          <h3>{current?.name}</h3>
          <p className="body-copy break">{current?.dir}</p>
          <p className="muted">
            mods、config、saves
            等随目录移入应用回收站；当前尚无恢复界面。其他独立版本不变。
          </p>
          <label className="dependency-row">
            <input
              type="checkbox"
              checked={ack}
              onChange={(event) => setAck(event.target.checked)}
            />
            我了解此版本的存档也会移动
          </label>
          {error && (
            <p className="error-text" role="alert">
              {error}
            </p>
          )}
          <button
            className="button danger full"
            disabled={!ack || pending}
            onClick={() =>
              void action(async () => {
                await core.remove(current.id);
                setDeleting(false);
              })
            }
          >
            确认移入回收站
          </button>
        </Modal>
      </ModalPresence>
    </section>
  );
}
