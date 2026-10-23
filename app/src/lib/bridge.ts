import type { LauncherConfig, SystemInfo, VersionSummary } from "./types";
import type {
  ApiErrorCode, ApiErrorPayload, ApiInfo, CoreConfig, CoreConfigPatch,
  CoreSaveEntry, CreateInstanceRequest, DeleteInstanceOptions, GameExitPayload,
  GameSession, Instance, InstanceChangedPayload, InstancePatch, SessionFilter,
  ModFile, Stats, Task, TaskFilter, TaskState, Unlisten,
} from "./types";

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

/* ---------- 错误 ---------- */

export class ApiError extends Error {
  readonly code: ApiErrorCode | "INTERNAL";
  readonly detail: string | null;
  constructor(code: ApiErrorCode | "INTERNAL", message: string, detail: string | null = null) {
    super(message);
    this.name = "ApiError";
    this.code = code;
    this.detail = detail;
  }
}

const API_ERROR_CODES = new Set<string>([
  "NETWORK", "DISK_FULL", "PERMISSION", "INVALID_NAME", "VERSION_NOT_FOUND",
  "INSTANCE_NOT_FOUND", "INSTANCE_BUSY", "JAVA_MISSING", "VERIFY_FAILED",
  "AUTH_REQUIRED", "AUTH_EXPIRED", "CANCELLED", "BUSY", "UNSUPPORTED", "INTERNAL",
]);

/** `CODE|消息|细节` → ApiError；只有前两个 `|` 是分隔符，detail 里可以再出现 */
export function parseApiError(cause: unknown): ApiError {
  if (cause instanceof ApiError) return cause;
  const raw =
    typeof cause === "string" ? cause
    : cause instanceof Error ? cause.message
    : cause && typeof cause === "object" && "message" in cause
      ? String((cause as { message: unknown }).message)
      : String(cause);
  const i = raw.indexOf("|");
  if (i > 0 && API_ERROR_CODES.has(raw.slice(0, i))) {
    const j = raw.indexOf("|", i + 1);
    const code = raw.slice(0, i) as ApiErrorCode;
    if (j < 0) return new ApiError(code, raw.slice(i + 1), null);
    return new ApiError(code, raw.slice(i + 1, j), raw.slice(j + 1));
  }
  return new ApiError("INTERNAL", raw, null);
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!inTauri) throw new ApiError("UNSUPPORTED", "浏览器预览未连接本地核心");
  const { invoke: inv } = await import("@tauri-apps/api/core");
  try {
    return await inv<T>(cmd, args);
  } catch (cause) {
    throw parseApiError(cause);
  }
}

/* ---------- 任务合并（rev 取大 + 终态粘滞） ---------- */

export function isTerminal(state: TaskState): boolean {
  return state === "done" || state === "error" || state === "cancelled";
}

export function mergeTask(map: Map<string, Task>, next: Task): Map<string, Task> {
  const cur = map.get(next.taskId);
  if (cur && next.revision <= cur.revision) return map;
  if (cur && isTerminal(cur.state) && !isTerminal(next.state)) return map;
  const out = new Map(map);
  out.set(next.taskId, next);
  return out;
}

/* ---------- 能力 ---------- */

export async function getApiInfo(): Promise<ApiInfo> {
  if (!inTauri) throw new ApiError("UNSUPPORTED", "浏览器预览未连接本地核心");
  return call<ApiInfo>("get_api_info");
}

/* ---------- 第一组：首页实例 / 启动 / 会话 ---------- */

export async function listInstances(): Promise<Instance[]> {
  return call<Instance[]>("list_instances");
}

export async function getInstance(id: string): Promise<Instance | null> {
  return call<Instance | null>("get_instance", { id });
}

export async function getActiveInstance(): Promise<Instance | null> {
  return call<Instance | null>("get_active_instance");
}

/** 返回合并后的配置，调用方以它为准更新选中态 */
export async function setActiveInstance(id: string | null): Promise<CoreConfig> {
  return call<CoreConfig>("set_active_instance", { id });
}

export async function startLaunch(instanceId: string): Promise<Task> {
  return call<Task>("start_launch", { instanceId });
}

export async function getActiveSession(instanceId?: string): Promise<GameSession | null> {
  return call<GameSession | null>("get_active_session", { instanceId });
}

export async function listSessions(filter: SessionFilter = {}): Promise<GameSession[]> {
  return call<GameSession[]>("list_sessions", {
    instanceId: filter.instanceId,
    limit: filter.limit,
  });
}

export async function onTask(cb: (task: Task) => void): Promise<Unlisten> {
  if (!inTauri) throw new ApiError("UNSUPPORTED", "浏览器预览未连接本地核心");
  const { listen } = await import("@tauri-apps/api/event");
  return listen<Task>("astral://task", (e) => cb(e.payload));
}

export async function onGameExit(cb: (event: GameExitPayload) => void): Promise<Unlisten> {
  if (!inTauri) throw new ApiError("UNSUPPORTED", "浏览器预览未连接本地核心");
  const { listen } = await import("@tauri-apps/api/event");
  return listen<GameExitPayload>("astral://game-exit", (e) => cb(e.payload));
}

export async function onInstanceChanged(
  cb: (event: InstanceChangedPayload) => void
): Promise<Unlisten> {
  if (!inTauri) throw new ApiError("UNSUPPORTED", "浏览器预览未连接本地核心");
  const { listen } = await import("@tauri-apps/api/event");
  return listen<InstanceChangedPayload>("astral://instance-changed", (e) => cb(e.payload));
}

/* ---------- 第二组：任务 / 实例管理 ---------- */

/** 注意：Rust 侧是顶层三个参数，不是 { filter } */
export async function getTasks(filter: TaskFilter = {}): Promise<Task[]> {
  return call<Task[]>("get_tasks", {
    state: filter.state,
    kind: filter.kind,
    instanceId: filter.instanceId,
  });
}

export async function getTask(taskId: string): Promise<Task | null> {
  return call<Task | null>("get_task", { taskId });
}

export async function startInstall(gameVersion: string): Promise<Task> {
  return call<Task>("start_install", { gameVersion });
}

export async function startCreateInstance(request: CreateInstanceRequest): Promise<Task> {
  return call<Task>("start_create_instance", { request });
}

export async function updateInstance(id: string, patch: InstancePatch): Promise<Instance> {
  return call<Instance>("update_instance", { id, patch });
}

export async function deleteInstance(
  id: string,
  opts: DeleteInstanceOptions = { trash: true }
): Promise<CoreConfig> {
  return call<CoreConfig>("delete_instance", { id, opts });
}

/* ---------- 第四组：存档 / 统计 / 配置 / 目录 ---------- */

/** 必须传实例 ID；无效 ID 现在会报 INSTANCE_NOT_FOUND，不再回落到全局目录 */
export async function listSaves(instanceId: string): Promise<CoreSaveEntry[]> {
  if (!instanceId) throw new ApiError("INVALID_NAME", "必须指定实例 ID");
  return call<CoreSaveEntry[]>("list_saves", { instanceId });
}

export async function getStats(range: "7d" | "30d" | "all" = "7d"): Promise<Stats> {
  return call<Stats>("get_stats", { range });
}

export async function getCoreConfig(): Promise<CoreConfig> {
  if (!inTauri) throw new ApiError("UNSUPPORTED", "浏览器预览未连接本地核心");
  return call<CoreConfig>("get_config");
}

/** 只提交本次编辑的字段（source / username）；参数名必须是 config */
export async function patchConfig(patch: CoreConfigPatch): Promise<CoreConfig> {
  return call<CoreConfig>("set_config", { config: patch });
}

/** 与旧的 openPath 不同：失败会抛错，不会静默 */
export async function openPathChecked(path: string): Promise<void> {
  return call<void>("open_path", { path });
}

/* ---------- 第五组：模组（仅托管实例） ---------- */

export async function listMods(instanceId: string): Promise<ModFile[]> {
  return call<ModFile[]>("list_mods", { instanceId });
}

/** 启停会改文件名；以返回的 fileName 为准，不要在界面猜后缀 */
export async function setModEnabled(
  instanceId: string,
  fileName: string,
  enabled: boolean
): Promise<ModFile> {
  return call<ModFile>("set_mod_enabled", { instanceId, fileName, enabled });
}

/** 直接删除，不进回收站 */
export async function deleteMod(instanceId: string, fileName: string): Promise<void> {
  return call<void>("delete_mod", { instanceId, fileName });
}
