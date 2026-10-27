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

---

## 24. 诊断包（V2.5 新增）

玩家说"我的游戏坏了"，最麻烦的从来不是修，而是先把材料收齐：日志在哪、崩溃报告是哪一份、
Java 是不是对、Mod 有没有冲突、上次安装卡在哪一步。这些信息后端其实都已经能拿到，
但散在 7 个命令里，前端要一个个点、一个个贴，玩家还得自己压缩成 zip。

`support.rs` 把这件事做成一个动作：**打一个包**。

### 24.1 两个命令

| 命令 | 作用 | 会写盘吗 |
|---|---|---|
| `plan_diagnostic_bundle` | 预览：包里会有哪几节、各多少个文件、多大、有没有问题 | 不会 |
| `export_diagnostic_bundle` | 真的生成 zip | 会 |

```ts
interface BundleSection {
  key: string;          // report / instance / logs / crash / mods / java / tasks
  label: string;        // 给玩家看的中文名
  files: number;
  bytes: number;
  redactions: number;   // 这一节里脱敏了多少处
  note?: string | null; // 为什么是空的、为什么被截断
}

interface BundlePlan {
  instanceId: string;
  instanceName: string;
  fileName: string;
  targetPath: string;
  exists: boolean;         // 同名包已存在，导出会覆盖
  sections: BundleSection[];
  estimatedBytes: number;
  freeBytes: number | null;
  issues: string[];        // 人话提示
  blocking: boolean;       // true = 导不出来
  collectedAt: string;
}

interface BundleResult {
  path: string;
  bytes: number;
  sections: BundleSection[];
  redactions: number;
  warnings: string[];
}
```

```ts
// 1) 先预览，把 sections 摊开给玩家看
const plan = await invoke<BundlePlan>("plan_diagnostic_bundle", {
  instanceId: id,
});
if (plan.blocking) {
  showIssues(plan.issues);
  return;
}

// 2) 确认后再导出（日志量大，这一步会慢几秒）
const result = await invoke<BundleResult>("export_diagnostic_bundle", {
  instanceId: id,
  includeLogs: true,
});
showToast(`诊断包已生成：${result.path}（脱敏 ${result.redactions} 处）`);
```

`includeLogs = false` 时包里不带日志，`report` 里会**明说**"这次没有收集日志"，
不会让拿到包的人以为日志是空的。

### 24.2 包里有什么

| 小节 | 内容 | 上限 |
|---|---|---|
| `report` | 一份中文说明：实例信息、体检结论、Java、Mod 扫描、崩溃诊断 | — |
| `instance` | `instance.json`、`options.txt` 等实例元数据 | — |
| `logs` | 最近 5 份日志，每份最多读 512 KB（留尾部） | 5 份 |
| `crash` | 最近 2 份崩溃报告 | 2 份 |
| `mods` | `mods/` 的文件清单（只有名字和大小，不含 jar 内容） | — |
| `java` | Java 检查结果 | — |
| `tasks` | 最近的任务记录 | — |
| `bundle.json` | 机器可读的那一份，字段稳定，留给以后接 AI 分析 | — |

整个包上限 **32 MB**，超了 `plan` 会标 `blocking`，`export` 直接报
`INVALID_VALUE`。生成位置固定在 `<游戏目录>/diagnostics/astral-diag-<实例 id>-<时间>.zip`，
先写 `.zip.part` 再改名，中途失败不会留半个包。

### 24.3 脱敏在本地做完，不上传

这一点必须说清楚，因为它关系到玩家敢不敢点这个按钮：

- 日志、崩溃报告、`bundle.json` 里的文本，**全部**先过一遍 `logs::redact`
  （token、邮箱、IPv4、`\Users\<名字>`），再补两处它管不到的：
  游戏目录本身换成 `<game>`、玩家自己起的离线用户名换成 `<user>`。
- 顺序是先换路径再脱敏。反过来的话，`redact` 会先把路径里的用户名换成 `<user>`，
  整条路径就跟真实目录对不上了，后面再想替换就找不到 —— 这个顺序有专门的回归测试盯着。
- 每一节的 `redactions` 是实数，前端可以直接显示"已在本机脱敏 N 处"。
- 后端**没有**任何上传代码。包就在本地，给不给别人由玩家决定。

### 24.4 前端接入建议

1. 崩溃诊断页 / 设置页放一个"生成诊断包"按钮，先调 `plan` 弹预览，
   把 `sections` 列成清单（哪几节、多少文件、多大），`blocking` 时只显示问题不让点确认。
2. 导出后不要自己拼路径，直接用 `result.path`，并给一个"打开所在文件夹"的按钮。
3. `warnings` 里有内容时原样显示 —— 比如"日志太大只留了尾部"，
   这种信息比一个成功提示有用。
4. 想要"发给别人"时，前端只负责把 `result.path` 交给系统（复制路径 / 打开资源管理器），
   不要自己做上传。

---

## 25. 启动前预检（V2.5 新增）

点"启动"之前想知道"现在能不能跑、哪里会出问题"，原来要把体检、Java 检查、Mod 扫描
挨个点一遍。`preflight_launch` 一次把该看的都看完，**只看不改、也不启动游戏**。

### 25.1 命令

```ts
const pf = await invoke<Preflight>("preflight_launch", { instanceId: id });
if (!pf.ready) {
  showBlockers(pf.blockers);
  return;
}
await invoke("start_launch", { instanceId: id }); // 原有的启动命令，行为不变
```

```ts
interface PreflightCheck {
  key: string;          // running | tasks | username | files | java | memory | mods
  label: string;        // 给玩家看的中文名
  ok: boolean;
  blocking: boolean;    // 不通过时是否挡住启动
  detail: string | null;
}

interface MemoryAdvice {
  systemTotalMb: number;
  systemFreeMb: number;
  instanceMb: number | null;   // 实例当前设置，null = 用默认
  recommendedMb: number;
  minMb: number;
  maxMb: number;
  ok: boolean;
  note: string;                // 为什么是这个数，可直接显示
}

interface Preflight {
  instanceId: string;
  instanceName: string;
  gameVersion: string;
  ready: boolean;              // 没有任何 blocking 项
  checks: PreflightCheck[];
  memory: MemoryAdvice;
  blockers: string[];          // 挡住启动的项（中文，可直接列给玩家）
  collectedAt: string;
}
```

### 25.2 七项检查与是否拦启动

| key | 查什么 | 拦启动 |
|---|---|---|
| `running` | 这个实例是不是已经在跑 | 是 |
| `tasks` | 有没有安装/更新任务还没结束 | 是 |
| `username` | 用户名是否合法（3–16 位字母数字下划线） | 是 |
| `files` | 版本文件在不在、完不完整（复用 `verify::check`） | 是 |
| `java` | Java 能不能用、版本对不对（复用 `java_audit::check_instance`） | 是 |
| `memory` | 内存设置是否落在合理区间 | **否**，只提示 |
| `mods` | `mods/` 里有没有会挡住启动的问题（复用 `modscan::scan`） | **否**，只提示 |

后两项不拦，是因为它们只是"可能出问题"：内存偏小照样能进游戏，Mod 报 error
也有玩家愿意自己扛。真正的硬条件（没装版本、Java 不对、实例在跑）才拦。

### 25.3 内存建议是怎么算的

不是拍一个 4096，而是按机器和内容推：

- 原版起步 2048 MB；
- 有加载器 +1024 MB；
- 每 25 个 Mod +512 MB，最多 +2048 MB（100 个封顶）；
- 上限 = min(系统内存 / 2, 系统内存 − 2048)，再夹一个 8192 MB 的绝对上限
  —— 给 Java 太多，Windows 自己就没内存了；
- 结果按 512 MB 取整，夹在 [1024, 上限] 之间。

`note` 会把"为什么是这个数"直接写成人话，前端不用自己解释。

### 25.4 前端接入建议

1. 启动按钮点下去先调 `preflight_launch`；`ready === true` 就直接启动，
   别让玩家多点一次。
2. `ready === false` 时把 `blockers` 列出来，并在对应项旁边给一个跳转
   （`files` → 检查修复页、`java` → 运行库页、`running` → 提示先关游戏）。
3. `memory.ok === false` 但 `blocking === false` 时，用一条可关闭的提示条显示
   `memory.note`，别做成弹窗打断启动。
4. 预检**不写任何文件**（日志、缓存、诊断目录都不会多出来），可以放心在
   鼠标悬停或页面切换时提前调一次。

---

## 26. 归档日志 `.gz`（V2.5 变更）

### 26.1 变了什么

Minecraft 会把旧日志自己滚动压缩成 `2026-10-01-1.log.gz`。以前 `list_logs` 直接跳过
这些文件，玩家点"历史日志"只能看到当前那份 `latest.log`，前几天的全不见了。

现在 `.log.gz` / `.txt.gz` 会正常出现在列表里，并带一个 `compressed: true` 标记。

```ts
export interface LogFile {
  id: string;
  name: string;
  kind: "game" | "crash" | "launcher";
  sizeBytes: number;
  modifiedAt: string;
  /** 新增：是不是 .gz 归档。这类文件不能按字节偏移追。 */
  compressed: boolean;
}
```

`sizeBytes` 仍然是**压缩后**的磁盘占用（和资源管理器里看到的一致），
不是解压后的大小 —— 解压后多大要读了才知道。

### 26.2 读归档日志

`read_log` 用法完全不变，不需要前端区分：

```ts
const log = await invoke<LogText>("read_log", {
  instanceId,
  logId,
  limit: 400,
});
```

后端边解压边只留尾部，所以：

- 只保留最后 `MAX_READ`（4 MiB）解压内容，再按 `limit` 取最后 N 行；
- 解压总量超过 64 MiB 直接报
  `INVALID_VALUE|日志解压后超过 64 MB，没法整份打开` —— 这是解压炸弹兜底；
- 从中间截断时会把开头那半行丢掉，不会出现半截时间戳。

### 26.3 实时跟随不支持归档

`tail_log` 对 `.gz` 会明确报错，而不是给出乱码：

```
INVALID_VALUE|这是归档日志，不能实时跟随，请直接打开查看
```

原因：gzip 是流式压缩，第 N 字节的偏移和原文对不上，做不了增量读取。
前端拿到这个错误应该退化成一次性打开（`read_log`），并且**不要继续轮询**。

`tail_log` 不传 `logId` 时自动挑"最新的游戏日志"，挑选时会跳过归档文件
（按真实 mtime 比，不是按字符串比）。

### 26.4 前端接入建议

1. 列表里 `compressed === true` 的项加一个小标记（比如一个压缩包图标），
   让玩家知道为什么这份日志点开后不能往下滚。
2. 打开归档日志时不要进"实时跟随"模式，直接走静态查看。
3. 归档日志一般很大，默认 `limit` 用 400 就够，别一上来要 2000 行。

---

## 27. 下载速度与剩余时间（V2.5 新增）

### 27.1 任务对象新增两个字段

`astral://task` 事件里的任务对象（以及 `list_tasks` / `get_task` 的返回值）多了两个字段：

```ts
interface Task {
  // ……原有字段不变……
  /** 最近一次测得的下载速度，字节/秒。没在下载时是 0 */
  speedBps: number;
  /** 按当前速度估算的剩余毫秒数；速度或总量未知时是 null */
  etaMs: number | null;
}
```

同时 **`bytes` / `totalBytes` 现在真的有值了**。这两个字段以前建好任务就一直是
0，界面上只能看到"第 3/12 个文件"，一个大文件下十分钟数字不动，玩家以为卡死了。

### 27.2 怎么算的

- 每次进度回调带上来 `(已完成个数, 总个数, 已下载字节, 总字节)`；
- 两次采样的间隔 **≥ 200 ms** 才重新算速度：`速度 = 字节增量 / 时间增量`；
- 间隔更短的采样**沿用上一次的速度** —— 网络抖一下数字就乱跳，比不显示更糟；
- 字节数比上次**变小**（换文件了、重试了）→ 重新开始采样，速度清零；
- `etaMs` 只在"速度 > 0 且 总字节 > 已下载"时给值，否则是 `null`。**总量未知时
  不瞎报剩余时间**。

### 27.3 前端怎么用

```ts
function formatSpeed(bps: number): string {
  if (bps <= 0) return "";
  if (bps < 1024) return `${bps} B/s`;
  if (bps < 1024 * 1024) return `${(bps / 1024).toFixed(0)} KB/s`;
  return `${(bps / 1024 / 1024).toFixed(1)} MB/s`;
}

function formatEta(ms: number | null): string {
  if (ms == null) return "";
  const s = Math.round(ms / 1000);
  if (s < 60) return `还剩 ${s} 秒`;
  const m = Math.floor(s / 60);
  return `还剩 ${m} 分 ${s % 60} 秒`;
}
```

建议放在进度条右侧，速度用次要色、ETA 用更浅的灰；两者都为空时**整块隐藏**，
不要显示 "0 B/s" 或 "--"。字节进度条用 `bytes / totalBytes`，文件进度条继续用
`done / total` —— 大文件场景下前者平滑得多。

### 27.4 哪些链路已经带上字节数

安装游戏本体、启动前补齐、安装加载器（Forge/NeoForge/Fabric/Quilt）、整合包安装、
单个资源安装（含依赖）、Mod 更新、修复实例，全部改走 `progress_bytes`。
整合包的文件下载还把整包总字节算出来（可选文件不计入），所以能显示整包进度。

唯一没带字节的是纯覆盖/解压阶段（`extract`），那一步没有下载，速度是 0、
ETA 是 null，属于预期。
---

## 28. 下载源测速（V2.5 新增）

### 28.1 为什么要这个

配置里的 `source` 一直是玩家自己选的（`official` / `bmclapi`），但没人知道
自己该选哪个。国内连 Mojang 有时几百毫秒、有时直接连不上，而镜像偶尔又会
慢得离谱 —— 结果"下载慢"变成玄学。

`probe_sources` 对每个下载源做几次 TCP 连接，量往返时间，然后给一句建议。

### 28.2 命令

```ts
import { invoke } from "@tauri-apps/api/core";

interface SourceProbe {
  source: string;        // 配置里 source 该填的值："official" | "bmclapi"
  label: string;         // 给玩家看的名字
  host: string;          // 测的主机名
  reachable: boolean;    // 连得上就是 true（只要有一次成功）
  dnsMs: number | null;  // DNS 解析耗时，取最快的一次
  connectMs: number | null;
  samples: number;       // 实际尝试了几次
  error: string | null;  // 连不上时的原因
}

interface ProbeReport {
  sources: SourceProbe[];
  suggested: string | null;  // 建议用的源；全都不通时是 null
  note: string;              // 一句话说清测的是什么
}

const report = await invoke<ProbeReport>("probe_sources");
```

### 28.3 它到底测了什么，没测什么

**测的**：DNS 解析耗时、TCP 连接耗时。两者分开计时，因为"解析慢"和
"连接慢"是两种完全不同的毛病，混成一个数字就看不出问题在哪。

**没测**：下载速度。每个源都得下一份几 MB 的东西，玩家点一下测速就要多耗
几十 MB 流量，而且慢速下载很容易把界面卡住。连接延迟已经足够区分
"这个源通不通、快不快"。

返回值里的 `note` 就是给界面直接显示这句话用的，别删。

### 28.4 建议是怎么给的

1. 全都不通 → `suggested` 为 `null`，界面提示"检查网络"
2. 官方源可达、且和最快的那个差距不到 20 ms → 建议官方源
   （镜像的内容可能比官方晚几小时到几天，延迟差不多时没理由用它）
3. 否则建议最快的那个

### 28.5 前端接入建议

- 设置页"下载源"旁边放一个"测速"按钮，点了调 `probe_sources`
- 每行显示 `label` + `connectMs`（连不上显示 `error`），`suggested` 那行加个标记
- 测速要花几秒（3 次采样 × 3 秒超时，最坏情况），按钮期间要禁用并显示"测速中"
- **测速不写配置**。要不要切换由玩家自己决定，后端不擅自改
- 结果不要缓存，玩家网络随时在变

### 28.6 和前端的既有字段的关系

`list_versions_cached` 返回的 `cache.source` 说的是"这份数据从网络还是缓存来的"，
和这里的下载源 `source` 是两回事，别混。
---

## 29. 本地文件导入（V2.5 新增）

### 29.1 解决什么问题

玩家手上经常已经有一个从群里、从视频简介里存下来的 jar，或者一个资源包 zip。
以前只能自己打开 `instances/<id>/mods/` 把文件拷进去 —— 大多数玩家根本找不到
这个目录，更别说判断一个 zip 到底是资源包还是光影包。

现在可以「把文件拖进来 → 先看预览 → 确认导入」。

### 29.2 两个命令

```ts
interface ImportCandidate {
  path: string;              // 玩家给的原始路径
  fileName: string;
  sizeBytes: number;
  kind: "mod" | "resourcepack" | "shaderpack" | "modpack" | "datapack" | "unknown";
  targetDir: string | null;  // 会落到哪个目录
  targetName: string | null; // 会用的文件名（重名时这里还是原名）
  note: string | null;       // 给人看的补充说明
  problem: string | null;    // 不能导入的原因
}

interface ImportOutcome {
  path: string;
  fileName: string;
  kind: string;
  savedAs: string | null;    // 真正落地的文件名（重名时带序号）
  targetPath: string | null;
  sizeBytes: number;
  sha1: string | null;       // 复制后回读算的，可以拿去 Modrinth 反查
  renamed: boolean;
  error: string | null;
}

// 只读预览：不复制任何文件
const candidates = await invoke<ImportCandidate[]>("inspect_local_files", {
  instanceId,
  paths: ["D:\\下载\\sodium.jar", "D:\\下载\\好看的材质.zip"],
});

// 确认后真正导入：一个失败不影响其他文件
const outcomes = await invoke<ImportOutcome[]>("import_local_files", {
  instanceId,
  paths,
  kind: null,   // 可选：强制指定类型
});
```

### 29.3 类型是按内容判断的，不是按扩展名

`.zip` 可能是资源包、光影包、数据包，甚至整合包。判断顺序：

1. `.mrpack` → 整合包，提示走「导入本地整合包」
2. `.jar` → Mod
3. zip 里有 `modrinth.index.json` → 整合包，同上
4. zip 里有 `shaders/` → 光影包
5. zip 里有 `pack.mcmeta` + `assets/` → 资源包（顺带读 `pack_format` 给出适配版本）
6. zip 里有 `pack.mcmeta` + `data/` → 数据包（提示要放进存档的 `datapacks/`）
7. 其余 → `unknown`，`problem` 里写清为什么

`inspect_local_files` 和 `import_local_files` 用的是同一套判断，所以预览说会放到
`resourcepacks/`，导入就一定放到那里。

### 29.4 安全与不丢数据的约定

- **重名不覆盖**：自动改成 `-1`、`-2`……不会悄悄盖掉玩家已有的文件
- 先写 `.astral-import` 临时文件再改名，中途失败不会留下半个 jar 让游戏加载崩
- 复制后回读大小并比对，不一致直接报错
- 单个文件上限 512 MB，一批最多 64 个
- 拒绝符号链接
- **不碰** `options.txt`、`config/`、存档

### 29.5 前端接入建议

- 接 Tauri 的拖放事件（`onDragDropEvent`）拿到 `paths`，先调 `inspect_local_files` 出预览
- 预览里按 `kind` 分组显示，`problem` 非空的置灰并显示原因
- 玩家点「导入」后调 `import_local_files`，按 `error` 逐条显示结果
- `sha1` 可以顺手拿去调 Modrinth 的 `/version_file/{sha1}` 反查是哪个项目，
  查到了就能显示图标、名字，还能进账本（见 §13）
- 导入完成后刷新 `list_mods` / `list_packs`

## 30. 存档导出 / 导入（发给别人、收别人的）

前面的 §19 是「自己给自己留后路」的备份（存在实例的 `.astral-worlds/` 里，玩家看不到）。
这一节是**给文件**：导出一个 `.zip` 发到群里，或者把别人给的 `.zip` 收进 `saves/`。

### 30.1 命令一览

| 命令 | 作用 | 写文件 |
|---|---|---|
| `inspect_world_zip` | 看一眼这个 zip 是不是存档、解压多大、建议叫什么 | 否 |
| `export_world_zip` | 把一个存档导出成 zip | 是 |
| `import_world_zip` | 把 zip 收进实例的 `saves/` | 是 |

```ts
export interface WorldZipInfo {
  path: string;
  fileName: string;
  bytes: number;                 // zip 本身多大
  rootDir: string | null;        // 包里最外层只有一个目录时，那个目录名
  suggestedName: string;         // 建议的存档名（rootDir，没有就用 zip 文件名）
  hasLevelDat: boolean;          // 包根有没有 level.dat
  fileCount: number;
  totalBytes: number;            // 解压后大概多大
  problem: string | null;        // 非空表示不能导入，直接显示这句
}

export interface WorldExport {
  path: string;
  worldId: string;
  worldName: string;
  bytes: number;                 // 导出后的 zip 大小
  fileCount: number;
}

export interface WorldImport {
  worldId: string;
  worldName: string;
  replaced: string | null;       // 覆盖前留底的目录名（没覆盖时为 null）
  files: number;
  bytes: number;
}
```

### 30.2 导出

```ts
// 1) 玩家点「导出存档」→ 先让他选存到哪（Tauri dialog 的 save 面板）
const destPath = await save({ defaultPath: `${worldName}.zip`, filters: [{ name: "Zip", extensions: ["zip"] }] });
if (!destPath) return;

// 2) 导出
const r = await invoke<WorldExport>("export_world_zip", {
  instanceId,
  worldId,
  destPath,
  overwrite: false,   // 目标已存在时传 false 会报 FILE_EXISTS，问过玩家再传 true
});
toast.success(`已导出到 ${r.path}（${(r.bytes / 1048576).toFixed(1)} MB）`);
```

导出包里是存档目录的**内容**（`level.dat`、`region/`、`playerdata/`……），
不再套一层目录 —— 这样别的启动器、甚至玩家自己解压后手动放进 `saves/` 都能用。

### 30.3 导入

```ts
// 1) 玩家选了一个 zip，先预览
const info = await invoke<WorldZipInfo>("inspect_world_zip", { path: zipPath });
if (info.problem) {
  // 例如「包里没有 level.dat，看起来不是一个存档」
  return showError(info.problem);
}
// info.suggestedName 填进输入框，玩家可以改

// 2) 确认后导入
const r = await invoke<WorldImport>("import_world_zip", {
  instanceId,
  sourcePath: zipPath,
  name: nameInput || null,   // 空就用 suggestedName
  overwrite: false,
});
// r.replaced 非空说明覆盖了旧存档，旧的那个改名留在了 saves/ 里
```

### 30.4 两种包写法都认

玩家从网上下到的存档包有两种常见写法：

- `MyWorld/level.dat` —— 整个文件夹打包（最常见）
- `level.dat` 直接在最外层 —— 只打了文件夹里的内容

`inspect_world_zip` 会自动判断：`rootDir` 非空说明是第一种，导入时会把那一层剥掉。
`hasLevelDat` 是**剥掉公共目录之后**在包根找到 `level.dat` 的结果。

### 30.5 安全与不丢数据

- **游戏运行中一律拒绝**导出和导入：存档正在被写，导出来是不一致的；导入则会在
  游戏退出时被覆盖
- 导出先写 `<目标>.part`，全部写完才改名 —— 中途失败不会留下半个包
- 导入逐条校验路径（防 Zip Slip）、拒绝符号链接条目、按实际写出的字节算总量
- 解压到 `saves/.import-<纳秒>` 暂存，成功才 rename 到目标位置
- 目标存档已存在时**默认拒绝**；`overwrite: true` 时先把旧的改名成
  `<名字>-replaced-<纳秒>` 留底，导入失败会改回去
- 全程不删除玩家的东西

### 30.6 前端接入建议

- 「存档」页面每个存档卡片加一个「导出」按钮；顶部加一个「导入存档」按钮
- 导入走「选文件 → 预览（显示包里有几个文件、解压多大、建议叫什么）→ 确认」
- `problem` 非空时不要让玩家点确认，直接把那句话显示出来
- 覆盖时用二次确认弹窗，并说明「旧存档会保留一份，不会删除」
- 导入成功后刷新存档列表（`list_saves` / `get_world_info`）

## 31. 游戏服务器列表导入（V2.5 新增）

玩家多半已经在游戏里存了一堆服务器（多人游戏列表）。启动器这边另有一份收藏列表（§16），
两边不通气的话，玩家得把地址一个个重新输一遍。这一节把它接上。

**只读，不写 `servers.dat`。** 那是游戏自己的内部格式（未压缩 NBT），我们读出来给界面看，
玩家点了才写进启动器自己的 `.astral-servers.json`。不碰游戏文件一个字节——写坏了游戏会丢列表。

### 31.1 命令一览

| 命令 | 参数 | 返回 |
|---|---|---|
| `scan_game_servers` | `instanceId` | `GameServerScan` |
| `import_game_servers` | `instanceId`, `addresses: string[]` | `ImportResult` |

### 31.2 类型

```ts
/** 游戏 `servers.dat` 里的一条 */
interface FoundServer {
  /** 游戏里显示的名字，可能是空的 */
  name: string;
  /** `host` 或 `host:port` */
  ip: string;
  /** 游戏里被「隐藏」的服务器 */
  hidden: boolean;
}

interface GameServerScan {
  /** `servers.dat` 的完整路径（界面上可以显示出来，让玩家知道读的是哪个文件） */
  path: string;
  /** 文件在不在 */
  exists: boolean;
  /** 读出来的服务器，按游戏里的顺序 */
  found: FoundServer[];
  /** 读不出来时说明原因；文件不存在不算错，这时是 null */
  note: string | null;
}

interface ImportResult {
  /** 这次新收进来的（就是 `ServerEntry`，见 §16） */
  added: ServerEntry[];
  /** 没进来的，每条一句话说明原因，比如「play.example.com：这个服务器已经在列表里了」 */
  skipped: string[];
}
```

### 31.3 调用示例

```ts
// 打开「从游戏里导入」弹窗时先扫一遍
const scan = await invoke<GameServerScan>("scan_game_servers", {
  instanceId: current.id,
});
if (!scan.exists) {
  showEmpty("这个实例还没有在游戏里加过服务器");
} else if (scan.note) {
  showWarning(scan.note); // 读不出来，但页面不崩
} else {
  showList(scan.found); // 勾选框列表，hidden 的可以默认不勾
}

// 玩家勾完点「导入」
const r = await invoke<ImportResult>("import_game_servers", {
  instanceId: current.id,
  addresses: checked.map((f) => f.ip),
});
toast(`收进 ${r.added.length} 个`);
if (r.skipped.length) showSkipped(r.skipped);
```

### 31.4 行为约定

- **按 `ip` 字段匹配。** 前端把玩家勾选的那些 `ip` 原样传回来，后端在扫描结果里找对应的那一条，
  用它的名字（空的就用地址）建收藏项。
- **重复不算失败。** 已经在收藏里的会进 `skipped` 并说明原因，同批其它照常收进来。
  一批里有一条重复不该让整批失败。
- **地址不存在也不算失败。** 如果传了一个扫描结果里没有的 `ip`（比如界面上的数据过期了），
  同样进 `skipped`。
- **`hidden` 只是给界面参考。** 游戏里被隐藏的服务器照样能导入，界面上可以默认不勾。
- **没勾任何一条** 会报 `INVALID_VALUE|没有勾选任何服务器`，不是静默成功。

### 31.5 为什么不做「双向同步」

写 `servers.dat` 需要完整的 NBT 写入 + 游戏版本之间格式差异的兼容，一旦写坏玩家的多人列表就全丢了。
收益（省一次点击）远小于风险。所以这里只读；玩家想同步回去，就在游戏里手动加一次。

### 31.6 前端接入建议

- 「服务器」页面顶部加一个「从游戏导入」按钮，点了先 `scan_game_servers`
- `!exists` → 提示「这个实例还没在游戏里加过服务器」，按钮置灰
- `note` 非空 → 黄色提示条显示那句话，列表为空
- 列表按 `hidden` 分组，隐藏的默认不勾
- 导入成功后刷新收藏列表（`list_servers`），并提示 `skipped` 里的内容

## 32. 磁盘占用分析（V2.5 新增）

一个整合包实例动辄几 GB，`assets` 又单独几 GB，缓存、备份、回收站里还各躺着一堆
看不见的东西。玩家只会看到「C 盘红了」，不知道该删什么。

`analyze_disk_usage` 把游戏目录按类量一遍，再按实例拆开，告诉界面这几个 GB 花在哪。
**只读**，不删任何东西——真正的清理仍然走各自已有的命令，每个都有自己的确认要求。

### 32.1 命令

| 命令 | 参数 | 返回 |
|---|---|---|
| `analyze_disk_usage` | 无 | `DiskReport` |

和旧的 `disk_usage` 的区别：旧命令只给几个顶层文件夹的总数（`folders` 是一个
`Record<string, number>`）；这个会按实例拆开、列最大文件、给出可清理标记和提醒。
两个都留着，旧的不动。

### 32.2 类型

```ts
interface DirUsage {
  /** 相对游戏目录的路径，"" 表示根 */
  path: string;
  /** 界面显示用的名字，比如「下载缓存」 */
  label: string;
  /** assets | libraries | versions | instances | cache | java | logs | meta
   *  或实例内部的 mods | saves | config | resourcepacks | shaderpacks
   *  | screenshots | logs | backups */
  kind: string;
  bytes: number;
  files: number;
  exists: boolean;
  /** 数到上限了，bytes 是下限不是准确值 */
  truncated: boolean;
  /** 用对应命令清理不会丢玩家数据 */
  safeToClean: boolean;
  /** 一句话说明这是干什么的 */
  note: string;
}

interface FileUsage {
  /** 相对实例目录 */
  path: string;
  bytes: number;
}

interface InstanceUsage {
  id: string;
  name: string;
  gameVersion: string;
  bytes: number;
  files: number;
  /** 只列存在的子目录 */
  parts: DirUsage[];
  /** 最大的 5 个文件 */
  largest: FileUsage[];
}

interface DiskReport {
  gameDir: string;
  totalBytes: number;
  /** 按大小倒序 */
  categories: DirUsage[];
  /** 按大小倒序 */
  instances: InstanceUsage[];
  freeBytes: number | null;
  totalDiskBytes: number | null;
  /** 值得提醒的话，不是错误 */
  hints: string[];
  /** 数到上限了，报告不完整 */
  truncated: boolean;
}
```

### 32.3 调用示例

```ts
const r = await invoke<DiskReport>("analyze_disk_usage");

// 顶部：总量与磁盘剩余
showTotal(r.totalBytes, r.freeBytes, r.totalDiskBytes);

// 分类条形图，safeToClean 的项才放「清理」按钮
for (const c of r.categories) {
  renderBar(c.label, c.bytes, c.safeToClean, c.note);
}

// 实例列表：点开看 parts 与 largest
for (const i of r.instances) {
  renderInstance(i.name, i.bytes, i.parts, i.largest);
}

// hints 用黄色提示条，别当错误弹窗
for (const h of r.hints) showHint(h);
```

### 32.4 行为约定

- **只读。** 扫完目录里一个文件都不会少。想清理就调 `cache_clear` /
  `purge_trash` / `prune_mod_update_backups` / `remove_managed_java` 等各自的命令，
  每个都有自己的确认要求。
- **`safeToClean` 是诚实的。** `cache` / `meta` / `logs` / `jvm` / `screenshots` /
  实例内的 `backups` 是 `true`；`assets` / `libraries` / `versions` / `instances` /
  `saves` / `mods` / `config` 一律 `false`。宁可让界面少放一个按钮，也不能让玩家
  一键把存档删了。
- **只统计托管实例。** 兼容实例（`versions/<ver>/` 里现成的版本）不在这里算，
  它们和共享的 `versions` 目录重叠，重复算会让总数虚高。
- **符号链接与 Windows 重解析点不跟。** 否则可能绕圈或者跳到别的盘上。
- **有上限。** 单个目录最多进 8 层、最多数 40 万个文件。到上限时 `truncated` 为
  `true`，这时 `bytes` 是下限——界面上应该说「至少 X GB」，不要说成准确值。
- **`hints` 不是每次都念。** 只在磁盘剩不到 2 GB、缓存超过 1 GB、某个实例超过 4 GB、
  更新备份超过 512 MB 时才会出现。

### 32.5 前端接入建议

- 「存储管理」页面进来先调一次，别做成每次切页面都扫（大目录要几百毫秒）
- 扫描期间显示「正在统计…」，用 skeleton 占位
- 分类用横向条形图，实例用可展开的列表
- `safeToClean` 为 `false` 的分类，按钮换成「查看」，点了跳转到对应页面（比如实例列表）
- `truncated` 为 `true` 时在页面底部加一行小字说明数字是下限

---

## 33. AI 崩溃诊断（BYOK，V2.5 新增）

项目立项时的差异化功能：本地规则先跑一遍，剩下看不懂的再交给大模型。**密钥由玩家自己提供**，
存在本机，不上传、不进前端。

### 33.1 六个命令

| 命令 | 作用 | 会不会花钱 |
| --- | --- | --- |
| `ai_key_status` | 查密钥状态（有没有配、末四位、模型名、加密方式） | 不会 |
| `ai_set_key` | 存密钥（先校验格式，再用 Windows DPAPI 加密） | 不会 |
| `ai_clear_key` | 删密钥，顺带清掉分析缓存 | 不会 |
| `ai_preview` | 生成「将要发送的内容」预览 | 不会 |
| `ai_analyze` | 真的发出去分析 | **会** |
| `ai_clear_cache` | 清掉分析结果缓存，返回清掉几条 | 不会 |

### 33.2 类型

```ts
interface KeyStatus {
  configured: boolean;
  /** 末四位，形如 "…6789"；没配时为 null */
  hint: string | null;
  /** 固定是 "deepseek-chat" */
  model: string;
  /** 界面上说明用，如 "Windows DPAPI（绑当前用户…）" */
  storage: string;
}

interface AiPreview {
  instanceId: string;
  /** 参与分析的日志名 */
  logs: string[];
  /** 本地规则已经命中的结论（也会一起发给模型，减少幻觉） */
  findings: string[];
  /** 真正要发送的文本（已脱敏、已裁剪） */
  prompt: string;
  /** 发送的字符数 */
  chars: number;
  /** 是否因为太长被截断 */
  truncated: boolean;
  /** 脱敏了多少处 */
  redactions: number;
  /** 发送前必须让玩家知道的话 */
  warnings: string[];
  model: string;
  /** 目标地址，写死在 Rust 里，前端传不了 */
  endpoint: string;
}

interface AiCause {
  title: string;
  detail: string;
  /** "high" | "medium" | "low" */
  confidence: string;
}

interface AiStep {
  title: string;
  detail: string;
}

interface AiAnalysis {
  summary: string;
  confidence: string;
  causes: AiCause[];
  steps: AiStep[];
  needMore: boolean;
}

interface AiResult {
  analysis: AiAnalysis;
  model: string;
  /** "network" 这次真的请求了；"cache" 命中了上次的结果 */
  source: "network" | "cache";
  /** 本地规则命中了几条 */
  ruleFindings: number;
  promptChars: number;
  truncated: boolean;
  redactions: number;
  /** 模型报的 token 用量，拿不到时为 null */
  promptTokens: number | null;
  completionTokens: number | null;
  elapsedMs: number;
  /** ISO 时间 */
  at: string;
}
```

### 33.3 调用示例

```ts
// 1) 进来先看有没有配
const status = await invoke<KeyStatus>("ai_key_status");
if (!status.configured) showSetupHint();

// 2) 存密钥（只在设置页做）
await invoke<KeyStatus>("ai_set_key", { key: "sk-xxxxxxxx" });

// 3) 点「分析崩溃」→ 先出预览，让玩家看清楚要发什么
const preview = await invoke<AiPreview>("ai_preview", {
  instanceId,
  logIds: [latestLogId],
  maxLines: 300,
});
showPreviewDialog(preview);

// 4) 玩家点「确认发送」才真的走网络
const result = await invoke<AiResult>("ai_analyze", {
  instanceId,
  logIds: [latestLogId],
  maxLines: 300,
});
renderAnalysis(result);
```

### 33.4 行为约定（前端必须知道的）

- **密钥永远不进前端。** `ai_key_status` 只回末四位；`ai_preview` / `ai_analyze` 的返回值里
  没有任何密钥字段。前端也不该把密钥存到 `localStorage`。
- **DPAPI 绑当前 Windows 用户。** 换了用户或换了机器，旧密钥解不开——这时候
  `ai_key_status.configured` 会是 `false`，界面应该提示重新填，而不是报错崩掉。
- **先预览再发送。** 这是隐私底线：日志里可能有服务器地址、玩家名，玩家有权先看一眼。
  界面上「发送」按钮不要做成默认高亮。
- **没配 Key 时 `ai_analyze` 报 `AUTH_REQUIRED`。** 前端应该引导去设置页，不要显示
  「分析中…」然后超时。
- **`source === "cache"` 说明没花钱。** 同样的日志 + 同样的模型会命中上次结果（最多存 20 条）。
  界面上可以标一句「上次的分析结果」。
- **模型乱答时明确报错。** 返回不是 JSON、或者 JSON 里没有任何结论，都会报 `INTERNAL`。
  不会编一个结论糊弄玩家。
- **`redactions` / `truncated` 要显示出来。** 「已隐藏 3 处敏感信息」「日志太长，只发了最后
  24000 字」这两句话能让玩家信任这个功能。
- **`endpoint` 写死在 Rust 里**（`https://api.deepseek.com/chat/completions`），前端传不了，
  也不该让玩家改——改了就等于把密钥发给别人。

### 33.5 错误码

| 场景 | 错误码 | 前端该做什么 |
| --- | --- | --- |
| 没配 Key | `AUTH_REQUIRED` | 引导到设置页填 Key |
| Key 格式不对 | `INVALID_VALUE` | 就地提示，不要清空输入框 |
| 网络不通 / 超时 | `NETWORK` | 提示稍后再试，日志内容还在 |
| 401 | `NETWORK` | 「API Key 不对或已失效」，引导重填 |
| 402 | `NETWORK` | 「DeepSeek 账户余额不足」 |
| 429 | `NETWORK` | 「请求太频繁」，建议等一会儿 |
| 5xx | `NETWORK` | 「DeepSeek 服务暂时不可用」 |
| 模型乱答 | `INTERNAL` | 「这次没得到有效结论，可以再试一次」 |

### 33.6 前端接入建议

- `AiDrawer.tsx` 现在按钮是写死 `disabled` 的：把 `disabled` 改成 `!status.configured`，
  文案从「分析服务尚未连接」改成「配置 DeepSeek API Key 后可分析」。
- 「确认发送」对话框里把 `preview.prompt` 放进可滚动的 `<pre>`，`warnings` 用黄色条列在上面。
- 分析结果按 `causes` / `steps` 分两块渲染，`confidence` 用颜色区分，但别把 `low` 画成红色
  （那是「判断不了」，不是「确定有问题」）。
- `elapsedMs` 超过 20 秒时加一句「模型正在思考…」，别让玩家以为卡死。
---

## 34. 结束正在运行的游戏（V2.5 新增）

启动器以前只能启动、不能停止。玩家想换实例只能开任务管理器找 `javaw.exe`，找错了还会
误杀别的东西。这次补上「结束游戏」。

### 34.1 命令

```ts
invoke<StopResult>("stop_game", {
  instanceId: string,
  /** 默认 false：先发关闭消息等游戏自己退；true：直接终止 */
  force?: boolean,
  /** 等多久（毫秒），默认 15000，最长 120000 */
  waitMs?: number,
})
```

### 34.2 返回类型

```ts
interface StopResult {
  sessionId: string;
  instanceId: string;
  pid: number;
  /** "graceful" = 发了关闭消息等它自己退；"force" = 直接终止 */
  method: "graceful" | "force";
  forced: boolean;
  /** 从发出请求到进程消失用了多久 */
  waitedMs: number;
  /** true 说明没停下来，界面必须如实告诉玩家 */
  stillRunning: boolean;
  /** 给玩家看的一句话，可以直接显示 */
  message: string;
}
```

### 34.3 行为约定

- **先礼后兵。** `force` 默认 `false`，只发 `WM_CLOSE`——等于玩家点窗口右上角的 ×，
  Minecraft 会走正常关闭流程（存区块、退服务器）。只有玩家确认「强制结束」才真的终止进程。
- **超时不替玩家做决定。** 15 秒内没退出，返回 `stillRunning: true` 加一句
  「游戏没有在 15 秒内退出（可能在存盘，也可能卡住了）。要强制结束吗？」。
  界面应该弹二次确认，而不是自动升级成强制。
- **只杀自己启动的那个进程。** 动手前后都比对 pid 的**创建时间**（`pid_started_at`）。
  Windows 会复用 pid，只按 pid 杀可能杀掉玩家别的程序。对不上就报 `NOT_FOUND`。
- **玩家自己结束不算崩溃。** 强制结束必然返回非 0 退出码，`astral://game-exit` 事件里
  `crashed` 会是 `false`、`stoppedByUser` 是 `true`。不要弹「游戏崩溃了」。
- **没在跑时明确报错。** `NOT_FOUND|这个实例没有在运行`，不会假装成功。

### 34.4 前端接入建议

- 「启动」按钮在实例运行中时换成「结束游戏」，点一次走 `force: false`。
- 收到 `stillRunning: true` 时弹确认框：「游戏还没退出，强制结束可能丢失未保存的进度。
  确定吗？」确认后才带 `force: true` 再调一次。
- 按钮进 loading 态时显示「正在等待游戏退出…」，因为优雅关闭最多要等 15 秒。
- `astral://game-exit` 事件里现在多了 `stoppedByUser` 字段：为 `true` 时走「已结束」的
  轻提示，别走崩溃诊断流程。

---

## 35. 游戏目录识别（V2.5 新增）

改游戏目录是启动器里最容易"看起来像数据丢了"的操作：手抖打错一个字母，启动器指到空目录，
所有实例、存档、Mod 全都不见了。所以在真正落盘之前，先让玩家看一眼目标目录到底有什么。

### 35.1 命令

| 命令 | 参数 | 返回 |
|---|---|---|
| `inspect_game_dir` | `{ path: string }` | `GameDirReport` |

`set_config` 的行为变化：**`gameDir` 落盘前会先过一遍校验**，不合法直接报错，不会写进配置。
校验规则：

- 空字符串 → `INVALID_VALUE|游戏目录不能为空`
- 相对路径（如 `.minecraft`）→ `INVALID_VALUE|游戏目录要写完整路径，比如 D:\Games\.minecraft`
- 指向一个文件 → `INVALID_VALUE|这个路径是一个文件，不是目录`
- 盘符根（`D:\`）→ `INVALID_VALUE|不能把整个磁盘根目录当游戏目录`

### 35.2 类型

```ts
export interface GameDirReport {
  path: string;
  exists: boolean;
  isDir: boolean;
  /** 真的建得起来、写得进去（会写一个探针文件再删掉） */
  writable: boolean;
  /** 有没有 versions / libraries / assets 这些标志文件夹 */
  looksLikeMinecraft: boolean;
  /** 目录里已有的版本名，最多列 50 个 */
  versions: string[];
  versionCount: number;
  instanceCount: number;
  saveCount: number;
  modCount: number;
  hasLibraries: boolean;
  hasAssets: boolean;
  /** 只在"存在 + 是目录 + 像 MC 目录"时才扫，避免对着一整个盘扫 */
  sizeBytes: number | null;
  freeBytes: number | null;
  /** 和当前配置里的目录是不是同一个（按真实路径比，不按字符串） */
  sameAsCurrent: boolean;
  current: string;
  /** 提示，给玩家看的，可以全部展示 */
  notes: string[];
  /** 非空就说明这个目录不该切过去 */
  blocking: string[];
}
```

### 35.3 调用示例

```ts
import { invoke } from "@tauri-apps/api/core";

const report = await invoke<GameDirReport>("inspect_game_dir", {
  path: "D:\\Games\\.minecraft",
});

// 1. 有 blocking 就别让玩家点"确定"
if (report.blocking.length > 0) {
  showErrors(report.blocking);
  return;
}

// 2. notes 逐条提示（"这个目录还不存在，切换后会新建一个空目录"）
for (const note of report.notes) toast.info(note);

// 3. 让玩家确认一眼
const ok = await confirm(
  `这个目录里有 ${report.versionCount} 个版本、${report.instanceCount} 个实例、` +
    `${report.saveCount} 个存档。切过去吗？`,
);
if (!ok) return;

// 4. 真的切
await invoke("set_config", { config: { ...currentConfig, gameDir: report.path } });
```

### 35.4 行为约定

- **只读**。`inspect_game_dir` 除了写一个探针文件（马上删）之外不改任何东西；不建目录、不移动文件。
- **校验在 `set_config` 里也生效**。前端就算跳过预览直接调 `set_config`，坏路径一样进不去。
- **`writable` 会真的写一下**。只查权限位在 Windows 上不可靠（ACL、只读盘、OneDrive 占位文件都会骗人）。
- **大小只在像 MC 目录时才扫**。玩家可能不小心指到 `D:\`，对着整个盘算大小会卡死界面。
- **`sameAsCurrent` 按真实路径比**。`D:\a\..\a\.minecraft` 和 `D:\a\.minecraft` 算同一个。
- **目录不存在不是错误**。返回 `exists: false` + 一条 note，`blocking` 为空——新建目录是合法用法。

### 35.5 前端接入建议

- 「浏览…」按钮选完目录后立刻调 `inspect_game_dir`，把 `notes` 显示在输入框下面。
- `blocking` 非空时把「确定」按钮置灰，并把 `blocking[0]` 作为红字提示。
- `looksLikeMinecraft === false && exists === true` 时给一个更明显的警告：
  「这个目录里没有 Minecraft 的文件，切过去之后启动器里会是空的」。
- 切换成功后要重新拉 `list_instances` / `list_saves`，因为 `gameDir` 变了。

---

## 36. 整合包更新（V2.5 新增）

装上整合包之后，作者发新版本了启动器原来完全不知道 —— 玩家只能自己回 Modrinth 看一眼。
这两个命令就是补这块：先查有没有新版本，再给一份"会加什么、删什么、要多少空间"的计划。
**两个都是只读的**，真正替换留给后续版本，而且必须先让玩家看过这份计划。

### 36.1 命令

| 命令 | 参数 | 返回 |
|---|---|---|
| `check_modpack_update` | `{ instanceId: string }` | `PackUpdateCheck` |
| `plan_modpack_update` | `{ instanceId: string, versionId: string }` | `PackUpdatePlan` |

### 36.2 类型

```ts
export interface PackUpdateCheck {
  instanceId: string;
  packName: string;
  projectId: string;
  installedVersionId: string;
  installedVersionName: string;
  latestVersionId: string;
  latestVersionName: string;
  latestPublished: string | null;
  /** 作者写的更新说明，已截断到 4000 字 */
  latestChangelog: string | null;
  updateAvailable: boolean;
  /** 本地导入的包没有来源信息，这里是 false */
  canAutoCheck: boolean;
  /** 给玩家看的说明，可以全部展示 */
  notes: string[];
}

export interface PackChange {
  path: string;
  /** mod / config / other */
  kind: string;
  oldSize: number;
  newSize: number;
  /** 只有 added / changed 才有下载地址 */
  url: string;
}

export interface PackUpdatePlan {
  planId: string;
  instanceId: string;
  packName: string;
  fromVersionId: string;
  fromVersionName: string;
  toVersionId: string;
  toVersionName: string;
  added: PackChange[];
  removed: PackChange[];
  changed: PackChange[];
  unchanged: number;
  downloadBytes: number;
  diskNeeded: number;
  diskFree: number | null;
  mcVersionFrom: string;
  mcVersionTo: string;
  loaderFrom: string | null;
  loaderTo: string | null;
  /** 固定 true：更新过程不碰存档 */
  keepsWorlds: boolean;
  /** 固定 false */
  touchesSaves: boolean;
  issues: { code: string; message: string; blocking: boolean }[];
  blocking: boolean;
}
```

### 36.3 调用示例

```ts
import { invoke } from "@tauri-apps/api/core";

// 1. 进实例详情页时查一下（失败不用弹错误，整合包之外还要看别的）
let check: PackUpdateCheck | null = null;
try {
  check = await invoke<PackUpdateCheck>("check_modpack_update", {
    instanceId: instance.id,
  });
} catch {
  check = null;
}

// 2. 有更新才显示横幅
if (check?.canAutoCheck && check.updateAvailable) {
  showBanner(`${check.packName} 有新版本 ${check.latestVersionName}`, async () => {
    const plan = await invoke<PackUpdatePlan>("plan_modpack_update", {
      instanceId: instance.id,
      versionId: check!.latestVersionId,
    });

    if (plan.blocking) {
      showErrors(plan.issues.filter((i) => i.blocking).map((i) => i.message));
      return;
    }

    // 3. 让玩家看清楚会动什么
    await confirm({
      title: `从 ${plan.fromVersionName} 更新到 ${plan.toVersionName}`,
      lines: [
        `新增 ${plan.added.length} 个文件`,
        `删除 ${plan.removed.length} 个文件`,
        `改动 ${plan.changed.length} 个文件`,
        `${plan.unchanged} 个文件不用动`,
        `需要下载约 ${Math.ceil(plan.downloadBytes / 1048576)} MB`,
        "存档不会被改动",
      ],
      notes: plan.issues.filter((i) => !i.blocking).map((i) => i.message),
    });
  });
}
```

### 36.4 行为约定

- **只读**。两个命令都不写实例文件；`plan_modpack_update` 会把两个版本的 `.mrpack` 下到
  `{game}/.astral/modpacks/` 缓存里（只下 `.mrpack` 本身，不下里面的 Mod），这样第二次规划不用重下。
- **本地导入的包查不了**。`check_modpack_update` 返回 `canAutoCheck: false` + 一条 note，不报错也不假装有更新。
- **不是整合包装的实例直接报错**。返回 `INVALID_VALUE|这个实例不是从整合包装的…`，因为没有任何东西可比。
- **游戏版本变了就是阻塞**。`mcVersionFrom !== mcVersionTo` 时 `blocking: true`，因为那等于换一个实例，
  不是"更新"。加载器类型变了同理。
- **删 Mod 和覆盖配置只提示不阻塞**。新版本移除了 Mod、或要覆盖 `config/` 时给 `NOTE`，
  让玩家自己决定（这些改动是可预期的，不是错误）。
- **磁盘不够是阻塞**。`diskFree < downloadBytes` 时 `DISK_FULL` + `blocking: true`。
- **`keepsWorlds` / `touchesSaves` 是给玩家看的承诺**。更新流程只动 `mods/` 与 `config/`，
  永远不碰 `saves/`；这两个字段写死是为了让界面能直接显示"存档不会被动"。

### 36.5 前端接入建议

- 更新横幅放在实例详情页顶部，不要弹窗打断启动。
- `latestChangelog` 可以直接用 markdown 渲染（作者写的是 Markdown）。
- 计划里的 `added` / `removed` / `changed` 按 `kind` 分组展示：Mod 一组、配置一组、其他一组。
- `removed` 里的条目要标红，并提示"这些文件会从实例里删掉"。
- 阻塞时不要给"仍然继续"的按钮 —— 换游戏版本请引导玩家新建实例。

---

## 37. 实例快照：整个实例打包与还原（V2.5 新增）

存档备份（§19 那一套是单存档）、整合包导出（§18，不含存档）都覆盖不到一件事：
**"我想把现在这个实例整个存下来，回头原样还回来"**。换电脑前存一份、装一堆新
Mod 之前存一份、手滑删了东西之后找回来 —— 这就是实例快照。

### 37.1 命令

| 命令 | 参数 | 返回 |
|---|---|---|
| `list_instance_snapshots` | — | `Snapshot[]` |
| `create_instance_snapshot` | `{ instanceId: string, note?: string, taskId?: string }` | `Snapshot` |
| `preview_instance_restore` | `{ snapshotId: string }` | `SnapshotPreview` |
| `restore_instance_snapshot` | `{ snapshotId: string, taskId?: string }` | `RestoreOutcome` |
| `delete_instance_snapshot` | `{ snapshotId: string, confirm: boolean }` | `number`（删掉的字节数） |

### 37.2 类型

```ts
export interface Snapshot {
  snapshotId: string;
  instanceId: string;
  instanceName: string;
  gameVersion: string;
  loaderKind: string | null;
  loaderVersion: string | null;
  createdAt: string;
  /** zip 文件大小 */
  bytes: number;
  /** 打包进去的文件数 */
  fileCount: number;
  /** 打包前这些文件的原始大小（还原要占的空间按这个算） */
  sourceBytes: number;
  sha256: string;
  note: string;
  /** 实际收进去的目录与文件，例如 ["instance.json", "mods", "config", "saves"] */
  parts: string[];
  /** 明确没进的，例如 ["logs", "screenshots"] */
  skipped: string[];
}

export interface SnapshotPreview {
  snapshot: Snapshot;
  targetDir: string;
  targetExists: boolean;
  targetFiles: number;
  targetBytes: number;
  needBytes: number;
  availableBytes: number | null;
  /** 现有实例会被搬到这里（不是删除）；目标原来不存在时为 null */
  replacedDir: string | null;
  /** 不能还原的原因，非空就不要给"还原"按钮 */
  blockers: string[];
  /** 能还原但玩家该知道的 */
  warnings: string[];
}

export interface RestoreOutcome {
  instanceId: string;
  restoredFiles: number;
  bytes: number;
  replacedDir: string | null;
}
```

### 37.3 调用示例

```ts
import { invoke } from "@tauri-apps/api/core";

// 1. 实例详情页的"备份"按钮
const snap = await invoke<Snapshot>("create_instance_snapshot", {
  instanceId: instance.id,
  note: "装新 Mod 之前",
});
toast(`已备份 ${snap.fileCount} 个文件，${(snap.bytes / 1048576).toFixed(1)} MB`);

// 2. 快照列表（全局，不属于某个实例）
const snaps = await invoke<Snapshot[]>("list_instance_snapshots");

// 3. 还原前必须先预览
const preview = await invoke<SnapshotPreview>("preview_instance_restore", {
  snapshotId: snaps[0].snapshotId,
});
if (preview.blockers.length > 0) {
  showErrors(preview.blockers);
} else {
  await confirm({
    title: `还原到「${preview.snapshot.createdAt}」的状态`,
    lines: [
      `会写回 ${preview.snapshot.fileCount} 个文件`,
      preview.targetExists
        ? `现在的实例会先搬到 ${preview.replacedDir}（不会删除）`
        : "这个实例现在不在，还原会重新建出来",
    ],
    notes: preview.warnings,
  });
  const out = await invoke<RestoreOutcome>("restore_instance_snapshot", {
    snapshotId: preview.snapshot.snapshotId,
  });
}
```

### 37.4 行为约定

- **游戏运行中、或有进行中的任务时一律拒绝**（`INSTANCE_BUSY`）。这时候文件正在被写，
  快照拿到的会是半成品，事后也无法自证一致 —— 和存档备份（§19）是同一条原则。
- **收哪些**：`mods/`、`config/`、`saves/`、`resourcepacks/`、`shaderpacks/`，以及顶层的
  `instance.json`、`options.txt`、`servers.dat`、`optionsof.txt`。
- **不收哪些**：`logs/` 和 `screenshots/`（体积大、丢了不可惜）。`skipped` 字段会如实列出来，
  还原预览的 `warnings` 也会提醒"历史日志/截图不会回来"。
- **快照存在 `<gameDir>/snapshots/`，不在实例目录里**。放在实例里的话，还原时整个目录被换掉，
  快照自己也没了。
- **还原不删现有实例**：先把整个实例目录 rename 到 `snapshots/replaced/<时间戳>-<实例id>/`，
  再把快照放回去。玩家不满意还能手动把旧目录搬回来。
- **先解到暂存目录**：`snapshots/staging-<纳秒>/`，全部解完、确认里面有 `instance.json`，
  才动现有实例。中途失败直接清掉暂存，现有实例一个字节都没动。
- **还原前校验 sha256**，坏包直接 `VERIFY_FAILED`，不会解出一半。
- **删除快照必须 `confirm: true`**，否则 `UNSUPPORTED`。
- **快照 zip 自带一份 `snapshot.json` 清单**，就算 `index.json` 丢了，zip 本身也说得清自己是什么。
- 符号链接一律跳过（打包时）/ 拒绝（解压时）；解压逐条校验路径防 Zip Slip。

### 37.5 前端接入建议

- 快照列表放"实例 → 备份"页，和存档备份（§19）并列，但用不同的 tab：
  一个叫"存档备份"，一个叫"整个实例"。
- 创建快照时给一句人话说明："会把 Mod、配置、存档一起打包，不含日志和截图"。
- 还原按钮做成危险色，且必须先走 `preview_instance_restore`；`blockers` 非空时禁用。
- `replacedDir` 可以显示成"旧版本保存在 …"，配一个"打开文件夹"按钮（复用 `open_path`）。
- 快照体积通常几百 MB，列表里显示 `bytes` 时要带单位换算。

---

## 38. 内存与 JVM 参数校验（V2.5 新增）

以前 `update_instance` 对 `memoryMb` 和 `jvmArgs` **一个字符都不校验**：

- 传 `memoryMb: 0` → 启动器生成 `-Xmx0M`，Java 直接起不来；
- 传 `memoryMb: 999999` → 比机器内存还大，Java 报一堆英文；
- 在 `jvmArgs` 里再写一个 `-Xmx8G` → 和启动器自己生成的那个打架，**后生效的那个说了算**，
  玩家看到"我明明设了 4 GB 却吃了 8 GB"，根本查不出来；
- 参数里带换行、引号没配对 → 拼出来的命令行是坏的。

这些都要等到点启动、Java 报错才暴露。现在写配置时就挡掉。

### 38.1 命令

| 命令 | 参数 | 返回 |
|---|---|---|
| `check_instance_jvm` | `{ instanceId: string }` | `JvmCheck` |

写配置仍然用已有的 `update_instance`，只是它会校验了。

### 38.2 类型

```ts
export interface JvmIssue {
  /** "memory" 或 "args" */
  field: "memory" | "args";
  /** "error" 挡住启动，"warn" 只提示 */
  severity: "error" | "warn";
  message: string;
}

export interface JvmCheck {
  /** 实例里设置的值，null = 没设过 */
  memoryMb: number | null;
  /** 实际会传给 Java 的 -Xmx 值 */
  effectiveMb: number | null;
  /** 去掉空项与首尾空白后的参数 */
  args: string[];
  systemTotalMb: number;
  /** 这台机器上说得通的内存上限 */
  maxMb: number;
  issues: JvmIssue[];
  /** 没有 error 级问题 */
  ok: boolean;
}
```

### 38.3 规则

**内存（`memoryMb`）**

| 情况 | 结果 |
|---|---|
| `0` | `error`：内存不能是 0，Java 起不来 |
| `< 512` | `error`：至少要 512 MB，建议 2048 MB 以上 |
| `> 32768` | `error`：超过上限 32768 MB |
| `> 物理内存` | `error`：比这台机器的物理内存还大，启动会失败 |
| `> 物理内存 × 3/4`（按 512 取整） | `warn`：超过建议上限，系统会开始用虚拟内存，反而更卡 |
| 其他 | 通过 |

**自定义参数（`jvmArgs`）**

| 情况 | 结果 |
|---|---|
| 超过 32 条 | `error` |
| 单条超过 512 字符 | `error` |
| 含换行 | `error` |
| `-Xmx` / `-Xms` / `-Xss` 开头 | `error`：内存请在上面「内存」里设置 |
| 引号数量是奇数 | `error`：引号没配对 |
| `-javaagent:<路径>` 指的文件不存在 | `warn`：加上它游戏会启动失败 |

**规范化**：所有参数会 `trim()`，空串直接丢掉 —— 玩家从论坛复制参数常常带多余空格和空行。

### 38.4 错误码

`update_instance` 校验失败时返回 `INVALID_VALUE`：

```
INVALID_VALUE|内存设置有问题：内存 0 MB ...
INVALID_VALUE|JVM 参数设置有问题：内存请在上面「内存」里设置，不要写进 JVM 参数：-Xmx8G。...
```

### 38.5 启动前预检多一项

`preflight_launch` 的 `checks` 里新增 `key: "jvm"`、`label: "JVM 参数可用"`，
**`blocking: true`**（写错了真的起不来）。所以现在预检一共 8 项：

```
running / tasks / username / files / java / memory / jvm / mods
```

`memory` 那一项仍然只是提示（`blocking: false`），`jvm` 这一项会拦。

### 38.6 前端接入建议

- 实例设置页的内存输入框旁边直接显示 `maxMb` 作为上限提示，别让玩家自己猜。
- `jvmArgs` 用一个"一行一条"的多行输入框，提交前先本地做一次 `trim` 过滤空行。
- 保存时如果收到 `INVALID_VALUE`，按 `|` 切开把第二段直接显示在对应输入框下方。
- 可以在实例卡片上显示一个警告角标：调一次 `check_instance_jvm`，`!ok` 就标红。
- **不要**在内存输入框里也允许写 `-Xmx`；两个入口会互相打架。

---

## 39. 一键进服：启动就直接进服务器（V2.5 新增）

以前只能"启动游戏"，进服务器还得自己在多人游戏里点。玩家收藏了服务器，
每次却要重新找一遍。现在可以从收藏列表直接点进去，也可以临时输个地址。

### 39.1 命令

| 命令 | 参数 | 返回 |
|---|---|---|
| `start_launch_server` | `{ instanceId, serverId }` | `Task` |
| `start_launch_address` | `{ instanceId, address }` | `Task` |
| `preview_server_join` | `{ instanceId, serverId?, address? }` | `JoinTarget` |

`preview_server_join` 是**只读**的，用来在按钮上显示"将进入 mc.example.com"，
以及让玩家在启动前确认参数对不对。`serverId` 和 `address` 传一个即可，
都传时优先 `serverId`。

### 39.2 类型

```ts
export interface JoinTarget {
  /** 来自收藏时是服务器 id，临时地址是 null */
  serverId: string | null;
  /** 显示用的名字（收藏的名字，或地址本身） */
  name: string;
  host: string;
  port: number;
  /** 显示用地址，端口是 25565 时不带端口 */
  address: string;
  /** "quickPlay"（1.20+）或 "legacy"（1.19 及更早） */
  mode: "quickPlay" | "legacy";
  /** 实际追加到游戏命令行的参数 */
  args: string[];
}
```

### 39.3 两种模式

| 游戏版本 | 追加的参数 | 说明 |
|---|---|---|
| 1.20 及以上 | `--quickPlayMultiplayer host:port` | 官方 quick play 参数，进游戏直接连 |
| 1.19 及更早 | `--server host --port port` | 老参数，效果一样 |

**版本号读不出来时走老参数**：老参数从 1.6 就有，覆盖面比新参数广，
猜错时它是更安全的一边。`1.20.1-forge-47.2.0` 这种带后缀的版本号也能识别
（只看前缀数字段）。

### 39.4 地址校验

| 情况 | 结果 |
|---|---|
| 空 / 全是空格 | `INVALID_VALUE|服务器地址不能为空` |
| 超过 253 字符 | `INVALID_VALUE|服务器地址太长了` |
| 含空格 / 换行 / 控制字符 | `INVALID_VALUE|服务器地址里不能有空格或换行` |
| 含 `/` 或 `\` | `INVALID_VALUE|服务器地址里不能有斜杠` |
| 端口 0 | `INVALID_VALUE|端口必须在 1-65535 之间` |

地址解析复用 `server::split_address`，所以 `mc.example.com`、
`mc.example.com:25566`、`[::1]:25565` 都能用。

### 39.5 行为约定

- **不修改任何文件**：进服参数只是启动时加在命令行后面，不写进实例配置，
  也不会改游戏的 `servers.dat`。
- 收藏列表里没有这个 id → `NOT_FOUND|收藏列表里没有这个服务器`。
- 实例正在运行 / 有任务时和普通启动一样报 `INSTANCE_BUSY` / `BUSY`。
- 任务标题会写成「启动 1.20.1 生存 并进入 mc.example.com」，玩家在任务面板
  能看出这次是带地址启动的。
- 进服参数在**加载器分支和原版分支都生效**（Fabric/Quilt/Forge/NeoForge 都一样）。

### 39.6 前端接入建议

- 服务器卡片上加一个"进入游戏"按钮，调 `start_launch_server`；实例没选时先让
  玩家选实例（或直接用 `getActiveInstance`）。
- 多人游戏页的"直接连接"输入框可以配 `start_launch_address`，玩家输一次地址
  就能进，不用先收藏。
- 按钮 hover 时可以调 `preview_server_join` 把 `address` 显示成 tooltip。
- 老版本（`mode === "legacy"`）可以在界面上加一句说明："这个版本用老参数
  进服，个别服务端可能需要在多人游戏里手动确认一次。"
- **不要**把 `args` 直接拿去自己拼命令行 —— 启动走 `start_launch_*` 命令，
  参数由后端统一处理。

## 40. 整合包更新执行（V2.5 新增）

E26 只解决了「有没有新版本、新版本会动哪些文件」，这一节补上「真去更新」。

### 40.1 命令一览

| 命令 | 参数 | 返回 | 说明 |
| --- | --- | --- | --- |
| `apply_modpack_update` | `instanceId`, `versionId`, `planId` | `Task` | 建一个 `modpack-update` 任务并在后台执行 |
| `pack_update_history` | `instanceId` | `PackUpdateRecord[]` | 可回滚的更新记录，最新的在前 |
| `rollback_modpack_update` | `instanceId`, `batchId` | `number` | 回滚一次更新，返回动过的文件数 |

`apply_modpack_update` 与其它安装命令一致：**立刻返回 Task**，进度靠 `astral://task` 事件推。完成后额外发一个 `astral://modpack-updated`。

### 40.2 类型

```ts
interface PackUpdateRecord {
  batchId: string;
  instanceId: string;
  fromVersionId: string;
  fromVersionName: string;
  toVersionId: string;
  toVersionName: string;
  createdAt: string;        // ISO 时间
  backedUp: string[];       // 备份下来的旧文件（相对实例目录）
  added: string[];          // 本次新增（回滚时删掉）
  removed: string[];        // 本次删除（回滚时从备份恢复）
}
```

### 40.3 调用示例

```ts
// 1) 先预览（E26），拿到 planId
const plan = await invoke<PackUpdatePlan>("plan_modpack_update", {
  instanceId,
  versionId: latestVersionId,
});

// 2) 玩家确认后执行
const task = await invoke<Task>("apply_modpack_update", {
  instanceId,
  versionId: latestVersionId,
  planId: plan.planId,
});

// 3) 想看能不能反悔
const history = await invoke<PackUpdateRecord[]>("pack_update_history", { instanceId });
await invoke<number>("rollback_modpack_update", {
  instanceId,
  batchId: history[0].batchId,
});
```

### 40.4 行为约定

- **计划指纹**：`planId` 是计划内容的哈希，不是随机数。执行前会重新算一遍计划，对不上直接 `PLAN_EXPIRED|更新计划已过期，请重新检查更新`，不会拿旧计划硬套到变了样的实例上。
- **先备份再改**：备份目录是实例内的 `.astral-pack-update/<batchId>/`，只备份**真要动的文件**（新增、变更、删除的那几个），不是整目录复制。来源记录 `.astral-pack.json` 也留一份。
- **失败整批回滚**：下载失败、解压失败、磁盘满、取消，都会把新增的删掉、备份的还原回去、来源记录还原。回滚干净时连备份目录一起删掉，不留垃圾。
- **回滚记录留 5 份**（`MAX_HISTORY = 5`），更早的连目录一起清掉。
- **不碰存档**：计划里固定 `keepsWorlds: true`、`touchesSaves: false`，执行的每一步只处理计划列出的 `mods/`、`config/` 等路径。
- **运行中拒绝**：游戏在跑时 `INSTANCE_BUSY`，避免改到一半的文件被游戏读。
- **本地导入的包查不了**（E26 已定），自然也没法更新。

### 40.5 前端接入建议

- 「有新版本」横幅上的按钮直接串 `plan_modpack_update` → 弹计划确认框 → `apply_modpack_update`。
- 确认框里务必展示 `plan.added/removed/changed` 的数量与 `downloadBytes`，并提示「有 N 个配置文件会被覆盖」。
- 任务进度用现成的任务列表组件（`stage` 是 `download` / `extract`，`label` 带当前文件路径）。
- 更新完在「更新历史」里给一个「回滚」按钮，调 `rollback_modpack_update`。回滚是不可逆操作，需要二次确认。

## 41. 日志搜索与级别筛选（V2.5 新增）

日志窗口原来只能「读最后 N 行」，玩家要把整屏往下翻才能找到报错。这一节补上搜索。

### 41.1 命令

| 命令 | 参数 | 返回 |
| --- | --- | --- |
| `search_log` | `instanceId`, `logId`, `query`, `level?`, `caseSensitive?`, `limit?` | `LogSearchResult` |

`level` 传 `error` / `warn` / `info` / `debug` / `trace`（也接受 `warning`），空或不传表示不过滤。`limit` 默认 200，最大 2000。

### 41.2 类型

```ts
interface LevelCounts {
  error: number;
  warn: number;
  info: number;
  debug: number;
  trace: number;
  other: number;   // 堆栈、原始输出这类没有级别的行
}

interface LogMatch {
  lineNo: number;          // 从 1 开始
  level: string | null;    // error | warn | info | debug | trace | null
  text: string;
}

interface LogSearchResult {
  id: string;
  name: string;
  query: string;
  level: string | null;
  caseSensitive: boolean;
  totalMatches: number;    // 扫到的命中总数
  returned: number;        // 实际返回条数
  skipped: number;         // 因为 limit 没返回的旧条数
  matches: LogMatch[];
  levelCounts: LevelCounts;
  scannedLines: number;
  scannedBytes: number;
  scannedTailOnly: boolean;
  note: string | null;
}
```

### 41.3 调用示例

```ts
// 搜索关键词
const r = await invoke<LogSearchResult>("search_log", {
  instanceId,
  logId,
  query: "fabric",
});

// 只看错误，带级别统计做筛选栏徽标
const errs = await invoke<LogSearchResult>("search_log", {
  instanceId,
  logId,
  query: "",
  level: "error",
});
// errs.levelCounts.error / .warn / ... 可以直接显示在按钮上
```

### 41.4 行为约定

- **级别识别**：只认行首 200 字符内的 `[...]` 标签，取斜杠后面那一段，所以 `[main/INFO]`、`[Render thread/ERROR]`、`[WARNING]` 都能认；`FATAL`/`SEVERE` 归到 error，`FINE` 归 debug，`FINEST` 归 trace。正文里出现的 `[FATAL]` 不会被误判。
- **级别过滤不影响统计**：`levelCounts` 永远是整段扫描结果的统计，方便前端在过滤状态下仍能显示「共 3 个错误」。
- **命中太多只返最近的**：`skipped` 是被丢掉的旧条数，`matches` 里是最后 `limit` 条。排查问题基本都在看最后发生了什么。
- **超大日志只扫尾部**：超过 16 MB 只扫最后 16 MB，此时 `scannedTailOnly: true`、`note` 里会写明「行号从这一段开头算起」，不会假装是全文行号。`.gz` 归档日志照样能搜（解压上限 64 MB）。
- **只读**：不写文件、不联网、不上传。

## 42. Mod 图标（V2.5 新增）

`ModFile.iconPath` 一直是 `null`，Mod 列表只能显示文件名。这一节补上图标读取。

### 42.1 命令

| 命令 | 参数 | 返回 |
| --- | --- | --- |
| `read_mod_icon` | `instanceId`, `fileName` | `ModIcon \| null` |

没有图标返回 `null`，**不是错误**。只有文件名非法、文件不存在、jar 打不开才返回 `Err`。

### 42.2 类型

```ts
interface ModIcon {
  fileName: string;
  entry: string;       // 在 jar 里的路径
  mime: string;        // image/png | image/jpeg | image/webp | image/gif
  sizeBytes: number;
  dataUrl: string;     // data:image/png;base64,...
  source: "fabric" | "quilt" | "forge" | "root";
}
```

`ModFile` 新增一个字段：

```ts
interface ModFile {
  // ...原有字段
  hasIcon: boolean;    // jar 里有没有能用的图标
}
```

`iconPath` 字段保留但恒为 `null`（历史兼容），图标请走 `read_mod_icon`。

### 42.3 图标从哪来

按顺序尝试，第一个命中且条目真的存在就用它：

| 来源 | 声明方式 |
| --- | --- |
| Fabric | `fabric.mod.json` 的 `"icon"`（字符串，或 `{"1x":"a.png","2x":"b.png"}` 对象） |
| Quilt | `quilt.mod.json` 的 `quilt_loader.metadata.icon` |
| Forge / NeoForge | `META-INF/mods.toml` / `neoforge.mods.toml` 里的 `logoFile = "x.png"`（`a.png,b.png` 取第一个） |
| 兜底 | jar 根目录的 `icon.png` / `logo.png` / `icon.jpg` |

对象形式优先取 `2x`（大图更清楚），没有就取任意一张。

### 42.4 行为约定

- **列表不解压图片**：`list_mods` 只是打开 jar 查条目在不在，返回 `hasIcon` 布尔。列 100 个 Mod 也不会把几 MB 图片塞进 IPC。
- **要图才读**：前端按需对可见的几行调 `read_mod_icon`，可以配合 IntersectionObserver 懒加载。
- **上限 512 KiB**：超了当没有图标（有些整合包里塞了整张壁纸当 logo）。
- **不写盘**：不缓存、不生成缩略图，每次现读。jar 是只读的，不存在一致性问题。
- **符号链接拒绝**：`mods/` 里指向别处的 jar 不会被读。

### 42.5 前端接入建议

- 列表行左侧留 32×32 的位置：`hasIcon` 为真时调 `read_mod_icon` 填 `<img src={dataUrl}>`，否则显示首字母占位。
- 图标只在行进入可视区时读，读完记在组件状态里，别每次渲染都 invoke。
- 同一个实例切走再回来时清一次缓存（Mod 可能被换了）。

## 43. 项目详情页（V2.5 新增）

`get_resource_project` 只给标题、简介、图标、下载量四样，够列表用。详情页还需要图集、完整介绍、分类、客户端/服务端支持、关注数、相关链接 —— 这一节补上。

### 43.1 命令

| 命令 | 参数 | 返回 |
| --- | --- | --- |
| `get_project_detail` | `projectId`（id 或 slug） | `ProjectDetail` |

`get_resource_project` 保留不动，列表页继续用它（字段少、更省）。

### 43.2 类型

```ts
interface GalleryImage {
  url: string;              // 只收 https
  featured: boolean;
  title: string | null;
  description: string | null;
  created: string | null;
}

interface LicenseInfo {
  id: string | null;
  name: string | null;
  url: string | null;
}

interface ProjectDetail {
  id: string;
  slug: string | null;
  title: string;
  description: string;        // 一句话简介
  body: string;               // 完整介绍（Markdown），已截断
  bodyChars: number;          // 截断前有多少字符
  bodyTruncated: boolean;
  iconUrl: string | null;
  projectType: string;        // mod | modpack | resourcepack | shaderpack | datapack | plugin
  categories: string[];
  additionalCategories: string[];
  clientSide: string | null;  // required | optional | unsupported | unknown
  serverSide: string | null;
  downloads: number;
  followers: number;
  published: string | null;
  updated: string | null;
  status: string | null;
  license: LicenseInfo;
  sourceUrl: string | null;
  issuesUrl: string | null;
  wikiUrl: string | null;
  discordUrl: string | null;
  gallery: GalleryImage[];
  gameVersions: string[];
  loaders: string[];
  versionsCount: number;
  source: "network" | "cache";
  stale: boolean;
  ageMs: number;
  networkError: string | null;
}
```

### 43.3 调用示例

```ts
const detail = await invoke<ProjectDetail>("get_project_detail", {
  projectId: hit.projectId,   // 搜索结果里的 projectId 或 slug 都行
});

// 图集：第一张是作者指定的封面
const cover = detail.gallery[0]?.url ?? detail.iconUrl;

// 正文：bodyTruncated 为真时在末尾加一条「完整介绍请到项目页查看」
```

### 43.4 行为约定

- **只读**：不写盘、不改任何实例。
- **走离线缓存**：断网时返回上次的详情，`source === "cache"`、`stale === true`、`networkError` 里是原始错误。界面应该照常显示内容，另加一条「当前是缓存数据」的提示。
- **正文按字符截到 2 万**：`bodyChars` 是原文长度，`bodyTruncated` 告诉你有没有被截。截断是为了不把几万字塞进 IPC。
- **图集只收 https**：相对路径、http 图片一律跳过（不猜 CDN 域名）。封面（`featured`）排最前，最多 20 张。
- **空 id 报错**：`INVALID_VALUE|项目 id 不能为空`；超长（>128）也拒。
- **格式异常报 `INTERNAL|项目详情格式异常`**：比如服务端返回数组而不是对象。

### 43.5 前端接入建议

- 详情页进页面时并发拉 `get_project_detail` 与 `list_resource_files`（后者要版本列表装东西），两个请求互不依赖。
- 正文是 Markdown，渲染前记得过滤 HTML（Modrinth 的 body 是 Markdown 不是 HTML，但别直接把字符串塞 `dangerouslySetInnerHTML`）。
- 图集做成横向缩略图条 + 点击放大；封面图直接用在页头背景。
- `clientSide` / `serverSide` 是 `unsupported` 时，在「安装到客户端」按钮旁标一行提醒（服务端专属的 Mod 装到客户端没用）。

## 44. 日志清理（V2.5 新增）

磁盘分析里 `logs` 一直标着"可以安全清理"，但之前**没有任何命令能清**。玩家只能自己去文件夹里挑着删，一不小心把 `latest.log` 删了，下次出问题就没法查。这一节补上「先预览、再清理」。

### 44.1 命令

| 命令 | 参数 | 返回 |
| --- | --- | --- |
| `log_usage` | `instanceId` | `LogUsage` |
| `plan_log_cleanup` | `instanceId`, `keepDays?`, `keepCount?` | `LogCleanupPlan` |
| `prune_logs` | `instanceId`, `keepDays?`, `keepCount?`, `confirm` | `LogCleanupResult` |

默认 `keepDays = 30`、`keepCount = 5`。

### 44.2 类型

```ts
interface LogUsage {
  instanceId: string;
  logsDir: string;
  files: number;           // logs/ 里的日志份数
  bytes: number;
  archives: number;        // 其中 .gz 归档
  archiveBytes: number;
  oldest: string | null;
  newest: string | null;
  crashFiles: number;      // crash-reports/ 单独算
  crashBytes: number;
  truncated: boolean;      // 文件太多只处理了一部分
}

interface LogFileItem {
  path: string;            // 相对实例目录，例如 logs/2025-01-01-1.log.gz
  bytes: number;
  modifiedAt: string;
  reason: string | null;   // 只有 keep 里有：为什么留着
}

interface LogCleanupPlan {
  instanceId: string;
  keepDays: number;
  keepCount: number;
  remove: LogFileItem[];
  removeBytes: number;
  keep: LogFileItem[];
  keepBytes: number;
  issues: string[];
  freedBytes: number;      // 删完能省多少
}

interface LogCleanupResult {
  instanceId: string;
  removed: number;
  freedBytes: number;
  kept: number;
  failed: string[];        // 删失败的（多半是被占用）
}
```

### 44.3 调用示例

```ts
// 1. 设置页显示占用
const usage = await invoke<LogUsage>("log_usage", { instanceId });

// 2. 点「清理日志」先出预览
const plan = await invoke<LogCleanupPlan>("plan_log_cleanup", {
  instanceId, keepDays: 30, keepCount: 5,
});
// plan.keep 里每一条都有 reason，直接列给玩家看

// 3. 玩家确认后才真删
const result = await invoke<LogCleanupResult>("prune_logs", {
  instanceId, keepDays: 30, keepCount: 5, confirm: true,
});
```

### 44.4 行为约定

- **`latest.log` 永远不删**，不管参数怎么传。它是当前这次启动的日志，删了下次崩就没得查。
- **只认日志文件**：`logs/` 与 `crash-reports/` 下的 `.log` / `.txt` / `.log.gz` / `.txt.gz`。别的一概不碰（`notes.md`、`settings.json` 都不会被删）。
- **三条保留规则**（按顺序判断，命中就留）：
  1. 文件名是 `latest.log` → 留；
  2. 是最新的 `keepCount` 份 → 留；
  3. `keepDays` 天内还写过 → 留（`keepDays = 0` 表示不按天数保）。
- **删前必须 `confirm: true`**，否则报 `UNSUPPORTED|清理日志需要明确确认`。
- **删失败不算致命**：文件被占用（游戏还开着）时记进 `failed`，其他照删。
- **只读的计划**：`plan_log_cleanup` 不删任何东西，可以随便调。
- **不跟符号链接**：`logs/` 里指向别处的文件不会被删。

### 44.5 前端接入建议

- 设置页「存储」那一栏：显示 `log_usage` 的 `bytes` 与 `archives`，旁边一个「清理」按钮。
- 点清理 → 弹窗列出 `plan.remove` 的条数与 `freedBytes`，下面小字写「会保留最近 5 份和 30 天内的日志，`latest.log` 永远保留」。
- 玩家点确定 → `prune_logs(..., confirm: true)` → 用返回值刷新占用。
- 有 `failed` 时提示「有 N 个日志正在被占用，关掉游戏后再试」。
