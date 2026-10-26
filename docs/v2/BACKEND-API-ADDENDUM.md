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