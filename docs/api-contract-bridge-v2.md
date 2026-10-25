# 共享桥接层增量（V2 · 第二批）

> 给 `app/src/lib/types.ts` 与 `app/src/lib/bridge.ts` 的新增代码块。
> 承接 `docs/api-contract-bridge.md`（v3.2 那批），**只新增，不改旧签名**。
>
> 对应后端：`feature/astral-v2-core`，命令 54 个，`--selftest` 81 项。
> 所有参数名按 Tauri v2 约定：Rust 的 `snake_case` 在 JS 侧是 `camelCase`。

---

## 1. `types.ts` 追加

```ts
/* ===== V2：安装计划 ===== */

export interface LoaderSelection {
  kind: "fabric" | "quilt" | "forge" | "neoforge";
  version: string;
}

export interface PlanIssue {
  code: string;
  message: string;
  blocking: boolean;
}

export interface PlannedFile {
  id: string;
  displayName: string;
  sizeBytes: number | null;
  role: "required" | "optional";
  action: "add" | "reuse" | "replace";
  relativePath: string;
}

export interface GameInstallRequest {
  name: string;
  gameVersion: string;
  loader?: LoaderSelection | null;
  memoryMb?: number;
}

export interface GameInstallPlan {
  planId: string;
  revision: number;
  expiresAt: string;
  request: GameInstallRequest;
  target: { proposedInstanceId: string; displayPath: string; isolated: boolean };
  files: PlannedFile[];
  issues: PlanIssue[];
  /** 未知总量为 null，不是 0 */
  downloadBytes: number | null;
}

/* ===== V2：加载器构建 ===== */

export interface LoaderBuild {
  id: string;
  kind: "fabric" | "quilt" | "forge" | "neoforge";
  version: string;
  mcVersion: string;
  stability: "stable" | "beta" | "alpha" | "unknown";
  recommended: boolean;
  publishedAt: string | null;
  available: boolean;
  unavailableReason: string | null;
}

/* ===== V2：世界目标 ===== */

export interface WorldTarget {
  worldId: string;
  displayName: string;
  dir: string;
  lastModified: string;
  sizeBytes: number;
  gameVersion: string | null;
}

/* ===== V2：资源 ===== */

export interface ResourceDependency {
  projectId: string | null;
  versionId: string | null;
  kind: string; // required | optional | incompatible | embedded
}

export interface ResourceProject {
  id: string;
  title: string;
  description: string;
  iconUrl: string | null;
  downloads: number;
  updatedAt: string | null;
  sourceUrl: string | null;
  licenseId: string | null;
}

export interface ResourceFile {
  versionId: string;
  name: string;
  versionNumber: string;
  channel: "release" | "beta" | "alpha";
  publishedAt: string;
  gameVersions: string[];
  loaders: string[];
  fileName: string;
  url: string;
  size: number;
  sha1: string | null;
  sha512: string | null;
  dependencies: ResourceDependency[];
}

export interface PlannedDep {
  projectId: string;
  versionId: string;
  fileName: string;
  size: number;
  sha1: string | null;
  sha512: string | null;
}

export interface ResourceInstallRequest {
  instanceId: string;
  kind: "mod" | "resourcepack" | "shaderpack" | "datapack";
  projectId: string;
  versionId: string;
  /** datapack 必填，来自 listTargetWorlds 的 worldId */
  worldId?: string;
  /** 默认 false：同名文件会被拒绝 */
  overwrite?: boolean;
}

export interface ResourcePlan {
  planId: string;
  revision: number;
  expiresAt: string;
  kind: string;
  projectId: string;
  versionId: string;
  fileName: string;
  targetDir: string;
  downloadBytes: number;
  replaces: string | null;
  issues: PlanIssue[];
  /** 会被先安装的必需依赖 */
  dependencies: PlannedDep[];
}

/* ===== V2：模组更新 ===== */

export interface ModUpdate {
  fileName: string;
  projectId: string;
  currentVersion: string;
  latestVersion: string;
  latestVersionId: string;
  latestFileName: string;
}

export interface UpdateReport {
  checked: number;
  unknown: number;
  updates: ModUpdate[];
  skipped: number;
}

/* ===== V2：整合包 ===== */

export interface ModpackInfo {
  name: string;
  summary: string;
  versionName: string;
  mcVersion: string;
  loaderKind: string | null;
  loaderVersion: string | null;
  fileCount: number;
  totalBytes: number;
}

export interface ModpackPlan {
  planId: string;
  revision: number;
  expiresAt: string;
  info: ModpackInfo;
  issues: PlanIssue[];
  downloadBytes: number;
}

export interface ModpackRequest {
  projectId: string;
  versionId: string;
  name?: string;
  memoryMb?: number;
}

export interface ExportResult {
  path: string;
  indexedFiles: number;
  overrideFiles: number;
  bytes: number;
}

/* ===== V2：光影组件 / Java ===== */

export interface ComponentBuild {
  id: string;
  label: string;
  projectId: string;
  requiredLoader: string | null;
  version: string | null;
  fileName: string | null;
  publishedAt: string | null;
  stability: "release" | "beta" | "alpha" | "unknown";
  recommended: boolean;
  available: boolean;
  unavailableReason: string | null;
}

export interface ComponentCatalog {
  shaderSupport: ComponentBuild[];
  fetchedAt: string;
}

export interface JavaRuntime {
  path: string;
  version: string;
  major: number;
  vendor: string;
  bits: number | null;
  source: "java_home" | "game_jvm" | "common_dir" | "path";
}

export interface JavaReport {
  runtimes: JavaRuntime[];
  requiredMajor: number | null;
  recommended: string | null;
}

/* ===== V2：光影组件版本 ===== */

export interface ComponentVersion {
  versionId: string;
  versionNumber: string;
  channel: "release" | "beta" | "alpha";
  gameVersions: string[];
  loaders: string[];
  fileName: string;
  size: number;
  publishedAt: string;
  recommended: boolean;
}
/* ===== V2：JDK 管理 ===== */

export interface ManagedJdk {
  major: number;
  dir: string;
  bytes: number;
  javaPath: string | null;
}

/* ===== V2：Task / Session 字段增补 ===== */

/** Task 在 v3 基础上新增： */
export interface TaskV2Fields {
  /** 产出该任务的进程启动标识；重启后旧任务会变成 INTERRUPTED */
  bootId: string;
  /** 已请求取消，等 worker 停下；此时还不能显示"已取消" */
  cancelling: boolean;
  /** 稳定阶段枚举，驱动启动动画 */
  phase: "queued" | "validating" | "checking-files" | "preparing-java"
       | "downloading" | "launching" | "process-created"
       | "done" | "failed" | "cancelled";
  /** 阶段中文名，直接显示，不要自己映射 */
  stageLabel: string;
  /** 发起参数，用于 retry_task；结构不承诺稳定，界面只读不看 */
  retrySpec: unknown | null;
}

/** GameSession 新增：启动器重启后按 pid 对账收尾，退出码与时长不精确 */
export interface GameSessionV2Fields {
  reconciled: boolean;
}

/** astral://game-exit 载荷新增 */
export interface GameExitPayloadV2Fields {
  /** 是否拿到了退出码；false 时不要显示成"崩溃" */
  exitKnown: boolean;
  /** 只有 exitKnown 且退出码非 0 才为 true */
  crashed: boolean;
  reconciled: boolean;
}

/* ===== V2：日志 / 账号 ===== */

export interface LogFile {
  id: string;
  name: string;
  kind: "game" | "crash";
  sizeBytes: number;
  modifiedAt: string;
}

export interface LogText {
  id: string;
  name: string;
  text: string;
  lines: number;
  truncated: boolean;
}

export interface DiagnosisPreview {
  text: string;
  redactions: number;
  bytes: number;
  truncated: boolean;
  warnings: string[];
}

export interface MsaStatus {
  configured: boolean;
  clientIdMasked: string | null;
  reason: string | null;
  steps: string[];
}

export interface AccountView {
  kind: "offline";
  name: string;
  online: boolean;
  note: string | null;
}
```

## 2. `bridge.ts` 追加

```ts
/* ---------- 安装计划 ---------- */

export async function prepareGameInstall(
  request: GameInstallRequest
): Promise<GameInstallPlan> {
  return call<GameInstallPlan>("prepare_game_install", { request });
}

/** 计划 15 分钟过期、一次性；blocking issue 会直接抛错 */
export async function startGameInstall(planId: string, revision: number): Promise<Task> {
  return call<Task>("start_game_install", { planId, revision });
}

/* ---------- 加载器 ---------- */

export async function listLoaderBuilds(
  gameVersion: string,
  kind: LoaderBuild["kind"]
): Promise<LoaderBuild[]> {
  return call<LoaderBuild[]>("list_loader_builds", { gameVersion, kind });
}

/* ---------- 世界目标 ---------- */

export async function listTargetWorlds(instanceId: string): Promise<WorldTarget[]> {
  return call<WorldTarget[]>("list_target_worlds", { instanceId });
}

/* ---------- 资源 ---------- */

export async function getResourceProject(projectId: string): Promise<ResourceProject> {
  return call<ResourceProject>("get_resource_project", { projectId });
}

/** mod / datapack 建议传 gameVersion + loader；资源包与光影包不要传版本 */
export async function listResourceFiles(
  projectId: string,
  gameVersion?: string,
  loader?: string
): Promise<ResourceFile[]> {
  return call<ResourceFile[]>("list_resource_files", { projectId, gameVersion, loader });
}

export async function prepareResourceInstall(
  request: ResourceInstallRequest
): Promise<ResourcePlan> {
  return call<ResourcePlan>("prepare_resource_install", { request });
}

export async function startResourceInstall(planId: string, revision: number): Promise<Task> {
  return call<Task>("start_resource_install", { planId, revision });
}

/* ---------- 模组更新 ---------- */

export async function checkModUpdates(
  instanceId: string,
  limit?: number
): Promise<UpdateReport> {
  return call<UpdateReport>("check_mod_updates", { instanceId, limit });
}

/** 先下新文件、校验通过才删旧文件 */
export async function startModUpdate(
  instanceId: string,
  fileName: string,
  projectId: string,
  versionId: string
): Promise<Task> {
  return call<Task>("start_mod_update", { instanceId, fileName, projectId, versionId });
}

/* ---------- 整合包 ---------- */

export async function prepareModpackInstall(
  request: ModpackRequest
): Promise<ModpackPlan> {
  return call<ModpackPlan>("prepare_modpack_install", { request });
}

export async function startModpackInstall(planId: string, revision: number): Promise<Task> {
  return call<Task>("start_modpack_install", { planId, revision });
}

export async function exportModpack(
  instanceId: string,
  name: string,
  version?: string,
  includeConfig?: boolean
): Promise<ExportResult> {
  return call<ExportResult>("export_modpack", {
    request: { instanceId, name, version, includeConfig },
  });
}

/* ---------- 光影组件 ---------- */

/** 列出某个组件的全部兼容版本（供玩家自选；第一个是推荐版本） */
export async function listComponentVersions(
  id: string,
  gameVersion: string,
  loader?: string
): Promise<ComponentVersion[]> {
  return call<ComponentVersion[]>("list_component_versions", { id, gameVersion, loader });
}
/* ---------- 任务重试 ---------- */

/** 只有失败/已取消的任务能重试；计划类任务会返回 UNSUPPORTED */
export async function retryTask(taskId: string): Promise<Task> {
  return call<Task>("retry_task", { taskId });
}

/* ---------- JDK ---------- */

/** 下载指定主版本的 Temurin JDK（约 180–200 MB），完成后返回 java.exe 路径 */
export async function installJava(major: number): Promise<Task> {
  return call<Task>("install_java", { major });
}

/** 本项目下载过的 JDK（只读） */
export async function listManagedJdks(): Promise<ManagedJdk[]> {
  return call<ManagedJdk[]>("list_managed_jdks", {});
}

/* ---------- 组件 / Java / 日志 / 账号 ---------- */

export async function getComponentCatalog(
  gameVersion: string,
  loader?: string
): Promise<ComponentCatalog> {
  return call<ComponentCatalog>("get_component_catalog", { gameVersion, loader });
}

export async function listJavaRuntimes(instanceId?: string): Promise<JavaReport> {
  return call<JavaReport>("list_java_runtimes", { instanceId });
}

export async function listLogs(instanceId: string): Promise<LogFile[]> {
  return call<LogFile[]>("list_logs", { instanceId });
}

export async function readLog(
  instanceId: string,
  logId: string,
  limit?: number
): Promise<LogText> {
  return call<LogText>("read_log", { instanceId, logId, limit });
}

/** 只在本机脱敏生成预览，不上传 */
export async function prepareDiagnosis(
  instanceId: string,
  logIds: string[],
  maxLines?: number
): Promise<DiagnosisPreview> {
  return call<DiagnosisPreview>("prepare_diagnosis", { instanceId, logIds, maxLines });
}

export async function getMsaStatus(): Promise<MsaStatus> {
  return call<MsaStatus>("msa_status", {});
}

export async function getAccount(): Promise<AccountView> {
  return call<AccountView>("get_account", {});
}
```

## 3. 使用规则（比代码重要）

### 3.1 安装计划必须先看一眼 `issues`

```ts
const plan = await prepareGameInstall(req);
const blocking = plan.issues.filter((i) => i.blocking);
if (blocking.length) {
  // 展示原因，不要调用 start
  return;
}
const task = await startGameInstall(plan.planId, plan.revision);
```

- `blocking=true` 的原因：实例名非法、官方清单没有该版本、加载器不可装、内存越界、网络不可用。
- `blocking=false` 只是提示（例如资源包版本不一致）。
- 计划 **15 分钟过期**，过期后 `start` 会抛 `PLAN_EXPIRED`，需要重新 `prepare`。

### 3.2 资源包 / 光影包不要传 `gameVersion`

它们的版本列表通常是跨版本的。传了精确版本会被过滤成空列表（我们踩过）。
只在 `kind === "mod" | "datapack"` 时传 `gameVersion` 与 `loader`。

### 3.3 `datapack` 必须传 `worldId`

`worldId` 只能来自 `listTargetWorlds`，**不要用目录名拼**。
存档被移动或删除后 `prepare` 会抛 `TARGET_CHANGED`，此时应重新拉列表。

### 3.4 更新要先检查再更新

```ts
const report = await checkModUpdates(instanceId);
for (const u of report.updates) {
  await startModUpdate(instanceId, u.fileName, u.projectId, u.latestVersionId);
}
```

`unknown` 表示"不是 Modrinth 来源，无法判断"，不要显示成"有更新"或"已是最新"。

### 3.5 整合包安装是长任务

`startModpackInstall` 内部会依次：建实例 → 补游戏本体 → 装加载器 → 下文件 → 解压 overrides。
失败会回滚实例目录。进度看 `astral://task` 的 `stage`：

| stage | 含义 |
|---|---|
| `install` | 游戏本体/加载器 |
| `download` | 整合包文件 |
| `extract` | 解压覆盖文件 |

### 3.6 日志上传必须二次确认

`prepareDiagnosis` **不会上传**，只返回脱敏预览（`redactions` 是脱敏条数）。
真正的 AI 分析接口（需要 DeepSeek Key）尚未实现——界面在这一步应该明确说
"仅本地预览"，不要伪造分析结果。

### 3.7 MSA 现在只有状态

`getMsaStatus()` 未配置时返回 `configured: false` + 注册步骤。
**不要渲染成"点击登录"**，除非 `configured === true`（即使那样，登录流程也还没实现）。

## 4. 本次新增错误码

| 码 | 出现场景 |
|---|---|
| `PLAN_EXPIRED` | 计划过期/已使用/revision 不匹配 |
| `TARGET_CHANGED` | 存档不存在（数据包目标） |
| `INCOMPATIBLE` | 文件的游戏版本或加载器与实例不符 |
| `FILE_EXISTS` | 目标已有同名文件且未显式 overwrite |
| `HASH_MISMATCH` | 下载后校验不符（文件已删除） |
| `BLOCKED_URL` | 下载来源不在白名单 |
| `INSTANCE_EXISTS` | 创建实例时同 id 已存在 |
| `INTERRUPTED` | 上次启动器退出时任务未结束 |

## 5. 还没实现、不要给入口的

- 真正的 MSA 登录（`msa_begin` / `msa_poll` 不存在）
- AI 诊断（`start_diagnose` 不存在）
- 依赖版本约束匹配（只取第一个兼容版本）
- JDK 删除/清理（`list_managed_jdks` 只读）

> 任务重试与 JDK 下载**已实现**，见上面的 `retryTask` / `installJava`。
