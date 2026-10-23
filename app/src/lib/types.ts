export type Tab = "launch" | "download" | "settings" | "more";
export type Theme = "light" | "dark";
export type Accent = "blue" | "green" | "purple" | "orange";
export type Radius = "sharp" | "default" | "soft";
export type Motion = "on" | "off";

export interface Skin {
  theme: Theme;
  accent: Accent;
  radius: Radius;
  motion: Motion;
}

export interface SystemInfo {
  os: string;
  cpuCores: number;
  totalMemGB: number;
  freeMemGB: number;
  /** true 表示运行在浏览器里，数据是假的 */
  demo: boolean;
}

export interface VersionEntry {
  id: string;
  loader: string;
  mods: number;
  hours: number;
  current?: boolean;
}

export interface SaveEntry {
  name: string;
  desc: string;
  when: string;
  size: string;
}

export interface NewsEntry {
  title: string;
  desc: string;
  date: string;
}

export interface StatEntry {
  value: string;
  unit: string;
  label: string;
}

export interface RemoteVersion {
  id: string;
  date: string;
  loaders: string;
  installed?: boolean;
}

/** Mojang 版本清单里的一条 */
export interface VersionSummary {
  id: string;
  /** release | snapshot | old_beta | old_alpha */
  kind: string;
  released: string;
}

export interface LauncherConfig {
  gameDir: string;
  /** official | bmclapi */
  source: string;
  username: string;
}

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

export interface ModFile {
  /** 实际文件名，禁用态带 .disabled */
  fileName: string;
  /** 不带扩展名的主体名 */
  baseName: string;
  /** 从 jar 元数据读出的显示名，读不到时回退文件名 */
  name: string;
  version: string | null;
  loader: string | null;
  enabled: boolean;
  sizeBytes: number;
  /** 暂为 null：图标抽取未做 */
  iconPath: string | null;
  projectId: string | null;
}
