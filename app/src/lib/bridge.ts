import type { LauncherConfig, SystemInfo, VersionSummary } from "./types";

/** 运行在 Tauri 里还是普通浏览器里 */
export const inTauri: boolean =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export interface Progress {
  stage: string;
  label: string;
  done: number;
  total: number;
  bytes: number;
  totalBytes: number;
}

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke: call } = await import("@tauri-apps/api/core");
  return call<T>(cmd, args);
}

export async function getSystemInfo(): Promise<SystemInfo> {
  if (!inTauri) {
    return {
      os: "Windows 11（浏览器预览）",
      cpuCores: 8,
      totalMemGB: 16,
      freeMemGB: 9.2,
      demo: true,
    };
  }
  try {
    return await invoke<SystemInfo>("get_system_info");
  } catch {
    return { os: "Windows", cpuCores: 0, totalMemGB: 0, freeMemGB: 0, demo: true };
  }
}

/** 拉取 Mojang 官方版本清单（走 Rust，本地有缓存） */
export async function listVersions(): Promise<VersionSummary[]> {
  if (!inTauri) return [];
  return invoke<VersionSummary[]>("list_versions");
}

/** 扫描本地已安装的版本 */
export async function scanVersions(): Promise<string[]> {
  if (!inTauri) return [];
  try {
    return await invoke<string[]>("scan_versions");
  } catch {
    return [];
  }
}

export async function installVersion(version: string): Promise<void> {
  if (!inTauri) throw new Error("安装需要在桌面端运行（pnpm tauri dev）");
  await invoke("install_version", { version });
}

export async function launchVersion(version: string): Promise<number> {
  if (!inTauri) throw new Error("启动需要在桌面端运行（pnpm tauri dev）");
  return invoke<number>("launch_version", { version });
}

export async function getConfig(): Promise<LauncherConfig> {
  if (!inTauri) {
    return { gameDir: "C:\\Users\\你\\AppData\\Roaming\\.minecraft", source: "official", username: "Player" };
  }
  return invoke<LauncherConfig>("get_config");
}

export async function setConfig(config: LauncherConfig): Promise<void> {
  if (!inTauri) return;
  await invoke("set_config", { config });
}

export type ResourceKind = "mod" | "modpack" | "resourcepack" | "shaderpack" | "datapack";

/** 下载一个文件到对应的游戏子目录，返回落地路径 */
export async function downloadTo(
  url: string,
  kind: ResourceKind,
  filename: string,
  onPct?: (pct: number) => void
): Promise<string> {
  if (!inTauri) {
    onPct?.(100);
    await new Promise((r) => setTimeout(r, 600));
    return filename;
  }
  const { listen } = await import("@tauri-apps/api/event");
  const un = await listen<[string, number, number, number, number]>(
    "astral://file",
    (e) => {
      const [, done, total] = e.payload;
      if (total > 0 && e.payload[0] === filename) onPct?.(Math.round((done / total) * 100));
    }
  );
  try {
    return await invoke<string>("download_to", { url, kind, filename });
  } finally {
    un();
  }
}

/** 订阅安装/启动进度，返回取消订阅函数 */
export async function onProgress(
  cb: (version: string, p: Progress) => void
): Promise<() => void> {
  if (!inTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<[string, Progress]>("astral://progress", (e) => {
    const [version, p] = e.payload;
    cb(version, p);
  });
}

export async function openPath(path: string): Promise<void> {
  if (!inTauri) return;
  try {
    await invoke("open_path", { path });
  } catch {
    /* W2 还不做错误提示 */
  }
}

export async function minimizeWindow(): Promise<void> {
  if (!inTauri) return;
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  await getCurrentWindow().minimize();
}

export async function toggleMaximize(): Promise<void> {
  if (!inTauri) return;
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  await getCurrentWindow().toggleMaximize();
}

export async function closeWindow(): Promise<void> {
  if (!inTauri) return;
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  await getCurrentWindow().close();
}
