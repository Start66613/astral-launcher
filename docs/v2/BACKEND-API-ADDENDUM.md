# 后端接口增量说明（V2.5）

> 给前端接入用。所有命令都是 Tauri `invoke`，参数名 **camelCase**，
> 返回结构体字段也是 **camelCase**（Rust 侧统一 `#[serde(rename_all = "camelCase")]`）。
> 错误统一是字符串 `CODE|message|detail`，`detail` 可能为空。
>
> 本文只列 V2.5 新增 / 语义有变化的命令。老命令见 `docs/api-contract.md`。

## 错误码速查

| 码 | 含义 | 界面建议 |
|---|---|---|
| `NETWORK` | 网络失败、5xx、超时 | 可重试，给"重试"按钮 |
| `BLOCKED_URL` | 域名不在白名单 / 解析到内网 / 重定向到非法地址 | 不要重试，提示来源不受信任 |
| `DISK_FULL` | 空间不足 | 提示清理，给"打开缓存目录" |
| `HASH_MISMATCH` | 下载内容校验不符 | 可重试一次，仍失败则提示来源异常 |
| `CANCELLED` | 用户取消 | 不要当错误弹窗，显示"已取消" |
| `BUSY` | 同一个文件正在被别的任务下载 | 提示"另一个任务在下载同一个文件" |
| `INSTANCE_BUSY` | 实例正在运行/有任务 | 提示先退出游戏或等任务结束 |
| `UNSUPPORTED` | 这个操作不支持 / 缺 `confirm` | 缺 confirm 时应该先弹确认框再重调 |
| `INVALID_NAME` / `INVALID_VALUE` / `INVALID_PATH` | 输入或路径不合法 | 就地标红，不用弹窗 |
| `FILE_EXISTS` | 目标已存在 | 提示"已存在"，提供覆盖选项 |
| `NOT_FOUND` / `INSTANCE_NOT_FOUND` / `VERSION_NOT_FOUND` | 找不到 | 刷新列表 |
| `PLAN_EXPIRED` | 计划过期（默认 15–30 分钟） | 重新走一遍"预览 → 确认" |
| `VERIFY_FAILED` | 校验失败 | 展示 `detail` |
| `DEPENDENCY_CYCLE` / `DEPENDENCY_CONFLICT` | 依赖成环 / 版本冲突 | 展示冲突列表让玩家选 |
| `JAVA_MISSING` / `JAVA_UNSUPPORTED` | 缺 Java / 版本不对 | 引导到"运行库"页面 |
| `INTERNAL` | 不该发生的错误 | 展示 `detail` 并建议反馈日志 |

```ts
// 统一解析错误
export type ApiError = { code: string; message: string; detail?: string };

export function parseError(e: unknown): ApiError {
  const raw = typeof e === "string" ? e : String(e);
  const [code, message, ...rest] = raw.split("|");
  return { code: code || "INTERNAL", message: message || raw, detail: rest.join("|") || undefined };
}
```

---

## 1. 下载与调度（A1/A2/A3）

### `net_scheduler_stats()`

实时调度状态，建议 1 秒轮询一次，用于"下载中心"的进度条。

```ts
export type SchedulerStats = {
  active: number;      // 正在下载的文件数
  maxActive: number;   // 全局并发上限（6）
  maxPerHost: number;  // 单域名并发上限（3）
  hosts: Record<string, number>;
  downloads: number;   // 本次启动累计下载次数
  bytes: number;
  resumed: number;     // 续传次数
  retries: number;
};

const stats = await invoke<SchedulerStats>("net_scheduler_stats");
```

### `cache_stats()` / `cache_enforce_budget()` / `cache_clear(confirm)`

跨实例共享的下载缓存（`%APPDATA%\Astral\cache\objects`，默认上限 1 GiB）。

```ts
export type CacheStats = {
  entries: number;
  bytes: number;
  budgetBytes: number;
  available: boolean;
};

await invoke<CacheStats>("cache_stats");
await invoke<number>("cache_enforce_budget");        // 返回删掉的字节数
await invoke<number>("cache_clear", { confirm: true }); // 不传 confirm 会返回 UNSUPPORTED
```

### `clean_partials(days?, confirm?)`

清理残留的 `.part` / `.part.json`。`days = 0` 表示全部清理，此时必须 `confirm: true`。

```ts
const r = await invoke<{ files: number; bytes: number }>("clean_partials", { days: 7 });
```

### `disk_usage()`

```ts
const u = await invoke<{
  assets: number; libraries: number; versions: number;
  instances: number; logs: number; cache: number; totalBytes: number;
}>("disk_usage");
```

---

## 2. Mod 依赖与更新（B1/B2）

### `resolve_mod_dependencies(instanceId, projectId, versionId)`

**只读预览，不下载任何东西**。给"安装前确认"弹窗用。

```ts
export type DepNode = {
  projectId: string; versionId: string; name: string;
  versionNumber: string; channel: "release" | "beta" | "alpha" | string;
  fileName: string; url: string; size: number;
  sha1: string | null; sha512: string | null;
  depth: number; kind: "required" | "optional" | "root";
  via: string[];                 // 谁把它带进来的
  replacesVersion: string | null; // 实例里已有的旧版本
};

export type DepResolved = {
  nodes: DepNode[];              // 已按"依赖在前"排序
  cycles: string[][];
  conflicts: { code: string; projectId: string; message: string; blocking: boolean }[];
  skipped: { projectId: string; reason: string; detail: string }[];
  downloadBytes: number;
  truncated: boolean;
  visits: number;
};

const r = await invoke<DepResolved>("resolve_mod_dependencies", {
  instanceId, projectId, versionId,
});
// r.conflicts 里有 blocking=true 的项时，不要继续安装
```

`skipped[].reason` 取值：`installed`（已装同版本）、`installed-unknown`（已装但认不出版本，
**没检查它自己的依赖**）、`optional`（可选且未勾选）、`unresolved`（查不到）。

### `plan_mod_updates(instanceId, updates)` → `apply_mod_updates(app, instanceId, plan)`

两段式：先出计划给人看，再执行。**执行失败会整批回滚**。

```ts
export type UpdatePlanItem = {
  fileName: string; projectId: string;
  currentVersion: string; targetVersion: string; targetVersionId: string;
  targetFileName: string; size: number;
  renames: boolean;              // 文件名会变（新旧文件不同名）
  blocked: string | null;        // 非 null 表示这一条不能更新，值是原因
  notes: string[];
};

export type UpdatePlan = {
  planId: string; instanceId: string;
  items: UpdatePlanItem[];
  skipped: string[];
  totalBytes: number;
  blocking: boolean;             // 全部被挡时为 true
  createdAt: string; expiresAt: string;
};

export type UpdateResult = {
  batchId: string;
  updated: string[];
  failed: [string, string][];
  rolledBack: boolean;
};

const plan = await invoke<UpdatePlan>("plan_mod_updates", { instanceId, updates });
// 用户确认后：
const res = await invoke<UpdateResult>("apply_mod_updates", { instanceId, plan });
```

**注意**：稳定版不会被自动升成 beta/alpha，这类条目会带 `blocked`。界面上要显示原因，
不要静默跳过。

### `mod_update_history(instanceId)` / `rollback_mod_updates(instanceId, batchId)` / `prune_mod_update_backups(instanceId, keepDays?)`

```ts
export type BackupRecord = {
  batchId: string; createdAt: string;
  files: Record<string, string>;    // 旧文件名 → 备份内文件名
  replaced: Record<string, string>; // 旧名 → 新名
  note: string;
};

await invoke<BackupRecord[]>("mod_update_history", { instanceId });
await invoke<number>("rollback_mod_updates", { instanceId, batchId }); // 恢复的文件数
await invoke<[number, number]>("prune_mod_update_backups", { instanceId, keepDays: 7 });
```

---

## 3. 文件检查与修复（B3）

### `check_version_files(instanceId?, version, deep?)`

四态分开报，**不要合成一个 healthy**。

```ts
export type FileState = {
  path: string;                 // 相对路径，`/` 分隔
  kind: "client-jar" | "library" | "asset-index" | "asset-object";
  exists: boolean;
  complete: boolean | null;     // null = 没查（比如只比了大小）
  expectedSize: number; actualSize: number;
  expectedSha1: string | null; actualSha1: string | null;
  detail: string;
};

export type VersionReport = {
  version: string; chain: string[];
  files: FileState[];
  missing: number; corrupt: number; ok: number;
  exists: boolean; complete: boolean; compatible: boolean;
  launchable: boolean | null;   // 检查阶段恒 null，别显示成"能启动"
  requiredJava: number | null;
  java: string | null;
  issues: string[];
  checkedAt: string;
};

const report = await invoke<VersionReport>("check_version_files", {
  instanceId, version, deep: true,
});
```

### `plan_version_repair(instanceId?, version, deep?)` → `repair_version(app, instanceId?, version, plan, allowReplace?)`

```ts
export type RepairAction = {
  kind: "reinstall-base" | "reinstall-loader" | "install-java";
  path: string | null; url: string | null;
  expectedSha1: string | null; expectedSize: number;
  reason: string;
  overwritesUserFile: boolean;   // 恒 false，后端保证不碰玩家文件
};

export type RepairPlan = {
  planId: string; instanceId: string | null; version: string;
  actions: RepairAction[]; downloadBytes: number;
  needsConfirm: boolean;         // true 时必须弹确认框
  summary: string[]; untouched: string[];
  createdAt: string;
  loaderKind: string | null; loaderVersion: string | null;
  corruptPaths: string[];
};

const [report, plan] = await invoke<[VersionReport, RepairPlan]>("plan_version_repair", {
  instanceId, version,
});
// allowReplace=true 时，损坏文件会先备份到 .astral-repair/<batch>/
await invoke<string>("repair_version", { instanceId, version, plan, allowReplace: true });
```

`untouched` 固定会列出 `mods/`、`config/`、`saves/`、`resourcepacks/ 与 shaderpacks/`、
`options.txt 与服务器列表`，界面上直接展示，让玩家放心。

`repair_backups()` 返回修复备份批次列表（倒序）。

---

## 4. 整合包（B4）

### `prepare_modpack_install(request)` / `import_local_modpack(path, name?)` / `start_modpack_install(planId, revision, includeOptional?)`

```ts
export type ModpackPlan = {
  planId: string; revision: number; expiresAt: string;
  info: {
    name: string; summary: string; versionName: string;
    mcVersion: string; loaderKind: string; loaderVersion: string;
    fileCount: number; totalBytes: number;
    clientFiles: number; optionalFiles: number;
    unsupportedFiles: number; serverOnlyFiles: number;
    overrideEntries: number;
    source: "modrinth" | "local";
    projectId: string; versionId: string;
  };
  issues: { code: string; message: string; blocking: boolean }[];
  downloadBytes: number;
};

// 从 Modrinth 装
const plan = await invoke<ModpackPlan>("prepare_modpack_install", {
  request: { projectId, versionId, name, memoryMb: 4096 },
});
// 从本地 .mrpack 装（会复制到 {game}/.astral/modpacks/，不动原文件）
const local = await invoke<ModpackPlan>("import_local_modpack", { path: "D:\\packs\\x.mrpack" });

// includeOptional=false（默认）只装 required + client 必需的文件
await invoke("start_modpack_install", {
  planId: plan.planId, revision: plan.revision, includeOptional: false,
});
```

`serverOnlyFiles` > 0 说明这个包里有服务端专用文件，客户端会跳过。

### `modpack_installed_versions(projectId)` / `verify_exported_modpack(path)`

```ts
await invoke<{ instanceId: string; versionId: string; versionName: string;
               installedAt: string; source: string }[]>(
  "modpack_installed_versions", { projectId });

await invoke<{ ok: boolean; path: string; name: string; mcVersion: string;
               loaderKind: string; loaderVersion: string; files: number;
               overrideEntries: number; totalBytes: number; issues: string[] }>(
  "verify_exported_modpack", { path });
```

每个实例目录里的 `.astral-pack.json` 记录了来源（`PackProvenance`），`modpack_installed_versions`
就是扫这个文件。

---

## 5. 回收站与存档（C1/C2）

### `list_trash()` / `restore_from_trash(trashName)` / `purge_trash(names, confirm)` / `purge_all_trash(confirm)`

```ts
export type TrashEntry = {
  trashName: string; instanceId: string; name: string;
  gameVersion: string; loaderKind: string; loaderVersion: string;
  deletedAt: string; bytes: number; fileCount: number;
  restorable: boolean; reason: string;
};

export type RestoreResult = {
  instanceId: string; trashName: string;
  renamed: boolean;        // 原 id 被占用，恢复成了新 id
  issues: string[];        // 恢复后体检发现的问题（缺文件/Java 不兼容等）
};
```

`purge_trash` / `purge_all_trash` 必须 `confirm: true`，否则返回 `UNSUPPORTED`。
**界面上这两个操作要用红色危险按钮 + 二次确认**。

### 存档备份

```ts
export type WorldBackup = {
  backupId: string; worldId: string; worldName: string;
  createdAt: string; bytes: number; fileCount: number;
  sha256: string; source: string; note: string; worldBytes: number;
};

export type RestorePreview = {
  backup: WorldBackup;
  targetDir: string; targetExists: boolean; targetFiles: number;
  needBytes: number; availableBytes: number | null;
  overwriteFiles: number;
  blockers: string[];       // 非空时不能恢复
};

await invoke<WorldBackup[]>("list_world_backups", { instanceId });
await invoke<WorldBackup>("backup_world", { instanceId, worldId, note: "手动备份" });
await invoke<RestorePreview>("preview_world_restore", { instanceId, backupId });
await invoke("restore_world", { instanceId, backupId, overwrite: true });
await invoke<number>("delete_world_backup", { instanceId, backupId, confirm: true });
```

- 游戏运行时 `backup_world` 直接返回 `INSTANCE_BUSY`（**不做热备份**，避免拿到不一致的存档）。
- `restore_world` 不带 `overwrite` 且目标已存在时返回 `FILE_EXISTS`；带 `overwrite` 时
  会先把现有存档改名留底到 `.astral-worlds/replaced-<时间戳>-<名字>`，**不删除**。

---

## 6. 复制与导入（C3）

### `scan_local_versions()` / `plan_instance_copy(...)` / `plan_version_import(...)` / `apply_instance_copy(planId)`

```ts
export type CopyOptions = {
  settings?: boolean;     // mods + config（默认 true）
  worlds?: boolean;       // saves
  resources?: boolean;    // resourcepacks + shaderpacks
  screenshots?: boolean;
  logs?: boolean;
  overwrite?: boolean;
};

export type CopyPlan = {
  planId: string;
  kind: "copy" | "import";
  sourceInstance: string | null;
  sourceVersion: string | null;
  targetName: string;
  items: { category: string; from: string; to: string;
           files: number; bytes: number; conflicts: number }[];
  totalFiles: number; totalBytes: number;
  availableBytes: number | null;
  issues: string[];
  createdAt: string;
};

// 复制现有实例（只复制白名单目录，绝不复制整个 .minecraft）
const plan = await invoke<CopyPlan>("plan_instance_copy", {
  instanceId, name: "新实例", options: { settings: true, worlds: false },
});
// 导入 versions/ 下已有的版本目录
const p2 = await invoke<CopyPlan>("plan_version_import", { version: "1.20.1-forge-47.2.0" });

const res = await invoke<{ instanceId: string; instanceName: string;
                           files: number; bytes: number;
                           skipped: string[]; warnings: string[] }>(
  "apply_instance_copy", { planId: plan.planId });
```

计划**只能用一次**（执行后即失效），过期时间 30 分钟。
兼容实例（`managed: false`，指向共享 `.minecraft`）不能复制，会返回 `UNSUPPORTED`。

---

## 7. Java（C4）

### `java_audit()` / `check_instance_java(instanceId)` / `remove_managed_java(major, confirm)`

```ts
export type JavaCheck = {
  instanceId: string; instanceName: string; gameVersion: string;
  javaPath: string | null;    // null = 自动选择
  source: "instance" | "auto";
  exists: boolean;
  version: string | null; major: number | null; required: number | null;
  requirement: string;        // "1.20.5 起官方要求 Java 21"
  compatible: boolean;
  problem: string | null;     // 具体问题，没有就是 null
  suggestion: string | null;
};

export type JavaAudit = {
  instances: JavaCheck[];
  managed: { major: number; dir: string; bytes: number;
             javaPath: string | null; exists: boolean; referencedBy: string[] }[];
  unused: number[];           // 没人用的托管 JDK 主版本
  broken: string[];           // 有问题的实例 id
  external: string[];         // 系统 Java（只读，不允许删）
  checkedAt: string;
};

await invoke<JavaAudit>("java_audit");
await invoke<JavaCheck>("check_instance_java", { instanceId });
await invoke<{ removed: number; major: number; bytes: number; dir: string }>(
  "remove_managed_java", { major: 21, confirm: true });
```

`remove_managed_java` 只删 `<game>/jvm/adoptium-<major>`，还有实例在用时返回 `INSTANCE_BUSY`。

### `java_cache_stats()` / `clear_java_cache(confirm)`

```ts
await invoke<{ dir: string; count: number; bytes: number;
               files: { name: string; bytes: number; modified: string }[] }>(
  "java_cache_stats");
await invoke<{ removed: number; bytes: number }>("clear_java_cache", { confirm: true });
```

---

## 8. 离线缓存与诊断（D1/D2）

### `list_versions_cached()` / `meta_cache_stats()` / `prune_meta_cache(keepDays?)` / `clear_meta_cache(confirm)`

```ts
export type VersionListReport = {
  versions: { id: string; kind: string; url: string }[];
  cache: {
    source: "cache" | "network";
    ageMs: number;
    stale: boolean;           // true = 数据可能过期，界面上要标出来
    fetchedAt: number;
    networkError?: string;    // 用缓存兜底时，网络失败的原因
  };
};

const r = await invoke<VersionListReport>("list_versions_cached");
if (r.cache.source === "cache") {
  showBadge(r.cache.stale ? "离线缓存（可能过期）" : "离线缓存");
}
```

**关键**：`stale: true` 时不要写"现在一定可以下载"，只能写"这是上次拿到的列表"。

### `diagnose_instance(instanceId, logId?, maxLines?)`

```ts
export type Finding = {
  id: string; label: string; severity: "error" | "warn" | "info";
  hint: string;
  cause: string;        // 可能原因
  confidence: "high" | "medium" | "low";
  category: "java" | "files" | "mods" | "memory" | "graphics"
          | "loader" | "config" | "network" | "account" | "filesystem";
  evidence: string[];
};

export type DiagnosisReport = {
  logId: string; logName: string; lines: number;
  findings: Finding[];
  text: string;              // 已脱敏的日志正文
  summary: string | null;    // 没发现问题时是 null，不要显示"一切正常"
  worst: "error" | "warn" | "none";
  nextSteps: string[];       // 最多 4 条
  basis: string;             // "按 12 条本地规则匹配日志原文（已脱敏 N 处），未上传任何内容"
  localOnly: boolean;
};

const d = await invoke<DiagnosisReport>("diagnose_instance", { instanceId });
```

界面上建议：`worst === "none"` 时显示"没在日志里发现已知故障特征"，
**不要说"没有问题"**——诊断只覆盖已知规则。

---

## 9. 任务与事件

所有长任务都会发 `astral://task` 事件，`Task` 结构和以前一致，新增字段：

```ts
export type Task = {
  // ...原有字段
  bootId: string;
  cancelling: boolean;
  phase: "validating" | "checking-files" | "preparing-java"
       | "downloading" | "launching" | "process-created";
  stageLabel: string;
};
```

新的事件名：`astral://mod-update`（批量更新）、`astral://repair`（修复）、
`astral://game-exit`（游戏退出）、`astral://instance-changed`（实例增删改）。

**取消语义**：`cancel_task` 之后任务进入 `cancelling`，真正停下来是在下一个检查点
（下载是每个 chunk 检查，所以大文件也能在几百毫秒内停）。
`state === "cancelled"` 时 `.part` 会保留，下次可以续传。

**安装类任务同样可取消**（V2.5 补完）：`install` / `loader` / `modpack` / `repair`
这些会调用 portablemc 批量下载器的任务，取消判断已经下沉到批量下载器内部——
每个分片读完一块检查一次，未开始的分片直接标记为已取消，不会再出现
「界面显示已取消、后台还在往磁盘写」的情况。前端不需要额外处理，
`cancelling` → `cancelled` 的时序和以前一样，只是真的会停了。


---

## 10. 资源搜索（V2.5 新增）

以前只能按项目 id 查详情，没有"找资源"的入口。现在有 `search_resources`。

```ts
export type SearchHit = {
  projectId: string;
  slug: string;
  title: string;
  description: string;
  author: string;
  projectType: "mod" | "modpack" | "resourcepack" | "shaderpack" | "datapack" | "plugin";
  categories: string[];
  displayCategories: string[];
  downloads: number;
  follows: number;
  iconUrl: string | null;
  license: string | null;
  versions: string[];
  gameVersions: string[];
  loaders: string[];
  latestVersion: string | null;
  dateModified: string | null;
};

export type SearchResult = {
  hits: SearchHit[];
  totalHits: number;
  offset: number;
  limit: number;
  index: string;
  query: string;
  source: "network" | "cache";   // 这次数据是网络来的还是本地缓存
  stale: boolean;                // 缓存是否已过期（source === "cache" 时才可能 true）
  ageMs: number;                 // 缓存年龄
  networkError: string | null;   // 网络失败但用了缓存时的错误原文
};

const r = await invoke<SearchResult>("search_resources", {
  query: "sodium",
  kind: "mod",            // 省略默认 "mod"
  gameVersion: "1.20.1",
  loader: "fabric",
  limit: 20,              // 1..100
  offset: 0,              // 最大 10000
  sort: "relevance",      // relevance | downloads | follows | newest | updated
});
```

要点：

- `kind` / `sort` 是白名单，传别的值会返回 `INVALID_VALUE`，不会静默改成默认值。
- `gameVersion` / `loader` 会作为 facet 传给 Modrinth，组间是 AND。
- 断网时如果本地有旧结果，会返回 `source: "cache"` + `stale: true` + `networkError`，
  界面上应该提示"这是上次的结果，可能不是最新的"，不要当成实时数据。
- 结果本身不下载任何文件；选中后照旧走
  `get_resource_project` → `list_resource_files` → `prepare_resource_install`。

界面上建议：搜索框防抖 300ms，翻页用 `offset += limit`，`totalHits` 只用来显示总数。

---

## 11. 资源包 / 光影包文件管理（V2.5 新增）

以前资源包和光影包只有"安装"，装完就没法管了；现在补上列表、启停、删除三件事。
和模组一样，**禁用是靠把文件名改成 `.zip.disabled`**，不是移到别的地方——
这样玩家在游戏里的资源包界面照样能看到它，也能自己再启用回来。

### 11.1 数据结构

```ts
type PackKind = "resourcepack" | "shaderpack";

interface PackFile {
  fileName: string;            // 实际文件名，禁用态带 .disabled
  baseName: string;            // 去掉 .disabled 的主体名
  kind: PackKind;
  enabled: boolean;
  sizeBytes: number;
  packFormat: number | null;   // pack.mcmeta 里的 pack_format，光影包恒为 null
  description: string | null;  // pack.mcmeta 里的描述（文本组件会取 text）
  compatibleVersions: string[];// 由 packFormat 推出的适配版本，如 ["1.20 – 1.20.1"]
  note: string | null;         // 给人看的提醒，例如"包里没有 pack.mcmeta"
}
```

### 11.2 命令

```ts
// 列出某个实例下的资源包或光影包
const packs = await invoke<PackFile[]>("list_packs", {
  instanceId: "inst_xxx",
  kind: "resourcepack",
});

// 启用 / 禁用
const one = await invoke<PackFile>("set_pack_enabled", {
  instanceId: "inst_xxx",
  kind: "resourcepack",
  fileName: "Better Leaves.zip",
  enabled: false,
});

// 删除（必须显式确认）
await invoke<void>("delete_pack", {
  instanceId: "inst_xxx",
  kind: "resourcepack",
  fileName: "Better Leaves.zip",
  confirm: true,
});
```

要点：

- `kind` 只接受 `resourcepack` / `shaderpack`，其他值返回 `INVALID_VALUE`；
  `datapack` 在存档里，不走这套命令。
- `delete_pack` 不带 `confirm: true` 会返回 `UNSUPPORTED|删除文件需要明确确认`，
  界面上应该先弹确认框，不要默认传 true。
- `packFormat` 是**读出来的**，不是猜的：包里没有 `pack.mcmeta` 时是 `null`，
  `note` 会写"游戏可能不认"；`pack_format` 不在已知对照表里时 `compatibleVersions` 为空、
  `note` 写"不在已知对照表里"，界面别硬编成"支持所有版本"。
- 光影包靠包里有没有 `shaders/` 目录判断，没有的话 `note` 会提示"可能不是光影包"。
- 这些命令都不会去改 `options.txt` 里的启用顺序——那是游戏自己管的，
  替玩家改顺序出问题的概率远大于收益。

---

## 12. 本地 Mod 静态扫描（V2.5 新增）

和 `resolve_mod_dependencies` 的分工要分清：

| | `resolve_mod_dependencies` | `scan_mods` |
|---|---|---|
| 联网 | 是（Modrinth） | **否** |
| 回答 | "装这个之前还要下什么" | "现在这堆 mod 为什么起不来" |
| 会不会改文件 | 不会（只出计划） | 不会（纯只读） |

### 12.1 数据结构

```ts
interface ScannedId { modId: string; name: string; version: string; loader: string }
interface ScannedDep { modId: string; versionRange: string | null; mandatory: boolean }

interface ScannedMod {
  fileName: string;
  enabled: boolean;
  sizeBytes: number;
  ids: ScannedId[];          // 一个 jar 可能有多个 mod id（jar-in-jar 之类）
  depends: ScannedDep[];
  conflicts: ScannedDep[];   // fabric 的 breaks / forge 的 type="breaks"
  error: string | null;      // 读不出来时的原因
}

interface ModIssue {
  code: "duplicate-id" | "missing-dependency" | "version-mismatch"
      | "broken-jar" | "no-metadata" | "declared-conflict" | "disabled-duplicate";
  severity: "error" | "warn" | "info";
  fileName: string | null;
  modId: string | null;
  message: string;
  detail: string | null;
  confidence: "high" | "medium" | "low";
}

interface ModScanReport {
  mods: ScannedMod[];
  issues: ModIssue[];
  counts: { total: number; enabled: number; disabled: number;
            readable: number; unreadable: number; errors: number; warnings: number };
  summary: string | null;   // 没问题时是 null，不硬编"一切正常"
}
```

### 12.2 调用

```ts
const report = await invoke<ModScanReport>("scan_mods", { instanceId: "inst_xxx" });

// 只显示需要处理的
const fatal = report.issues.filter(i => i.severity === "error");
```

要点：

- **纯离线**，断网也能用；读的是 jar 里的 `fabric.mod.json` / `quilt.mod.json` /
  `META-INF/mods.toml` / `META-INF/mcmod.info`。
- `severity` 分级是保守的：**重复 mod id 是 error**（几乎必然崩），
  **版本不满足是 warn**（版本号写法太多，可能只是我判断不了），
  **判断不了范围时给 info + `confidence: "low"`**，绝不冒充"确定不兼容"。
- `version-mismatch` 的 `confidence` 是 `medium`：Modrinth 上的范围写法
  （如 `>=1.20 <1.21`、`1.20.x`）和 Forge/NeoForge 的 Maven 区间
  （`[1.20,1.21)`、`(1.20,1.21]`、`[47,)`、`[1.20.1]`、多段 `[1.18,1.19.3),[1.20,1.21)`）
  都能解析；确实解析不出来的写法（`>=abc`）才判未知并给低置信度。
- `minecraft` / `forge` / `neoforge` / `fabric-api` 这些平台 id 不算缺失依赖。
- 结果里 `mods` 含已禁用的（`enabled: false`），禁用的不参与依赖判定。
- 这个接口**不会**删任何东西，删要靠 `delete_mod`。
---

## 13. 已安装 Mod 账本（V2.5 新增，内部机制）

这一节没有新命令，讲的是 `resolve_mod_dependencies` 背后的一条数据：

### 13.1 为什么需要

`mods::list` 从 jar 里只能读到**人类可读的版本号**（`1.0.0`、`mc1.20.1-0.5.13`），
拿不到 Modrinth 的 `version_id`。而依赖解析要判断"已装的这个 Mod 是不是依赖要的那个版本"，
必须知道 version id。没有它，所有已装的 Mod 只能进 `unknown`——解析器会跳过安装，
"装的是旧版、依赖要新版"这种情况永远发现不了。

### 13.2 存在哪

`<实例>/mods/.astral-ledger.json`：

```json
{
  "files": {
    "sodium-fabric-0.5.13+mc1.20.1.jar": {
      "projectId": "AANobbMI",
      "versionId": "abc123xy",
      "versionNumber": "0.5.13+mc1.20.1",
      "fileName": "sodium-fabric-0.5.13+mc1.20.1.jar",
      "installedAt": "2026-10-27T01:20:00Z"
    }
  }
}
```

### 13.3 规则（重要，前端不要依赖"所有已装 Mod 都有账本"）

- **只有 Astral 自己装下来的才记账**：`prepare_resource_install` → 执行安装、
  批量 Mod 更新。玩家手动丢进 `mods/` 的 jar **不会**有账本条目。
- 账本跟着文件走：启用/禁用改名、删除、批量更新换文件、回滚，都会同步。
- 文件被玩家手动删掉后，账本条目虽然还在，但**不再算已装**（`live()` 会过滤）。
- 所以 `resolve_mod_dependencies` 的结果里，`skipped` 里的
  `installed_unknown` 是**正常的**——那是玩家自己放的文件，不是 bug。
- 账本写失败不影响安装成功（只影响下次解析的精度），不要把它当成致命错误。

### 13.4 对前端的要求

不需要读这个文件。需要展示"这个 Mod 是哪个版本"时，用 `list_mods` 的
`version` 字段（人类可读的那个）即可；`scan_mods` 的 `mods[].version` 同理。


---

## 14. 实时日志跟随（V2.5 新增）

### 14.1 命令

```ts
invoke<LogChunk>("tail_log", {
  instanceId: string,
  logId?: string,     // 不传就自动跟"当前正在写"的那份游戏日志
  offset?: number,    // 上次返回的 nextOffset，第一次传 0 或不传
  maxBytes?: number,  // 单次上限，默认 65536，最大 262144
})
```

### 14.2 返回

```ts
interface LogChunk {
  id: string;
  name: string;
  fromOffset: number;    // 这次从哪个字节开始读的
  nextOffset: number;    // 下次该传的 offset
  text: string;          // 已脱敏，且只含完整的行
  lines: number;         // 这次带回多少行
  eof: boolean;          // true = 已经追到文件末尾（下次可能还有新内容）
  reset: boolean;        // true = 文件被重写过，这次是从头给的
  pendingBytes: number;  // 还剩多少字节没读（一次读不完就接着调）
  truncated: boolean;    // 是否因为超长单行而截断
}
```

### 14.3 前端怎么用

```ts
let offset = 0;
let timer: number | undefined;

async function tick() {
  const c = await invoke<LogChunk>("tail_log", {
    instanceId, logId, offset,
  });
  if (c.reset) {
    // 游戏重启把 latest.log 重写了：把界面上的旧内容清掉再贴
    clearViewer();
  }
  if (c.text) appendText(c.text);
  offset = c.nextOffset;
  // pendingBytes 还有就立刻再要一次，否则等下一拍
  if (c.pendingBytes > 0) queueMicrotask(tick);
}

// 500ms 一拍足够；游戏自己写日志也就这个频率
timer = window.setInterval(tick, 500);
```

要点：

- **只给完整的行**。末尾没写完的半行不会返回，`nextOffset` 也停在半行之前，
  所以前端不会出现"半截乱码"或者拼错行。
- **`reset` 必须处理**。游戏重启会把 `latest.log` 重写成长度更短的新文件，
  这时后端自动 `reset=true` 并从 0 开始给；前端要清空已有内容，否则会重复。
- **`logId` 可以不传**。不传时后端找 `modifiedAt` 最新的游戏日志（按文件系统
  真实 mtime，不是秒级字符串），启动后直接就能跟，不用先 `list_logs`。
- **一次读不完会分批**。`pendingBytes > 0` 说明还有，接着用 `nextOffset` 调；
  单行超长（几百 KB 的堆栈或 JSON）读满上限就整段返回并标 `truncated`，
  不会死等一个换行。
- 内容和 `read_log` 一样走脱敏（邮箱、IP、token、用户目录路径），可以放心贴到界面。

---

## 15. 服务器状态查询（V2.5 新增）

### 15.1 命令

```ts
invoke<ServerStatus>("ping_server", {
  host: string,        // "play.example.com" 或 "play.example.com:25566" 都行
  port?: number,       // 不传就用地址里带的，再没有就 25565
  timeoutMs?: number,  // 默认 4000，夹在 500..30000
})
```

### 15.2 返回

```ts
interface ServerStatus {
  host: string;
  port: number;
  online: boolean;          // 没连上就是 false，不会抛异常
  latencyMs: number;        // 握手 + 状态查询的往返耗时
  versionName: string | null;      // "1.20.1"
  versionProtocol: number | null;  // 763
  playersOnline: number | null;
  playersMax: number | null;
  sample: string[];         // 在线玩家名，服务器只给前几个
  motd: string;             // 剥掉 § 颜色代码的纯文本，直接显示用
  motdRaw: string;          // 带颜色代码，想自己渲染时用
  favicon: string | null;   // "data:image/png;base64,..."
  legacy: boolean;          // true = 走的是 1.6 及更早的老协议
  error: string | null;     // online=false 时一定有值，可以直接显示
}
```

### 15.3 前端怎么用

```ts
async function refresh(addr: string) {
  try {
    const s = await invoke<ServerStatus>("ping_server", { host: addr });
    if (!s.online) {
      setStatus(`离线：${s.error}`);
      return;
    }
    setStatus(
      `${s.versionName} · ${s.playersOnline}/${s.playersMax} 人 · ${s.latencyMs}ms`,
    );
    setMotd(s.motd);          // 已经去过颜色代码
    setIcon(s.favicon);       // 可能为 null
  } catch (e) {
    // 只有地址写错才会走到这里（空、带空格、端口 0）
    setStatus(String(e));
  }
}
```

要点：

- **不要把"连不上"当异常处理**。服务器没开、地址写错、防火墙拦了都是常见情况，
  后端一律返回 `online: false` + 中文 `error`；只有参数本身不合法才抛错。
- **地址一个框就够**。玩家粘贴 `play.example.com:25566` 或 `[::1]:25565`
  后端会自己拆，不需要单独的端口输入框。
- **MOTD 有两份**。`motd` 已经去掉 `§a` 这类代码，直接显示；想按服务器原意
  渲染颜色就用 `motdRaw` 自己解析。
- **favicon 只信任内嵌图**。后端只接受 `data:image/...` 且 ≤256 KiB，
  其它一律丢弃，前端不用做额外校验，也不会有外链请求。
- **超时默认 4 秒**。批量刷新服务器列表时建议前端自己限制并发（比如 4 个
  一批），别一次发几十个。
- 刷新频率建议 ≥30 秒一次；服务器列表协议没有订阅机制，只能轮询。

---

## 16. 收藏的服务器列表（V2.5 新增）

### 16.1 六个命令

```ts
// 全部收藏，按玩家排好的顺序
invoke<ServerEntry[]>("list_servers")

// 加一个；host 可以直接粘贴 "example.com:25566"
invoke<ServerEntry>("add_server", { name, host, port?, note? })

// 改；只传要改的字段，其余保持不变
invoke<ServerEntry>("update_server", { id, name?, host?, port?, note? })

// 删（只删记录，不碰任何文件）
invoke<void>("remove_server", { id })

// 按给定顺序重排；没提到的保持原相对顺序排在后面
invoke<ServerEntry[]>("reorder_servers", { ids })

// 批量查状态；不传 ids 就查全部
invoke<[string, ServerStatus][]>("refresh_servers", { ids? })
```

### 16.2 数据结构

```ts
interface ServerEntry {
  id: string;        // "s_<8位十六进制>"
  name: string;      // 没填就用 "host:port" 兜底
  host: string;      // 已经和端口分开
  port: number;
  note: string;      // 备注，最长 200 字符
  addedAt: string;   // ISO
  lastSeen: string | null;  // 只有真查到过才有值
}
```

### 16.3 前端怎么用

```ts
// 进页面先读本地清单（不联网，立刻能显示）
const list = await invoke<ServerEntry[]>("list_servers");

// 然后并行刷一轮状态
const results = await invoke<[string, ServerStatus][]>("refresh_servers", {});
const byId = new Map(results);
for (const s of list) {
  const st = byId.get(s.id);
  render(s, st);   // st 一定存在，只是可能 online:false
}
```

要点：

- **不碰 `servers.dat`**。清单存在 `<game>/.astral-servers.json`，和游戏内部
  的 NBT 格式完全分开，不会打架。也就是说：**在启动器里加的服务器不会自动出现在
  游戏内的多人列表里**，这是有意的取舍（改 NBT 风险大），需要的话玩家自己在游戏里加。
- **`refresh_servers` 一定返回全部条目**，不会因为某个地址连不上就少一条。
  每条都有自己的 `online` / `error`，前端直接按 id 对应即可。
- **`lastSeen` 只在成功时更新**。连不上不会写，所以"最近见过"是可信的。
- 加重复地址会返回 `INVALID_VALUE`（同一个 host+port 只允许一条）；改回自己
  原来的地址不算冲突。
- 名字超 48 字符会被截断（不报错），备注超 200 字符同理；最多存 200 个。
- 批量刷新内部是 8 线程并行，前端不要再自己并发一轮，直接等这一个调用即可。
- 建议刷新频率 ≥30 秒；服务器列表协议没有推送机制。

---

## 17. 存档详情与 `level.dat`（V2.5 新增）

### 17.1 变了什么

`list_saves` 和 `list_target_worlds` 以前 `gameVersion` 永远是 `null`（当时注释写的是
"level.dat 是 NBT，暂不解析"）。现在解析了，所以这两个接口多出几个字段，
**老字段一个没动**，前端不改也能跑。

```ts
interface SaveEntry {
  name: string;
  dir: string;
  gameVersion: string | null;   // 现在是真值了，例如 "1.20.1"
  lastModified: string;
  sizeBytes: number;
  iconPath: string | null;

  // 新增
  levelName: string | null;     // 存档自报的名字，可能和目录名不同
  versionId: number | null;     // 数据版本号，如 3465
  gameMode: string | null;      // "生存" / "创造" / "冒险" / "观战"
  difficulty: string | null;    // "和平" / "简单" / "普通" / "困难"
  hardcore: boolean | null;
  allowCommands: boolean | null;
  lastPlayed: string | null;    // 存档自报的上次游玩时间（UTC ISO）
  infoNote: string | null;      // 读不出来时的一句人话
}
```

### 17.2 新命令：`get_world_info`

列表只给概要（要扫很多存档，不能每个都深读），点开某一个再要详情：

```ts
invoke<WorldDetail>("get_world_info", { instanceId, worldId })
```

```ts
interface WorldDetail {
  worldId: string;
  displayName: string;
  dir: string;
  lastModified: string;
  sizeBytes: number;
  gameVersion: string | null;
  levelName: string | null;
  versionId: number | null;
  gameMode: string | null;
  difficulty: string | null;
  hardcore: boolean | null;
  allowCommands: boolean | null;
  lastPlayed: string | null;
  dataVersion: number | null;
  infoNote: string | null;
  regionFiles: number;       // region/*.mca 数量，大致等于探索过的区块组
  playerFiles: number;       // playerdata/*.dat 数量
  hasIcon: boolean;
  looksLikeServer: boolean;  // 有 session.lock + level.dat_old，像是服务端存档
}
```

要点：

- `worldId` 还是 `w_<8位十六进制>`（由目录名派生），不是目录名，也不是
  `levelName`。前端传 `worldId`，Rust 负责映射回真实目录。
- **`lastPlayed` 优先用存档自报的时间**，而不是文件 mtime。复制、解压、
  从备份恢复都会改 mtime，但存档内部记的时间不会变——那才是真的"上次玩"。
- 读不出来（`level.dat` 损坏、被替换成别的文件、加密）时：
  存档**照常出现在列表里**，`infoNote` 给一句 "level.dat 读不出来（可能已损坏）"，
  其余字段为 `null`。**不会因为一个坏存档让整页失败。**
- `get_world_info` 找不到存档返回 `WORLD_NOT_FOUND|存档不存在`。
- 难度/模式是中文（"生存"/"普通"），直接显示即可；要枚举的话按上面的字符串匹配。
- `looksLikeServer` 只是个提示，不是判定——真实服务器存档可能没有这两个文件，
  单人存档也可能因为开过局域网而有 `session.lock`。
---

## 18. 整合包导出预览与递归打包（V2.5 新增）

导出整合包原来只有 `export_modpack` 一步到位：给个名字就直接写盘，玩家事先
不知道会打进去多少、多大、磁盘放不放得下。现在补一个**只读预览**，先看清楚
再决定要不要导。

### 18.1 `plan_modpack_export`

```ts
invoke<ExportPlan>("plan_modpack_export", {
  request: {
    instanceId,
    name: "My Pack",
    version: "1.0.0",       // 可选，默认 1.0.0
    includeConfig: true,    // 打 config/（递归）
    includeResources: false // 打 resourcepacks/ 与 shaderpacks/
  }
});
```

```ts
interface ExportPlan {
  instanceId: string;
  name: string;
  version: string;
  fileName: string;        // 例如 my-pack-1-0-0.mrpack
  targetPath: string;      // <gameDir>/exports/<fileName>
  exists: boolean;         // 目标已存在 → 导出会失败，要换版本号
  indexedMods: number;     // 能对上 Modrinth，写进 index（包里不含它们的字节）
  overrideMods: number;    // 对不上，整包带上
  configFiles: number;     // 递归统计的 config 文件数
  resourceFiles: number;   // 资源包 + 光影包文件数
  packFiles: number;       // 会真正写进 zip 的文件总数
  estimatedBytes: number;  // .mrpack 大致大小（不含 index 里另下的 mod）
  freeBytes: number | null;// 目标盘可用空间；拿不到是 null
  issues: string[];        // 阻塞原因与非阻塞提示混在一起
  blocking: boolean;       // true = 现在导会失败，先处理 issues
}
```

预览**不写任何文件**，可以随便点。

### 18.2 `export_modpack`（行为有变化）

`ExportRequest` 新增 `includeResources`（默认 `false`，老调用方不受影响）。

```ts
interface ExportResult {
  path: string;
  indexedFiles: number;
  overrideFiles: number;
  configFiles: number;    // 新增
  resourceFiles: number;  // 新增
  bytes: number;
}
```

变化点：

- **`includeConfig` 现在是递归的**。以前只拷 `config/` 第一层，`config/jei/…`
  这类子目录全丢，导出的包装回去配置就不对了。
- `includeResources` 把 `resourcepacks/`、`shaderpacks/` 一起放进 `overrides/`。
- 收集时**跳过符号链接与 Windows junction**（否则一个指到 `C:\` 的链接能把
  整块盘打进包里），条数上限 20000、总量上限 2 GiB、层级上限 12。
- 写盘前做一次空间检查，不够报 `DISK_FULL`。
- 先写 `.mrpack.part`，全部写完再原子改名 —— 中途失败不会留下半个包骗人。
- 目标同名已存在仍然报 `FILE_EXISTS`，预览里对应 `exists: true` + `blocking: true`。

### 18.3 纯配置包不再被当成坏包

修了一个真 bug：`modpack::parse_index` 原来只要 `files` 为空就报"没有任何可安装
文件"。但 Modrinth 上有一类**纯配置包**（只带 `overrides/` 里的配置或资源包，
没有需要下载的文件），这是合法形态，之前会被直接拒掉。

现在改成：`read_index` 里 **`files` 为空且 overrides 也为空**才算空包。只解析
JSON 的 `parse_index` 不再对空 `files` 报错（它看不到 overrides）。

前端要注意的：`ModpackInfo.fileCount` 可能是 0，同时 `overrideEntries` 大于 0，
这是正常的配置包，不是坏包。

### 18.4 前端接入建议

1. 点"导出"先弹预览（`plan_modpack_export`），把 `indexedMods` / `overrideMods`
   / `configFiles` / `estimatedBytes` 列出来。
2. `blocking` 为 true 时禁用"开始导出"，把 `issues` 逐条显示。
3. `exists` 为 true 时让玩家改版本号，不要自动覆盖。
4. 导出完成后再调 `verify_exported_modpack` 回读校验，把 `issues` 展示出来。

---

## 19. 存档管理：改名 / 复制 / 删除到回收站（V2.5 新增）

以前只有"备份 / 恢复"，玩家想给存档改个名字、留一份副本、或者删掉一个玩腻
的存档，都得自己去文件管理器里折腾。现在这四件事都能在启动器里做，而且
**默认不丢东西**。

### 19.1 命令一览

| 命令 | 作用 | 会不会动文件 |
|---|---|---|
| `rename_world` | 改存档目录名 | 改（重命名） |
| `duplicate_world` | 复制一份存档 | 新增一份副本 |
| `delete_world` | 删除存档 | **只移到回收站** |
| `list_deleted_worlds` | 列出回收站 | 只读 |
| `restore_deleted_world` | 从回收站恢复 | 移回 saves |
| `purge_deleted_world` | 永久删除 | 真删（需 `confirm`） |

### 19.2 改名 / 复制

```ts
invoke<[string, string]>("rename_world", { instanceId, worldId, newName: "新名字" });
// 返回 [新的 worldId, 新目录名]

invoke<[string, string, number]>("duplicate_world", {
  instanceId, worldId, newName: "副本"
});
// 返回 [新的 worldId, 新目录名, 复制了多少字节]
```

- 名字按 **Windows 规则**校验：不能有空/控制字符/`<>:"/\|?*`，不能以点或空格
  结尾，不能用 `CON`、`NUL`、`COM1` 这类保留名。报 `INVALID_NAME|…`。
- 目标同名已存在一律 `FILE_EXISTS|已经有一个叫「X」的存档了`，**不覆盖**。
- 复制不跟随符号链接 / junction，上限 8 GB 与 20 万条；先复制到
  `saves/.copy-<纳秒>`，成功再改名，失败不留半个存档。
- 改名后 `worldId` 会变（它由目录名派生），前端要拿返回值刷新列表。

### 19.3 删除 / 回收站 / 恢复

```ts
interface DeletedWorld {
  trashName: string;   // 回收站内目录名，恢复与永久删除都用它
  worldName: string;   // 原来的存档目录名
  deletedAt: string;   // ISO8601
  bytes: number;
  fileCount: number;
}

const entry = await invoke<DeletedWorld>("delete_world", { instanceId, worldId });
await invoke<DeletedWorld[]>("list_deleted_worlds", { instanceId });

const [newWorldId, name, replaced] = await invoke<[string, string, string | null]>(
  "restore_deleted_world",
  { instanceId, trashName: entry.trashName, overwrite: false }
);
```

要点：

- 删除是 `std::fs::rename` 进 `<实例>/.astral-worlds/deleted/del_<纳秒>`，
  **同一个盘上瞬间完成，不复制字节**。索引写在 `deleted.json`。
- 索引写失败会把目录**搬回去**，绝不留下"文件在回收站、列表里没有"的孤儿。
- 恢复时目标同名存在：
  - `overwrite: false`（默认）→ `FILE_EXISTS|…先改名或选择覆盖`
  - `overwrite: true` → 先把现有存档改名成 `<名字>-replaced-<纳秒>` 留底，
    再把回收站那份搬回去。**留底不会自动删**，玩家自己决定要不要清。
- 恢复失败（移动出错）会把留底的目录改回原名，不会两头空。
- `purge_deleted_world` 是独立的危险命令：不传 `confirm: true` 直接
  `UNSUPPORTED|永久删除存档需要明确确认`。返回删掉了多少字节。
- 回收站**永不自动清空**，也不参与磁盘清理。

### 19.4 什么时候会拒绝

游戏正在运行（`session::active`）时，改名 / 复制 / 删除 / 恢复全部报
`INSTANCE_BUSY|…先退出游戏…`。原因很实在：跑着的存档是写了一半的状态，
这时候复制出来的东西没法自证一致，改名还会让游戏自己写丢文件。

### 19.5 前端接入建议

1. 存档卡片上给"改名 / 复制 / 删除"三个入口，删除后 toast 提示
   "已移到回收站，可在存档页的回收站里恢复"。
2. 回收站做成折叠区，显示 `worldName` + `deletedAt` + 大小，每条给
   "恢复"和"永久删除"。
3. 永久删除要二次确认弹窗，文案里写明"这一步不能撤销"。
4. 覆盖恢复时先调 `restore_deleted_world` 拿 `FILE_EXISTS`，再弹
   "已有同名存档，是否覆盖（会先留一份备份）"。

---

## 20. 截图浏览（V2.5 新增）

游戏截图以前在启动器里完全看不到，玩家只能自己去翻 `screenshots/`
目录。现在有三件事：列出来（带尺寸）、读出来显示、删掉。

### 20.1 命令

| 命令 | 作用 |
|---|---|
| `list_screenshots` | 列出实例 `screenshots/` 下的图片（按修改时间倒序） |
| `read_screenshot` | 读一张，返回可直接塞进 `<img src>` 的 data URL |
| `delete_screenshot` | 删除一张（必须 `confirm: true`） |

```ts
interface Screenshot {
  fileName: string;
  path: string;          // 完整路径，给"在文件夹中显示"用
  sizeBytes: number;
  modifiedAt: string;    // ISO8601
  width: number | null;  // 读不出尺寸时是 null
  height: number | null;
  note: string | null;   // 例如"不是 PNG，读不出尺寸"
}

interface ScreenshotData {
  fileName: string;
  mime: string;          // image/png | image/jpeg
  sizeBytes: number;
  dataUrl: string;       // data:image/png;base64,...
}

const shots = await invoke<Screenshot[]>("list_screenshots", { instanceId });
const data = await invoke<ScreenshotData>("read_screenshot", {
  instanceId, fileName: shots[0].fileName
});
```

### 20.2 为什么用 data URL 而不是 asset 协议

Tauri 的 `asset://` 需要在 `tauri.conf.json` 里静态声明允许访问的目录。
但游戏目录是玩家可以随便改的配置项，静态 scope 只有两种选择：写死一个
路径（玩家改了就失效），或者放得太宽（等于没限制）。走 IPC 一次读一张
图片，配合 CSP 里已有的 `img-src data:` 就够用，也不用动安全配置。

代价是图片要过一次 IPC 和 base64 编码（体积约 1.33 倍）。所以设了
**单张 8 MB 上限**——MC 截图通常 1–4 MB，4K 也够。超了会报
`INVALID_VALUE|图片太大（N MB），请用系统看图工具打开`，而不是硬塞。

### 20.3 尺寸怎么来的

直接读 PNG 文件头里的 IHDR 块（8 字节签名 + 4 字节长度 + `IHDR` + 宽高），
**不解码像素**。列 200 张截图只读每张前 24 字节，不会因为解码卡住界面。

不是 PNG 的文件（比如 `.jpg`）尺寸返回 `null`，`note` 里说明原因。文件头
损坏的 PNG 也一样，只是少个尺寸，不影响它出现在列表里。

### 20.4 安全约束

- 只接受**纯文件名**：含 `/`、`\`、`:`、以 `.` 开头、`..`、超 200 字符
  一律 `INVALID_NAME|…`。
- 只认 `.png` / `.jpg` / `.jpeg`（大小写不敏感）。
- 读取和删除都拒符号链接 / junction（`INVALID_PATH|…`）。
- 删除必须 `confirm: true`，否则 `UNSUPPORTED|删除截图需要明确确认`。

### 20.5 前端接入建议

1. 截图墙用 `grid`，每格先显示占位，点击时再调 `read_screenshot` 拿大图，
   不要一次读全部（IPC 传 base64 有成本）。
2. 列表里已经有 `width`/`height`，可以按比例排瀑布流，不用等图片加载完。
3. 删除做二次确认；删完重新拉一次 `list_screenshots`。
4. "在文件夹中显示"直接用返回的 `path`，不用自己拼。

---

## 21. 游戏设置：读与改 options.txt（V2.5 新增）

Minecraft 把玩家的图形/声音/操作设置存在实例根目录的 `options.txt` 里，
一行一个 `键:值`。以前启动器完全不管这个文件，玩家想调渲染距离、界面缩放、
语言，都得先进游戏。

现在有两条命令，但**只动白名单里的键**。

### 21.1 命令

```ts
interface OptionChoice { value: string; label: string; }

interface OptionField {
  key: string;
  label: string;              // 中文名，例如"渲染距离"
  group: string;              // 画面 / 界面 / 性能 / 窗口 / 声音 / 操作
  kind: "bool" | "int" | "float" | "enum";
  min: number | null;         // int / float 有
  max: number | null;
  choices: OptionChoice[];    // enum 有，值是文件里存的原始字符串
  value: string | null;       // 文件里的当前值；文件里没这个键就是 null
  default: string;
  note: string | null;        // 一句人话说明
  invalid: boolean;           // 文件里有值但不合法（被改坏了）
}

interface GameOptions {
  path: string;
  exists: boolean;
  modifiedAt: string | null;
  fields: OptionField[];
  otherKeys: number;          // 不在白名单的键有多少个（原样保留，不动）
  rawLines: number;
}

interface OptionsWrite {
  path: string;
  changed: string[];          // 实际变了的键
  backup: string | null;      // 备份文件路径；新建文件时没有
  backupDir: string;
}

const opts = await invoke<GameOptions>("get_game_options", { instanceId });

const result = await invoke<OptionsWrite>("set_game_options", {
  instanceId,
  changes: { renderDistance: "16", lang: "zh_cn" }
});
```

### 21.2 为什么是白名单

`options.txt` 里有几类键**不能随便写**：

- `version` —— 游戏用它判断设置文件版本，写错了整份设置会被重置；
- `key_key.*` / `key_key.*` —— 玩家一个一个按出来的按键绑定；
- `lastServer`、`resourcePacks`、`incompatibleResourcePacks` —— 游戏自己维护。

所以后端只认 30 个确定语义的键（渲染距离、模拟距离、画质预设、平滑光照、
云、粒子、贴图模糊、实体阴影、视角摇晃、视场角、亮度、界面缩放、最大帧率、
垂直同步、全屏、窗口宽高、失焦暂停、主音量/音乐/方块/敌对生物音量、
鼠标灵敏度、反转 Y 轴、自动跳跃、潜行切换、语言、字幕、聊天可见性、
简化调试信息、旁白）。改别的直接报
`INVALID_VALUE|不支持修改「lastServer」，只允许改白名单里的设置项`。

### 21.3 改文件时的三条保证

1. **其余行原样保留**。注释、空行、读不懂的坏行、键的顺序，全部照原样写回，
   只替换目标行。文件里没有的键追加到末尾。
2. **先整批校验再动手**。传 `{renderDistance: "16", guiScale: "9"}` 会因为
   `guiScale` 越界整批拒绝，不会出现"改了一半"。
3. **改之前备份**。复制到 `<实例>/.astral-options/options-<时间戳>.txt`，
   只保留最近 10 份。写文件先落 `options.txt.part` 再替换。

### 21.4 什么时候会拒绝

游戏正在运行时 `set_game_options` 报
`INSTANCE_BUSY|游戏正在运行，先退出游戏再改设置（游戏退出时会覆盖这个文件）`。
这不是保守 —— 游戏退出时会把内存里的设置整份写回 `options.txt`，运行期间
改了等于白改，还会让玩家以为设置没生效。

`get_game_options` 是纯读，运行中也能调。

### 21.5 前端接入建议

1. 按 `group` 分组渲染（画面/界面/性能/窗口/声音/操作），每组一个折叠面板。
2. `kind` 决定控件：`bool` 用开关，`int`/`float` 用滑块（`min`/`max` 就是
   合法范围），`enum` 用下拉（`choices` 里 `label` 是给人看的，`value` 是
   要传回去的原始字符串）。
3. `value === null` 表示文件里没这个键，控件显示 `default` 但**不要**在保存时
   一并写回 —— 只提交用户真正改过的键。
4. `invalid: true` 的项高亮提示"这个值游戏不认，要不要重置成默认"。
5. 保存成功后提示"已备份到 <backupDir>"，给个"打开备份目录"的入口。

---

## 22. 实例封面（V2.5 变更）

`instance.json` 里的 `cover` 以前是**任意字符串**：前端传什么就存什么。
这有两个问题 —— 存绝对路径会把本机目录结构写进配置文件，存 `..` 能指到
别处；而且前端没有 asset 协议，压根显示不出来，等于一个存了也没用的字段。

现在改成：图片复制进实例目录，只存固定文件名，读的时候走 IPC 返回 data URL。

### 22.1 命令

```ts
interface CoverInfo {
  fileName: string;   // `.astral-cover.png` 这类固定名
  mime: string;       // image/png | image/jpeg | image/webp | image/gif
  sizeBytes: number;
  width: number | null;   // 只有 PNG / JPEG 能读出来
  height: number | null;
  dataUrl: string;
}

// 设置：传本机图片路径，后端复制进实例目录
const inst = await invoke<Instance>("set_instance_cover", {
  instanceId, sourcePath: "D:\\图片\\cover.png"
});

// 读：没设置封面返回 null
const info = await invoke<CoverInfo | null>("read_instance_cover", { instanceId });

// 清：同时删文件并把 instance.json 的 cover 置空
const cleared = await invoke<Instance>("clear_instance_cover", { instanceId });
```

### 22.2 行为约定

- **按魔数识别格式**，不信扩展名：PNG / JPEG / WebP / GIF 四种。
  传进来一个改了后缀的文本文件会被拒（`INVALID_VALUE|只支持 …`）。
- 落盘名固定为 `.astral-cover.png` / `.jpg` / `.webp` / `.gif`。
  **换格式时会把旧扩展名的文件删掉**，不会留两个。
- 单张上限 8 MB；拒符号链接；先写 `.part` 再 `rename`，失败清理。
- 读的时候 PNG / JPEG 会顺带报宽高（读文件头，不解码像素），
  WebP / GIF 返回 `null`。
- 文件不在（玩家手动删了）时 `read_instance_cover` 返回 `null`，不报错。

### 22.3 破坏性变更：`update_instance` 不再接受任意 cover

`InstancePatch.cover` 现在**只接受 `.astral-cover.<ext>`**，其它值报：

```
INVALID_VALUE|封面请用 set_instance_cover 设置，不能直接写路径
```

`instance::build` 里还会再 `sanitize` 一次 —— 老配置里如果已经存了绝对
路径，列表里读出来是 `null`（当没有封面），不会显示一个来路不明的路径。

前端如果要支持"选一张图当封面"，应该调 `set_instance_cover`，而不是把
路径塞进 `update_instance`。

### 22.4 前端接入建议

1. 实例卡片：`read_instance_cover` 返回 `null` 就显示默认渐变底 + 首字母，
   有 `dataUrl` 就直接 `<img src={dataUrl}>`。
2. 设置封面的入口用系统文件选择框（Tauri dialog 插件或 `<input type=file>`
   拿到的 `path`），把路径交给 `set_instance_cover`。
3. 换封面不用先 `clear_instance_cover`，直接 `set` 会覆盖。
4. 封面是缩略图场景，别拿 4K 原图当封面 —— 8 MB 上限只是兜底，
   前端可以在选图时提示"建议 1280×720 以内"。

---

## 23. 崩溃报告解析（V2.5 新增）

Minecraft 崩溃时会在 `crash-reports/` 下留一份文本报告。以前这份报告和普通
日志一样处理 —— 只做关键词匹配。但报告本身格式是固定的，字段全都写着，
照着读比猜准得多。

### 23.1 命令

```ts
interface CrashReport {
  isCrashReport: boolean;        // 有没有 "---- Minecraft Crash Report ----"
  time: string | null;           // "2026-10-26 12:00:00"
  description: string | null;    // 崩的时候在干什么："Rendering overlay"
  exception: string | null;      // 顶层异常
  causeChain: string[];          // "Caused by:" 链，由外到内，最多 8 条
  frames: string[];              // 顶层异常的前几行栈帧，最多 12 行
  sections: string[];            // 报告里所有 "-- xxx --" 小节标题
  system: Record<string, string>; // "-- System Details --" 的键值
  suspects: string[];            // 报告自己点名的可疑模组
  suspectsSource: "report" | "inferred" | "none";
  inferredPackages: string[];    // 栈帧里的非原版包名（推断，仅供参考）
  lines: number;
  truncated: boolean;            // 超过 2 MB 只读了尾部
}

interface CrashView {
  logId: string;
  logName: string;
  report: CrashReport;
  text: string;                  // 脱敏后的原文，最多 256 KiB
  findings: Finding[];           // 和 diagnose_instance 同一套规则
  basis: string;                 // 依据说明
}

// 不传 logId 就取最新的崩溃报告
const view = await invoke<CrashView>("read_crash_report", {
  instanceId, logId: null
});
```

### 23.2 为什么不全信 `inferredPackages`

报告里如果有 `Suspected Mods:` 这一节，那就是**游戏自己**认定的可疑模组，
`suspectsSource` 会是 `"report"`，直接用。

没有这一节时（多数 Forge 崩溃报告没有），后端不会硬猜是哪个模组，只把
栈帧里出现的非原版包名列进 `inferredPackages`，`suspectsSource` 保持
`"none"`。前端展示时**必须**把这一块标成"推断"，不能说成"就是它崩的"——
栈帧里出现的第三方包很可能只是路过。

原版/框架包名（`java.`、`net.minecraft`、`com.mojang`、`org.lwjgl`、
`org.apache`、`org.slf4j`、`com.google`、`it.unimi`、`oshi.` 等）会被过滤掉。

### 23.3 和 `diagnose_instance` 的关系

两个命令是**互补**的，不是替代：

- `diagnose_instance` 读普通日志（`latest.log`），按 12 条关键词规则给结论，
  适合"游戏没崩但行为异常"。
- `read_crash_report` 读崩溃报告，按报告格式拆字段，适合"确实崩了"。

`CrashView.findings` 里放的就是同一套关键词规则的结论，所以界面上可以
先展示 `report.description` + `exception`（人话 + 技术细节），再列
`findings`，最后给 `next_steps`。

### 23.4 前端接入建议

1. 实例页检测到 `crash-reports/` 有新文件时（`list_logs` 里 `kind === "crash"`），
   直接弹一个"上次崩溃了，要看看原因吗"的入口。
2. 头部用 `description` + `time`，正文用 `exception` + `causeChain`，
   技术细节折叠 `frames`。
3. `system` 里的 `Minecraft Version` / `Java Version` / `Operating System`
   单独展示成一行标签 —— 这三个是排查时最常看的。
4. `suspects` 有值时才显示"报告点名了这些模组"，`inferredPackages` 单独
   一块并明确标"推断"。
5. `text` 已经是脱敏后的原文，可以直接放进 `<pre>` 里滚动显示，
   不需要前端再脱敏一遍。
