import { useCallback, useEffect, useRef, useState } from "react";
import { getConfig, inTauri, launchVersion, onProgress, scanVersions, type Progress } from "../lib/bridge";
import type { LauncherConfig } from "../lib/types";

// Presentation state around the existing core; no unimplemented v2/v3 commands.
export function useLocalLauncher() {
  const [versions, setVersions] = useState<string[]>([]);
  const [current, setCurrent] = useState("");
  const [config, setConfig] = useState<LauncherConfig | null>(null);
  const [status, setStatus] = useState<"browser" | "loading" | "ready" | "error">(inTauri ? "loading" : "browser");
  const [error, setError] = useState("");
  const [launchError, setLaunchError] = useState("");
  const [launchBusy, setLaunchBusy] = useState(false);
  const [progress, setProgress] = useState<Progress | null>(null);
  const [pid, setPid] = useState<number | null>(null);
  const loadGeneration = useRef(0);
  const mounted = useRef(true);
  const launchLock = useRef(false);
  const listener = useRef<(() => void) | undefined>(undefined);
  const refresh = useCallback(async () => {
    if (!inTauri) return;
    const generation = ++loadGeneration.current;
    setStatus("loading"); setError("");
    const [list, cfg] = await Promise.allSettled([scanVersions(), getConfig()]);
    if (!mounted.current || generation !== loadGeneration.current) return;
    if (list.status === "fulfilled" && cfg.status === "fulfilled") {
      const ids = [...new Set(list.value.filter(id => typeof id === "string" && id.length > 0))];
      setVersions(ids); setCurrent(old => ids.includes(old) ? old : ids[0] ?? "");
      setConfig(cfg.value); setStatus("ready");
    } else {
      setStatus("error"); setError(String(list.status === "rejected" ? list.reason : cfg.status === "rejected" ? cfg.reason : "读取失败"));
    }
  }, []);
  useEffect(() => {
    mounted.current = true;
    void refresh();
    return () => { mounted.current = false; ++loadGeneration.current; listener.current?.(); listener.current = undefined; };
  }, [refresh]);
  function select(id: string) {
    if (launchLock.current || !versions.includes(id)) return;
    setCurrent(id); setPid(null); setLaunchError(""); setProgress(null);
  }
  async function launch() {
    if (!inTauri || status !== "ready" || !current || launchLock.current) return;
    launchLock.current = true;
    setLaunchBusy(true); setLaunchError(""); setPid(null); setProgress(null);
    const version = current;
    let unsubscribe: (() => void) | undefined;
    try {
      unsubscribe = await onProgress((id, update) => { if (mounted.current && id === version) setProgress(update); });
      listener.current = unsubscribe;
      if (!mounted.current) return;
      const processId = await launchVersion(version);
      if (!Number.isInteger(processId) || processId <= 0) throw new Error("未收到有效的进程 ID，请检查启动日志。");
      if (mounted.current) setPid(processId);
    } catch (cause) {
      if (mounted.current) setLaunchError(String(cause));
    } finally {
      unsubscribe?.(); listener.current = undefined; launchLock.current = false;
      if (mounted.current) { setLaunchBusy(false); setProgress(null); }
    }
  }
  return { versions, current, config, status, error, launchError, launchBusy, progress, pid, refresh, select, launch };
}
export type LocalLauncher = ReturnType<typeof useLocalLauncher>;
