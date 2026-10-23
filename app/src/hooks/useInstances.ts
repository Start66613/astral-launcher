import { useCallback, useEffect, useRef, useState } from "react";
import {
  ApiError, deleteInstance, getActiveInstance, getActiveSession, getCoreConfig, getTasks,
  isTerminal, listInstances, listSessions, mergeTask, onGameExit, onInstanceChanged,
  onTask, parseApiError, setActiveInstance, startCreateInstance, startLaunch, updateInstance,
} from "../lib/bridge";
import type { CoreConfig, CreateInstanceRequest, GameExitPayload, GameSession, Instance, InstancePatch, Task, Unlisten } from "../lib/types";

export type LaunchPhase = "recovering" | "idle" | "submitting" | "starting" | "confirming" | "running" | "exited" | "error" | "unknown";
const PHASE_LABELS: Record<LaunchPhase, string> = {
  recovering: "恢复查询中", idle: "尚未启动", submitting: "正在提交启动请求", starting: "启动中",
  confirming: "正在确认运行状态", running: "运行中", exited: "已退出", error: "启动未完成", unknown: "运行状态未知",
};

// This hook lives in App, so page navigation never drops task/session subscriptions.
export function useInstances(enabled: boolean, features: string[]) {
  const hasTasks = features.includes("tasks");
  const hasSessions = features.includes("sessions");
  const [instances, setInstances] = useState<Instance[]>([]);
  const [config, setConfig] = useState<CoreConfig | null>(null);
  const [tasks, setTasks] = useState(new Map<string, Task>());
  const [sessions, setSessions] = useState(new Map<string, GameSession>());
  const [sessionErrors, setSessionErrors] = useState<Record<string, string>>({});
  const [sessionChecks, setSessionChecks] = useState<Record<string, boolean>>({});
  const [status, setStatus] = useState<"loading" | "ready" | "error">("loading");
  const [error, setError] = useState("");
  const [refreshing, setRefreshing] = useState(false);
  const [mutating, setMutating] = useState("");
  const [submitting, setSubmitting] = useState<string | null>(null);
  const [launchErrors, setLaunchErrors] = useState<Record<string, string>>({});
  const [createTaskId, setCreateTaskId] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [connectionRetry, setConnectionRetry] = useState(0);
  const epoch = useRef(0);
  const live = useRef(false);
  const listening = useRef(false);
  const initialized = useRef(false);
  const pendingInstanceChange = useRef(false);
  const readGeneration = useRef(0);
  const sessionGeneration = useRef(new Map<string, number>());
  const taskRef = useRef(new Map<string, Task>());
  const sessionRef = useRef(new Map<string, GameSession>());
  const exits = useRef(new Map<string, GameExitPayload>());
  const actionLock = useRef(false);
  const launchLock = useRef(false);
  const submitBaselineTask = useRef<string | undefined>(undefined);
  const createLock = useRef(false);
  const refreshAction = useRef<() => Promise<void>>(async () => {});
  const sessionAction = useRef<(id: string) => Promise<void>>(async () => {});

  const rememberSession = useCallback((incoming: GameSession) => {
    const exit = exits.current.get(incoming.sessionId);
    const next: GameSession = exit?.instanceId === incoming.instanceId
      ? { ...incoming, state: "exited", exitCode: exit.exitCode, endedAt: exit.endedAt, durationMs: exit.durationMs }
      : incoming;
    const current = sessionRef.current.get(next.instanceId);
    if (current?.sessionId === next.sessionId && current.state === "exited" && next.state !== "exited") return;
    if (current && current.sessionId !== next.sessionId && current.startedAt > next.startedAt) return;
    sessionRef.current = new Map(sessionRef.current).set(next.instanceId, next);
    setSessions(sessionRef.current);
  }, []);

  const merge = useCallback((task: Task) => {
    const next = mergeTask(taskRef.current, task); // Contract implementation; no parallel reducer.
    if (next === taskRef.current) return;
    taskRef.current = next; setTasks(next);
    if (task.kind === "launch" && task.state === "done" && task.instanceId && task.result?.kind === "launch") {
      const result = task.result;
      const exit = exits.current.get(result.sessionId);
      if (exit?.instanceId === task.instanceId) rememberSession({
        ...result, instanceId: task.instanceId, taskId: task.taskId,
        state: "exited", exitCode: exit.exitCode, endedAt: exit.endedAt, durationMs: exit.durationMs,
      });
      if (initialized.current) void sessionAction.current(task.instanceId);
    }
    if ((task.kind === "create" || task.kind === "duplicate") && task.state === "done" && initialized.current) void refreshAction.current();
  }, [rememberSession]);

  const syncSession = useCallback(async (id: string) => {
    if (!live.current || !hasSessions) return;
    const ticket = epoch.current;
    const sequence = (sessionGeneration.current.get(id) ?? 0) + 1;
    sessionGeneration.current.set(id, sequence);
    const valid = () => live.current && ticket === epoch.current && sequence === sessionGeneration.current.get(id);
    setSessionChecks(old => ({ ...old, [id]: true }));
    try {
      let session = await getActiveSession(id);
      if (!session) session = (await listSessions({ instanceId: id, limit: 1 }))[0] ?? null;
      if (!valid()) return;
      if (session?.instanceId === id) rememberSession(session);
      else if (!session && sessionRef.current.get(id)?.state !== "exited") {
        sessionRef.current = new Map(sessionRef.current);
        sessionRef.current.delete(id); setSessions(sessionRef.current);
      } else if (session) throw new ApiError("INTERNAL", "核心返回的会话归属与当前实例不一致");
      setSessionErrors(old => ({ ...old, [id]: "" }));
    } catch (cause) { if (valid()) setSessionErrors(old => ({ ...old, [id]: parseApiError(cause).message })); }
    finally { if (valid()) setSessionChecks(old => ({ ...old, [id]: false })); }
  }, [hasSessions, rememberSession]);
  sessionAction.current = syncSession;

  const applyConfig = useCallback((next: CoreConfig) => { setConfig(next); }, []);
  const refresh = useCallback(async () => {
    if (!live.current) return;
    if (!listening.current) { setConnectionRetry(value => value + 1); return; }
    const ticket = epoch.current;
    const generation = ++readGeneration.current;
    const valid = () => live.current && ticket === epoch.current && generation === readGeneration.current;
    setRefreshing(true);
    try {
      if (hasTasks) { const snapshot = await getTasks(); if (!valid()) return; snapshot.forEach(merge); }
      const active = await getActiveInstance();
      const [list, cfg] = await Promise.all([listInstances(), getCoreConfig()]);
      if (!valid()) return;
      const complete = active && active.id === cfg.activeInstanceId && !list.some(item => item.id === active.id) ? [...list, active] : list;
      setInstances(complete); applyConfig(cfg); setError("");
      if (cfg.activeInstanceId && hasSessions) await syncSession(cfg.activeInstanceId);
      if (!valid()) return;
      initialized.current = true; setStatus("ready");
      if (pendingInstanceChange.current) { pendingInstanceChange.current = false; void refreshAction.current(); }
    } catch (cause) { if (valid()) { setError(parseApiError(cause).message); setStatus("error"); } }
    finally { if (valid()) setRefreshing(false); }
  }, [hasTasks, hasSessions, merge, applyConfig, syncSession]);
  refreshAction.current = refresh;

  useEffect(() => {
    if (!enabled) return;
    const ticket = ++epoch.current;
    live.current = true; listening.current = false; initialized.current = false;
    setStatus("loading"); setError("");
    const valid = () => live.current && ticket === epoch.current;
    const unlisten: Unlisten[] = [];
    async function attach(promise: Promise<Unlisten>) {
      const un = await promise;
      if (!valid()) un(); else unlisten.push(un);
    }
    void (async () => {
      try {
        if (hasTasks) await attach(onTask(task => { if (valid()) merge(task); }));
        if (!valid()) return;
        if (hasSessions) await attach(onGameExit(event => {
          if (!valid()) return;
          exits.current.set(event.sessionId, event);
          const current = sessionRef.current.get(event.instanceId);
          if (current && current.sessionId !== event.sessionId) return;
          if (current?.sessionId === event.sessionId) rememberSession(current);
          // Also handle an exit that preceded the command's done frame.
          for (const task of taskRef.current.values()) {
            if (task.instanceId === event.instanceId && task.result?.kind === "launch" && task.result.sessionId === event.sessionId) {
              rememberSession({ ...task.result, taskId: task.taskId, instanceId: event.instanceId,
                state: "exited", exitCode: event.exitCode, endedAt: event.endedAt, durationMs: event.durationMs });
            }
          }
        }));
        if (!valid()) return;
        await attach(onInstanceChanged(() => {
          if (!valid()) return;
          if (initialized.current) void refreshAction.current(); else pendingInstanceChange.current = true;
        }));
        if (!valid()) return;
        listening.current = true;
        await refreshAction.current();
      } catch (cause) {
        unlisten.splice(0).forEach(un => un());
        if (valid()) { listening.current = false; setError(parseApiError(cause).message); setStatus("error"); }
      }
    })();
    const onFocus = () => { if (valid() && initialized.current) void refreshAction.current(); };
    window.addEventListener("focus", onFocus);
    return () => {
      live.current = false; listening.current = false; ++epoch.current; ++readGeneration.current;
      unlisten.forEach(un => un()); window.removeEventListener("focus", onFocus);
    };
  }, [enabled, hasTasks, hasSessions, merge, rememberSession, connectionRetry]);

  async function mutate<T>(label: string, operation: () => Promise<T>, apply: (value: T) => void): Promise<T> {
    if (!live.current || !initialized.current) throw new ApiError("BUSY", "实例状态尚未读取完成");
    if (actionLock.current) throw new ApiError("BUSY", "上一项操作尚未完成，请稍候");
    const ticket = epoch.current;
    actionLock.current = true; ++readGeneration.current; setRefreshing(false); setMutating(label);
    try {
      const value = await operation();
      if (live.current && ticket === epoch.current) { ++readGeneration.current; apply(value); }
      return value;
    } finally { actionLock.current = false; if (live.current && ticket === epoch.current) setMutating(""); }
  }
  async function select(id: string) {
    const cfg = await mutate("select", () => setActiveInstance(id), applyConfig);
    if (cfg.activeInstanceId) await syncSession(cfg.activeInstanceId);
    // The returned Config is authoritative, even if the core selected another ID.
    return cfg;
  }
  async function edit(id: string, patch: InstancePatch) {
    const updated = await mutate("edit", () => updateInstance(id, patch), value => setInstances(old => old.map(item => item.id === value.id ? value : item)));
    await refresh(); return updated;
  }
  async function remove(id: string) {
    try {
      const cfg = await mutate("delete", () => deleteInstance(id, { trash: true }), value => {
        applyConfig(value); setInstances(old => old.filter(item => item.id !== id));
      });
      await refresh(); return cfg;
    } catch (cause) { await refresh(); throw cause; }
  }
  async function create(request: CreateInstanceRequest) {
    if (!live.current || !initialized.current || !hasTasks) throw new ApiError("UNSUPPORTED", "当前核心没有实例创建任务能力");
    if (createLock.current) throw new ApiError("BUSY", "正在提交创建请求");
    createLock.current = true; setCreating(true);
    try { const task = await startCreateInstance(request); if (live.current) { merge(task); setCreateTaskId(task.taskId); } return task; }
    finally { createLock.current = false; if (live.current) setCreating(false); }
  }

  const activeId = config?.activeInstanceId ?? null;
  const current = instances.find(instance => instance.id === activeId) ?? null;
  const launchTask = [...tasks.values()].reverse().filter(task => task.kind === "launch" && task.instanceId === activeId)
    .sort((a, b) => b.startedAt.localeCompare(a.startedAt))[0];
  const session = activeId ? sessions.get(activeId) ?? null : null;
  const expectedSession = launchTask?.result?.kind === "launch" ? launchTask.result.sessionId : null;
  const matchingSession = session && (!expectedSession || expectedSession === session.sessionId) ? session : null;
  let phase: LaunchPhase = "idle";
  if (status === "loading") phase = "recovering";
  else if (activeId && submitting === activeId && (!launchTask || launchTask.taskId === submitBaselineTask.current)) phase = "submitting";
  else if (launchTask && !isTerminal(launchTask.state)) phase = "starting";
  else if (launchTask?.state === "error" || launchTask?.state === "cancelled") phase = "error";
  else if (activeId && sessionErrors[activeId]) phase = "unknown";
  else if (matchingSession?.state === "exited") phase = "exited";
  else if (activeId && sessionChecks[activeId]) phase = launchTask?.state === "done" ? "confirming" : "recovering";
  else if (matchingSession?.state === "running") phase = "running";
  else if (matchingSession?.state === "starting") phase = "starting";
  else if (launchTask?.state === "done") phase = activeId && sessionChecks[activeId] ? "confirming" : "unknown";
  const launchBusy = ["recovering", "submitting", "starting", "confirming", "unknown"].includes(phase);
  const canLaunch = enabled && hasTasks && hasSessions && status === "ready" && !!current?.health?.ok && !launchBusy && phase !== "running";
  async function launch() {
    if (!canLaunch || !activeId || launchLock.current) return;
    const id = activeId;
    launchLock.current = true; submitBaselineTask.current = launchTask?.taskId;
    setSubmitting(id); setLaunchErrors(old => ({ ...old, [id]: "" }));
    try {
      const task = await startLaunch(id);
      if (!live.current) return;
      merge(task);
      // The event may already have delivered a newer done frame before this response.
      if (taskRef.current.get(task.taskId)?.state === "done") await syncSession(id);
    } catch (cause) {
      if (live.current) { setLaunchErrors(old => ({ ...old, [id]: parseApiError(cause).message })); await syncSession(id); }
    } finally { launchLock.current = false; if (live.current) setSubmitting(null); }
  }
  return {
    instances, config, current, activeId, tasks, status, error, refreshing, mutating,
    hasTasks, hasSessions, phase, phaseLabel: PHASE_LABELS[phase], session: matchingSession, launchTask,
    launchBusy, canLaunch, launchError: activeId ? launchErrors[activeId] || launchTask?.error?.message || sessionErrors[activeId] || "" : "",
    createTask: createTaskId ? tasks.get(createTaskId) ?? null : null, creating,
    refresh, select, edit, remove, create, launch,
  };
}
