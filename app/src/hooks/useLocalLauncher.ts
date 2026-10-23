import { useCallback, useEffect, useRef, useState } from "react";
import { getApiInfo, inTauri, parseApiError } from "../lib/bridge";
import type { ApiInfo } from "../lib/types";
import { useLegacyLauncher } from "./useLegacyLauncher";
import { useInstances } from "./useInstances";

type Mode = "checking" | "browser" | "legacy" | "core" | "error";
const NO_FEATURES: string[] = [];

export function useLocalLauncher() {
  const [mode, setMode] = useState<Mode>(inTauri ? "checking" : "browser");
  const [api, setApi] = useState<ApiInfo | null>(null);
  const [error, setError] = useState("");
  const generation = useRef(0);
  const detect = useCallback(async () => {
    if (!inTauri) return;
    const ticket = ++generation.current;
    setMode("checking"); setError("");
    try {
      const info = await getApiInfo();
      if (ticket !== generation.current) return;
      setApi(info);
      setMode(info.apiVersion >= 3 && info.features.includes("instances") ? "core" : "legacy");
    } catch (cause) {
      if (ticket !== generation.current) return;
      const failure = parseApiError(cause);
      // An old core has no capability command. Other failures remain visible and retryable.
      if (failure.code === "UNSUPPORTED" || /(?:unknown command|command.*get_api_info.*not found|未找到.*get_api_info)/i.test(failure.message)) {
        setApi(null); setMode("legacy");
      } else { setMode("error"); setError(failure.message); }
    }
  }, []);
  useEffect(() => { void detect(); return () => { ++generation.current; }; }, [detect]);
  const legacy = useLegacyLauncher(mode === "legacy");
  const core = useInstances(mode === "core", api?.features ?? NO_FEATURES);
  const refresh = useCallback(async () => {
    if (mode === "core") await core.refresh();
    else if (mode === "legacy") await legacy.refresh();
    else await detect();
  }, [mode, core.refresh, legacy.refresh, detect]);
  return {
    mode, api, error, core, legacy, refresh,
    config: mode === "core" ? core.config : mode === "legacy" ? legacy.config : null,
    launchBusy: mode === "core" ? core.launchBusy : legacy.launchBusy,
    statusLabel: mode === "browser" ? "浏览器预览 · 未连接" : mode === "checking" ? "正在检测核心能力"
      : mode === "error" ? "核心连接未完成" : mode === "core" ? `桌面模式 · ${core.status === "error" ? "本地数据未能读取" : core.phaseLabel}`
      : legacy.launchBusy ? "正在准备游戏进程" : "桌面模式 · 兼容版本模式",
  };
}
export type LocalLauncher = ReturnType<typeof useLocalLauncher>;
