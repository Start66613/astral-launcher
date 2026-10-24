import {
  ApiError,
  getTask,
  inTauri,
  isTerminal,
  mergeTask,
  onTask,
  parseApiError,
} from "../lib/bridge";
import type { Task } from "../lib/types";

/** UI-owned adapter. Existing bridge/types and Rust remain unchanged. */
export async function invokeLive<T>(
  command: string,
  args: Record<string, unknown> = {},
): Promise<T> {
  if (!inTauri)
    throw new ApiError("UNSUPPORTED", "浏览器预览 · 未连接本地核心");
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke<T>(command, args);
  } catch (cause) {
    throw parseApiError(cause);
  }
}
export type LoaderCatalog = {
  kind: string;
  label: string;
  recommended: string | null;
  versions: string[];
};
export const listLoaders = (gameVersion: string) =>
  invokeLive<LoaderCatalog[]>("list_loaders", { gameVersion });
export const installLoader = (
  instanceId: string,
  kind: string,
  version: string,
) => invokeLive<Task>("start_install_loader", { instanceId, kind, version });

// Incremental core fields; keep the coordinated shared DTO untouched.
export function hasCooperativeCancellation(
  task: Task,
): task is Task & { bootId: string; cancelling: boolean } {
  const snapshot = task as Task & { bootId?: unknown; cancelling?: unknown };
  return (
    typeof snapshot.bootId === "string" &&
    snapshot.bootId.length > 0 &&
    typeof snapshot.cancelling === "boolean"
  );
}

export const requestTaskCancellation = (taskId: string) =>
  invokeLive<{ accepted: boolean }>("cancel_task", { taskId });

/** Subscribe first, then reconcile. Timers query real state; never advance progress. */
export async function waitForTask(
  task: Task,
  update: (value: Task) => void,
): Promise<Task> {
  let map = mergeTask(new Map(), task);
  let finish!: (task: Task) => void;
  let fail!: (cause: unknown) => void;
  const completed = new Promise<Task>((resolve, reject) => {
    finish = resolve;
    fail = reject;
  });
  let stopped = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const accept = (incoming: Task) => {
    if (incoming.taskId !== task.taskId || stopped) return;
    map = mergeTask(map, incoming);
    const current = map.get(task.taskId)!;
    update(current);
    if (isTerminal(current.state)) {
      stopped = true;
      if (timer) clearTimeout(timer);
      finish(current);
    }
  };
  const unlisten = await onTask(accept);
  async function reconcile() {
    if (stopped) return;
    try {
      const current = await getTask(task.taskId);
      if (!current)
        throw new Error(
          "任务不在当前核心进程中；请查看保留的任务记录，后台操作可能仍在进行。",
        );
      accept(current);
    } catch (cause) {
      stopped = true;
      fail(cause);
    }
    if (!stopped) timer = setTimeout(() => void reconcile(), 1500);
  }
  accept(task);
  void reconcile();
  try {
    const final = await completed;
    if (final.state !== "done")
      throw new Error(
        final.error?.message ||
          `任务${final.state === "cancelled" ? "已取消" : "未完成"}`,
      );
    return final;
  } finally {
    stopped = true;
    if (timer) clearTimeout(timer);
    unlisten();
  }
}

export async function publicJson<T>(
  url: string,
  signal?: AbortSignal,
): Promise<T> {
  const response = await fetch(url, {
    signal,
    headers: { Accept: "application/json" },
  });
  if (!response.ok) throw new Error(`读取清单失败：HTTP ${response.status}`);
  return response.json();
}
export const MODRINTH = "https://api.modrinth.com/v2";
export type Project = {
  id: string;
  slug: string;
  title: string;
  description: string;
  downloads: number;
  updated: string;
  team?: string;
  icon_url?: string;
  license?: { id: string };
  project_type?: string;
};
export type ProjectVersion = {
  id: string;
  name: string;
  version_number: string;
  version_type: string;
  featured: boolean;
  date_published: string;
  loaders: string[];
  game_versions: string[];
  files: { filename: string; size: number; primary: boolean }[];
  dependencies: {
    project_id: string | null;
    version_id: string | null;
    dependency_type: string;
  }[];
};
