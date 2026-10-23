# 共享桥接层增量（核心 v3 · 第一批）

> 给 `app/src/lib/types.ts` 与 `app/src/lib/bridge.ts` 的**新增代码块**。
> 原则：只新增，不改现有导出的签名与语义。Codex 直接粘贴，不需要 DSH 改这两个文件。

最后更新：2026-10-23 · 对应核心提交 `feature/core-features`

---

## 0. 怎么用

1. 把第 1 节整体追加到 `types.ts` 末尾（**不要替换**已有的 `SaveEntry` / `LauncherConfig`，那是设计示例类型）。
2. 把第 2 节整体追加到 `bridge.ts` 末尾。
3. 页面里按第 3 节的顺序接：先 `getApiInfo` 判断能力，再查询、再订阅。

本批覆盖你提的第 1、2、4 组；第 3 组（下载中心的任务订阅）只到只读部分，取消/重试仍未开放；第 5 组等核心实现。

---

## 1. `types.ts` 增量

```ts
/* ===== 核心 v3（新增块，勿与现有设计类型混用） ===== */

export type ApiErrorCode =
  | "NETWORK" | "DISK_FULL" | "PERMISSION" | "INVALID_NAME"
  | "VERSION_NOT_FOUND" | "INSTANCE_NOT_FOUND" | "INSTANCE_BUSY"
  | "JAVA_MISSING" | "VERIFY_FAILED" | "AUTH_REQUIRED" | "AUTH_EXPIRED"
  | "CANCELLED" | "BUSY" | "UNSUPPORTED" | "INTERNAL";

/** Task.error 的形态（JSON 对象，不是 Error 实例） */
export interface ApiErrorPayload {
  code: string;
  message: string;
  detail: string | null;
}

export interface ApiInfo {
  apiVersion: number;
  appVersion: string;
  /** 实际注册的能力，不是愿望清单 */
  features: string[];
}

export interface CoreConfig {
  gameDir: string;
  source: "official" | "bmclapi";
  username: string;
  activeInstanceId: string | null;
}
export type CoreConfigPatch = Partial<Pick<CoreConfig, "source" | "username">>;

export type LoaderKind = "vanilla" | "fabric" | "forge" | "neoforge" | "quilt";

export interface InstanceHealth {
  ok: boolean;
  /** ok=false 时至少一条，可直接展示，例：「缺少客户端 1.20.1.jar」 */
  reasons: string[];
}

export interface InstanceDirs {
  mods: string;
  config: string;
  saves: string;
  logs: string;
  resourcepacks: string;
  shaderpacks: string;
  screenshots: string;
}

export interface LoaderRef {
  kind: LoaderKind;
  version: string;
}

export interface Instance {
  /** 稳定 id（inst_xxxxxxxx 或 ver_<版本>），与目录名解耦 */
  id: string;
  name: string;
  gameVersion: string;
  loader: LoaderRef | null;
  /** 实例根目录；兼容实例为游戏根目录 */
  dir: string;
  dirs: InstanceDirs;
  modsCount: number;
  modsDisabled: number;
  cover: string | null;
  health: InstanceHealth;
  lastPlayed: string | null;
  totalPlaytimeMs: number;
  memoryMb: number | null;
  jvmArgs: string[];
  javaPath: string | null;
  createdAt: string;
  /** 核心实际多给的字段：true 才是 instances/<id> 托管实例 */
  managed: boolean;
}

export interface InstancePatch {
  name?: string;
  memoryMb?: number | null;
  jvmArgs?: string[];
  javaPath?: string | null;
  cover?: string | null;
}

export type TaskKind =
  | "install" | "launch" | "loader" | "modpack"
  | "download" | "diagnose" | "create" | "duplicate";

export type TaskState = "queued" | "running" | "done" | "error" | "cancelled";

export type TaskResult =
  | { kind: "install"; gameVersion: string }
  | { kind: "create"; instance: Instance }
  | { kind: "duplicate"; instance: Instance }
  | { kind: "launch"; sessionId: string; pid: number; startedAt: string }
  | { kind: "loader"; loader: LoaderRef }
  | { kind: "modpack"; instance: Instance }
  | { kind: "download"; path: string; bytes: number; replaced: boolean }
  | { kind: "diagnose"; report: unknown };

export interface Task {
  taskId: string;
  /** 单调递增；合并时取大 */
  revision: number;
  kind: TaskKind;
  instanceId: string | null;
  gameVersion: string | null;
  title: string;
  /** 核心给的阶段值，不要固化成 UI 枚举 */
  stage: string;
  label: string;
  done: number;
  total: number;
  bytes: number;
  totalBytes: number;
  state: TaskState;
  error: ApiErrorPayload | null;
  result: TaskResult | null;
  startedAt: string;
  endedAt: string | null;
  retryOf: string | null;
}

export interface GameSession {
  sessionId: string;
  instanceId: string;
  taskId: string;
  pid: number;
  startedAt: string;
  state: "starting" | "running" | "exited";
  exitCode: number | null;
  endedAt: string | null;
  durationMs: number | null;
}

/** 核心的存档条目（与现有设计示例 SaveEntry 不同名） */
export interface CoreSaveEntry {
  name: string;
  dir: string;
  gameVersion: string | null;
  lastModified: string;
  sizeBytes: number;
  iconPath: string | null;
}

export interface Stats {
  range: "7d" | "30d" | "all";
  playtimeMs: number;
  sessions: number;
  /** 无数据时为 null，界面应隐藏而不是显示 0 */
  deaths: number | null;
  achievements: number | null;
  crashes: number;
  byInstance: { instanceId: string; name: string; playtimeMs: number }[];
  updatedAt: string;
}

export interface GameExitPayload {
  sessionId: string;
  taskId: string | null;
  instanceId: string;
  exitCode: number | null;
  crashed: boolean;
  durationMs: number;
  endedAt: string;
}

export interface InstanceChangedPayload {
  instanceId: string;
  change: "created" | "updated" | "deleted";
}

export interface TaskFilter {
  state?: TaskState;
  kind?: TaskKind;
  instanceId?: string;
}

export interface SessionFilter {
  instanceId?: string;
  limit?: number;
}

export interface CreateInstanceRequest {
  name: string;
  gameVersion: string;
  loaderKind?: LoaderKind;
  loaderVersion?: string;
  memoryMb?: number;
}

export interface DeleteInstanceOptions {
  /** 默认 true：移入 instances/.trash */
  trash?: boolean;
}

export type Unlisten = () => void;
```

---

## 2. `bridge.ts` 增量

```ts
import type {
  ApiErrorCode, ApiErrorPayload, ApiInfo, CoreConfig, CoreConfigPatch,
  CoreSaveEntry, CreateInstanceRequest, DeleteInstanceOptions, GameExitPayload,
  GameSession, Instance, InstanceChangedPayload, InstancePatch, SessionFilter,
  Stats, Task, TaskFilter, TaskState, Unlisten,
} from "./types";

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
  if (!inTauri) return { apiVersion: 0, appVersion: "browser", features: [] };
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
  if (!inTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<Task>("astral://task", (e) => cb(e.payload));
}

export async function onGameExit(cb: (event: GameExitPayload) => void): Promise<Unlisten> {
  if (!inTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<GameExitPayload>("astral://game-exit", (e) => cb(e.payload));
}

export async function onInstanceChanged(
  cb: (event: InstanceChangedPayload) => void
): Promise<Unlisten> {
  if (!inTauri) return () => {};
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
  if (!inTauri) {
    return {
      gameDir: "", source: "official", username: "Player", activeInstanceId: null,
    };
  }
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
```

---

## 3. 三个必须照做的顺序

### 3.1 启动三态

```ts
// 1) 先订阅，再取快照（顺序反了会漏帧）
const unTask = await onTask((t) => (tasks = mergeTask(tasks, t)));
const unExit = await onGameExit((e) => handleExit(e));
const snapshot = await getTasks({ kind: "launch" });
snapshot.forEach((t) => (tasks = mergeTask(tasks, t)));

// 2) 启动
const task = await startLaunch(instanceId);        // 立刻有 taskId
// queued/running → 启动中；done 只代表进程已创建

// 3) 确认运行态
const session = await getActiveSession(instanceId); // state === "running" 才是运行中
// 4) 结束
//    astral://game-exit 或 session.state === "exited" → 已退出
```

要点：

- `Task.state === "done"` **不等于**在运行，必须再看 `getActiveSession`。
- 退出事件要同时校验 `instanceId` 和 `sessionId`，迟到事件不能清掉新会话。
- `exitCode === null` 显示「已结束，退出码未知」，不要当正常退出。
- 刷新后从「恢复查询中」开始：重建监听 → `getTasks` → `getActiveInstance` → `getActiveSession`。
- 查询到 `null` 再去 `listSessions({ instanceId, limit: 1 })`，有已结束记录才显示「上次已退出」。

### 3.2 实例归属

```ts
// 只有 managed === true 才开放改名 / 删除 / 实例内写模组
const canManage = (i: Instance) => i.managed === true;
```

兼容实例（`ver_<版本>`）的 `dirs.*` 指向**共享游戏目录**，卡片上标「兼容版本 · 共享游戏目录」。

### 3.3 health 是"基础文件检查"，不是"可玩"

`ok=true` 只说明版本 json / jar / assets 在。展示「基础文件检查通过」，不要写「所有依赖已就绪」。
`ok=false` 展示 `reasons[0]`，展开看完整列表，按钮引导去下载中心补全——**不要**在前端把 `ok` 置 true。

---

## 4. 本轮已修正的差异（你上轮点出来的）

| 你提出的问题 | 处理 |
|---|---|
| `list_saves` 无效 ID 回落全局 | 改为返回 `INSTANCE_NOT_FOUND`，不再回落 |
| `sessionId` 重启后重复 | 改为 `sess_<毫秒时间戳>_<序号>`，跨进程唯一 |
| `features` 含 `mods` 但命令缺失 | 已移除 `mods`，现在只列真实注册的能力；`apiVersion` 仍为 3 |
| `cancel_task` 只改注册表、后续进度会覆盖终态 | `Task.update` 加了终态粘滞：`done/error/cancelled` 之后忽略任何修改。但**后台 worker 仍未协作停止**，取消按钮继续保持关闭 |
| `start_install` / `start_launch` 无正式签名 | 见下方 §5，已补进契约 v3.1 |
| `get_tasks` 实际是顶层三参数 | 桥接里已按顶层字段传，并在注释标注 |
| `managed` 字段契约没有 | 已补进契约 v3.1，类型里标了核心实际字段 |
| 实例 `memoryMb` / `jvmArgs` 没生效 | **已修**：给 vendored `portablemc` 加了 `set_extra_jvm_args`，`start_launch` 会把 `-Xmx{mem}M` 与自定义参数拼进 JVM 命令。`javaPath` 仍未生效，先别开放 |
| `astral://instance-changed` 不发射 | **已修**：`update_instance` / `delete_instance` / `start_create_instance` 成功后推送 |
| 创建实例没有公开命令 | **已加** `start_create_instance`，返回 Task，`result.kind === "create"` 带完整 `Instance` |

## 5. 契约 v3.1 的增量（同步写进了 `docs/api-contract.md`）

```ts
// 安装
start_install({ gameVersion: string }): Task   // result: { kind: "install", gameVersion }

// 启动
start_launch({ instanceId: string }): Task     // result: { kind: "launch", sessionId, pid, startedAt }

// 任务查询（顶层字段，非 filter 对象）
get_tasks({ state?: TaskState, kind?: TaskKind, instanceId?: string }): Task[]

// 实例新增字段
Instance.managed: boolean
```

## 6. 仍然不可用（别写入口）

| 能力 | 原因 |
|---|---|
| `cancel_task` | 后台 worker 没有停止检查；`accepted` 只是受理 |
| `retry_task` | 未注册；不复用 `start_*` 冒充 |
| `start_duplicate_instance` | 未实现 |
| `list_loaders` / `start_install_loader` | 下一批 |
| `list_mods` / `set_mod_enabled` / `delete_mod` | 下一批 |
| `start_download`（实例级） | 现在只有 legacy `download_to`，写共享目录 |
| `start_install_modpack` / `export_modpack` | 下一批 |
| `get_account` / MSA | 下一批 |
| `prepare_diagnosis` / `start_diagnose` | 下一批 |
| `javaPath` 生效 | 已存字段但启动未消费 |
