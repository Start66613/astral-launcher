# 前后端接口契约 v2

> 本文档是 **Rust 核心** 与 **React 界面** 之间唯一的对接约定。
> 任何一方要改接口，**先改这份文档**，再改代码。
>
> 版本：**v2** · 2026-10-23 · 维护者：核心（`feature/core-features`）
> v1 的正文已并入本文；v1 遗留接口在第 8 节标注 `legacy`。

---

## 0. 协作方式

1. 核心写契约 → 提交到 `feature/core-features` → 界面**只读**审核并提建议 → 核心修订 → 双方确认 → 开始实现。
2. 不让两个 AI 同时编辑本文件。
3. 界面要新接口：在 issue/PR 里写「需要 `xxx`，参数 …，返回 …」，核心补进本文档后再实现。

### 文件归属

| 目录 / 文件 | 归属 | 说明 |
|---|---|---|
| `app/src-tauri/src/**` | 核心 | Rust 命令、安装/启动、下载、诊断 |
| `app/src-tauri/vendor/portablemc/**` | 核心 | 打过镜像补丁的上游 crate |
| `app/src-tauri/Cargo.toml`、`tauri.conf.json`、`capabilities/**` | 核心 | 新权限由核心加 |
| `app/src/pages/**`、`components/**`、`styles/**` | 界面 | 页面、组件、主题 |
| `app/src/lib/bridge.ts`、`types.ts` | **共享** | 只做「新增」；改已有签名需双方同一次合并 |
| `docs/**` | 各自维护 | 核心写实现记录，界面写设计记录 |

---

## 1. 运行时模型

- 桌面端：Tauri 2；`invoke(cmd, args)` 调命令，`listen(event, cb)` 收推送。
- 浏览器端（`pnpm dev` 直接开网页）：`inTauri === false`，返回演示数据，界面必须能渲染并标注「浏览器预览」。
- 所有命令异步；耗时任务在 Rust 侧 `spawn_blocking`，不阻塞 UI 线程。
- **长任务一律「先返回 Task，再推事件」**，不用「阻塞到完成再返回」。

### 1.1 错误格式（v2 变更）

Rust 侧仍返回 `Result<T, String>`，字符串是**结构化编码**：

```
CODE|中文消息|可选技术细节
例：VERSION_NOT_FOUND|找不到版本 1.20.4|manifest 中无此 id
```

- 只按**前两个** `|` 切分，`detail` 内允许出现 `|`。
- `CODE` 取值见 3.1；未知 CODE 用 `INTERNAL`。
- `bridge.ts` 统一解析成 `ApiError`（`class ApiError extends Error`，带 `code` / `detail`），页面 `catch (e)` 拿到的仍是 Error 子类，`String(e)` 能直接显示消息，`e.code` 可用于分支。

这样旧页面不做任何改动也不会坏，新页面可以按 code 分支。

### 1.2 任务模型（v2 新增）

所有超过 1 秒的操作都产生一个 `Task`：

- `start_*` 命令立即返回 `Task`；
- 状态变化通过 `astral://task` 推送**完整快照**（不是增量）；
- 前端可以用 `get_tasks()` / `get_task(id)` **补拉**，解决组件重挂载、React 18 StrictMode 双挂载、刷新后恢复的问题；
- `cancel_task(id)` 请求取消。

---

## 2. 配置

路径：`%APPDATA%\Astral\config.json`

```jsonc
{
  "gameDir": "C:\\Users\\你\\AppData\\Roaming\\.minecraft",
  "source": "official",        // "official" | "bmclapi"
  "username": "Player",        // 离线登录名
  "activeInstanceId": null     // 当前选中实例，见 4.10
}
```

- `gameDir` 可指向任意目录（测试/多开）。
- `source` 影响版本清单来源，以及安装时依赖库/资源是否改写为 BMCLAPI。
- 设置页改完即时落盘。**v2 起 `set_config` 支持部分更新**，见第 4 节 #3。

---

## 3. 数据类型

### 3.1 `ApiErrorCode`

```
NETWORK | DISK_FULL | PERMISSION | VERSION_NOT_FOUND | INSTANCE_NOT_FOUND
JAVA_MISSING | VERIFY_FAILED | AUTH_REQUIRED | AUTH_EXPIRED | CANCELLED
BUSY | UNSUPPORTED | INTERNAL
```

### 3.2 `Task`

```ts
type TaskKind = "install" | "launch" | "loader" | "modpack" | "download" | "diagnose";
type TaskState = "queued" | "running" | "done" | "error" | "cancelled";

interface Task {
  taskId: string;              // 唯一，稳定
  kind: TaskKind;
  instanceId: string | null;   // 与实例相关时给出
  gameVersion: string | null;  // 与版本相关时给出
  title: string;               // 可直接显示的标题，如「安装 1.20.1」
  stage: string;               // "resolve" | "download" | "assets" | "jvm" | "extract" | "run" | "analyze"
  label: string;               // 可直接显示，如「下载 132/652」
  done: number;
  total: number;
  bytes: number;
  totalBytes: number;
  state: TaskState;
  error: { code: string; message: string; detail?: string } | null;
  result: unknown | null;      // kind 专属，见各命令说明
  startedAt: string;           // ISO8601
  endedAt: string | null;
}
```

> `done/total` 在 stage 切换时会归零，**不要用它算总进度**，用 `label` 展示、`stage` 做阶段图标。

### 3.3 `VersionSummary`

```ts
interface VersionSummary {
  id: string;        // "1.21.4"
  kind: string;      // "release" | "snapshot" | "old_beta" | "old_alpha"
  released: string;  // ISO8601
}
```

### 3.4 `Instance`（v2 修订）

```ts
type LoaderKind = "vanilla" | "fabric" | "forge" | "neoforge" | "quilt";

interface Instance {
  id: string;            // 稳定 id，如 "inst_7f3a91c2"；与目录名解耦
  name: string;          // 显示名，可中文、可重名
  gameVersion: string;   // "1.20.1"
  loader: { kind: LoaderKind; version: string } | null;   // vanilla 时为 null
  dir: string;           // 绝对路径，来自 id 映射
  modsCount: number;
  modsDisabled: number;  // 被禁用的模组数
  cover: string | null;  // 封面图绝对路径，无则由界面用渐变占位
  healthy: boolean;      // 版本 json + jar 齐全
  lastPlayed: string | null;      // ISO8601
  totalPlaytimeMs: number;        // 累计时长
  memoryMb: number | null;        // null = 自动
  createdAt: string;
}
```

> `id` 不再等于目录名。改名/换目录不影响 id，前端可以安全地把它当 React key。

### 3.5 `ModFile`

```ts
interface ModFile {
  fileName: string;      // "sodium-fabric-0.5.8.jar"
  name: string;          // 从 mod 元数据读出的显示名，读不到时回退文件名
  version: string | null;
  loader: string | null;
  enabled: boolean;      // 禁用 = 文件名为 *.jar.disabled
  sizeBytes: number;
  iconPath: string | null;
  projectId: string | null;  // Modrinth project id，用于回链/查更新
}
```

### 3.6 `SaveEntry`

```ts
interface SaveEntry {
  name: string;          // 存档目录名 "Expedition_07"
  dir: string;           // 绝对路径
  gameVersion: string | null;   // 从 level.dat 读
  lastModified: string;  // ISO8601
  sizeBytes: number;
  iconPath: string | null;      // 存档图标
}
```

### 3.7 `Stats`

```ts
interface Stats {
  range: "7d" | "30d" | "all";
  playtimeMs: number;
  sessions: number;
  deaths: number | null;       // 无统计数据时为 null，界面显示 "—"
  achievements: number | null;
  crashes: number;
  byInstance: { instanceId: string; playtimeMs: number }[];
}
```

### 3.8 `Account` / `MsaStatus`

```ts
type AccountStatus = "offline" | "pending" | "online" | "expired";

interface Account {
  status: AccountStatus;
  kind: "offline" | "microsoft";
  name: string;
  uuid: string;
  skinUrl: string | null;
  expiresAt: string | null;
}

type MsaStatus =
  | { status: "pending"; intervalSec: number }
  | { status: "success"; account: Account }
  | { status: "expired" }
  | { status: "denied"; message: string };
```

> 登录轮询**不再用错误表示“等待中”**，见 4.9。

### 3.9 `LoaderCatalog`

```ts
interface LoaderCatalog {
  kind: LoaderKind;
  label: string;        // "Fabric" / "Forge" / "NeoForge"
  recommended: string;  // 推荐版本
  versions: string[];   // 倒序，最新在前
}
```

---

## 4. 命令

### 4.1 基础（v1 保留，签名不变）

| # | 命令 | 参数 | 返回 | bridge | 说明 |
|---|---|---|---|---|---|
| 1 | `get_system_info` | — | `SystemInfo` | `getSystemInfo()` | 设置页展示 |
| 2 | `get_config` | — | `Config` | `getConfig()` | |
| 3 | `set_config` | `patch: Partial<Config>` | `Config` | `setConfig(patch)` | **v2 改为部分更新**，返回合并后的完整配置 |
| 4 | `scan_versions` | — | `string[]` | `scanVersions()` | 已装版本 id 列表（legacy，实例化后优先用 `list_instances`） |
| 5 | `list_versions` | — | `VersionSummary[]` | `listVersions()` | **一次返回全量**（约 920 条，30 KB），前端本地筛选 |
| 6 | `open_path` | `path: string` | `()` | `openPath(p)` | 路径不存在返回 `PERMISSION` |

`SystemInfo`：`{ os, cpuCores, totalMemGB, freeMemGB, demo }`

### 4.2 安装与启动（v2：任务化）

| 命令 | 参数 | 返回 `Task` | 说明 |
|---|---|---|---|
| `start_install` | `gameVersion: string` | `Task` | 安装/补全原版；`result` 无 |
| `start_launch` | `instanceId: string` | `Task` | 装（缺则补）+ 起进程；`state=done` 表示**已启动**，进程退出看 `game-exit` |
| `cancel_task` | `taskId: string` | `()` | 尽力取消；已开始的下载可能仍会完成当前文件 |

```ts
// start_launch 的 task.result
{ pid: number; startedAt: string }
```

### 4.3 任务查询

| 命令 | 参数 | 返回 |
|---|---|---|
| `get_tasks` | `filter?: { state?: TaskState; kind?: TaskKind }` | `Task[]` |
| `get_task` | `taskId: string` | `Task \| null` |

> 页面挂载时先 `get_tasks()` 拉一次，再 `listen` 事件——避免错过挂载前的事件。

### 4.4 实例

| 命令 | 参数 | 返回 |
|---|---|---|
| `list_instances` | — | `Instance[]` |
| `get_instance` | `id: string` | `Instance \| null` |
| `create_instance` | `{ name, gameVersion, loaderKind?, loaderVersion?, memoryMb? }` | `Task`（`result: Instance`） |
| `update_instance` | `id, patch: { name?, memoryMb?, jvmArgs?, javaPath? }` | `Instance` |
| `delete_instance` | `id, opts?: { trash?: boolean }` | `()` |
| `duplicate_instance` | `id, name` | `Task`（`result: Instance`） |
| `set_active_instance` | `id: string \| null` | `Config` |
| `get_active_instance` | — | `Instance \| null` |

- `delete_instance` 默认 `trash: true`（移入 `.astral/trash/`），`false` 才是真删。
- 创建/复制耗时（要下文件），所以返回 Task；`update/delete` 是秒级操作，直接返回。

### 4.5 加载器

| 命令 | 参数 | 返回 |
|---|---|---|
| `list_loaders` | `gameVersion: string` | `LoaderCatalog[]` |
| `start_install_loader` | `instanceId, kind, version` | `Task` |

### 4.6 整合包

| 命令 | 参数 | 返回 |
|---|---|---|
| `start_install_modpack` | `{ source: string; isUrl?: boolean; name?: string }` | `Task`（`result: Instance`） |
| `export_modpack` | `instanceId, format: "mrpack"` | `string`（导出路径） |

### 4.7 模组

| 命令 | 参数 | 返回 |
|---|---|---|
| `list_mods` | `instanceId: string` | `ModFile[]` |
| `set_mod_enabled` | `instanceId, fileName, enabled` | `ModFile` |
| `delete_mod` | `instanceId, fileName` | `()` |

禁用走重命名 `x.jar` ↔ `x.jar.disabled`，与主流启动器一致。

### 4.8 下载到目录

| 命令 | 参数 | 返回 |
|---|---|---|
| `start_download` | `{ url, kind, fileName, instanceId? }` | `Task` |

`kind` → 目录（`instanceId` 为空时落到全局目录）：

| kind | 目录 |
|---|---|
| `mod` | `mods/` |
| `modpack` | `modpacks/` |
| `datapack` | `datapacks/` |
| `resourcepack` | `resourcepacks/` |
| `shaderpack` | `shaderpacks/` |
| 其他 | `mods/` |

`fileName` 由 Rust 用 `Path::file_name()` 截断，防路径穿越。

### 4.9 登录

| 命令 | 参数 | 返回 |
|---|---|---|
| `msa_begin` | — | `{ deviceCode, userCode, verificationUri, expiresIn, intervalSec }` |
| `msa_poll` | `deviceCode` | `MsaStatus`（**正常返回 pending，不是错误**） |
| `get_account` | — | `Account \| null` |
| `logout` | — | `()` |

### 4.10 存档 / 统计 / 日志 / 诊断

| 命令 | 参数 | 返回 |
|---|---|---|
| `list_saves` | `instanceId?` | `SaveEntry[]` |
| `get_stats` | `range: "7d" \| "30d" \| "all"` | `Stats` |
| `read_log` | `{ instanceId?, lines?: number, follow?: boolean }` | `string[]` |
| `start_diagnose` | `instanceId?` | `Task`（`result` 见下） |

```ts
// start_diagnose 的 task.result
interface CrashReport {
  severity: "info" | "warn" | "fatal";
  summary: string;
  evidence: { source: string; snippet: string }[];
  fixes: { id: string; title: string; risk: "low" | "medium" | "high" }[];
}
```

### 4.11 约定路径

界面需要「打开某目录」时，统一走 `open_path`，路径由 Rust 给出，不要在界面拼：

| 用途 | 来源 |
|---|---|
| 游戏根目录 | `get_config().gameDir` |
| 实例目录 | `instance.dir` |
| 截图 | `%gameDir%/screenshots`（后续可加 `get_paths()` 统一返回） |
| 崩溃报告 | `%gameDir%/crash-reports` |
| 启动器日志 | `%APPDATA%/Astral/logs` |

---

## 5. 事件

### 5.1 `astral://task`（v2 主通道）

```ts
payload: Task   // 完整快照，每次都带全部字段
```

- 同一 task 会推送多次（阶段变化 + 进度节流，默认 ≥100ms 一次）。
- 结束时推一帧 `state = "done" | "error" | "cancelled"`。
- 前端按 `taskId` 覆盖本地 Map，不要按到达顺序累积。

### 5.2 `astral://game-exit`（v2 新增）

```ts
payload: {
  taskId: string | null;      // 对应的 start_launch
  instanceId: string;
  exitCode: number | null;    // null = 被强制结束，拿不到
  crashed: boolean;           // 非 0 退出或日志出现崩溃特征
  durationMs: number;
  endedAt: string;            // ISO8601
}
```

用途：回填 `lastPlayed` / `totalPlaytimeMs`、弹出「要不要 AI 诊断」、刷新 `get_stats`。

### 5.3 legacy 事件（保留，v3 移除）

| 事件 | payload | 替代 |
|---|---|---|
| `astral://progress` | `[version, Progress]` | `astral://task` |
| `astral://file` | `[filename, count, total, size, totalSize]` | `astral://task` |

新代码禁止使用 legacy 事件。

---

## 6. 页面 → 命令映射

给界面排版时的对照表，同时也是实现顺序的依据。

### 启动页

| 区块 | 命令 / 事件 |
|---|---|
| 账号 | `get_account` |
| 当前实例卡 | `get_active_instance`、`get_instance` |
| 启动按钮 | `start_launch` → `astral://task` → `astral://game-exit` |
| 运行状态恢复 | `get_tasks({ kind: "launch" })` |
| 版本列表 | `list_instances` |
| 存档 | `list_saves` |
| 本周统计 | `get_stats("7d")` |
| AI 卡 | `get_tasks({ kind: "diagnose" })` 或最近一次 `start_diagnose` |
| 快捷操作 | `open_path` |
| 公告 | **静态资源**，不走命令（界面自带 JSON） |

### 实例页

`list_instances` / `get_instance` / `create_instance` / `update_instance` / `delete_instance` / `duplicate_instance` / `set_active_instance` / `list_loaders`

### 下载中心

`list_versions` / `start_install` / `get_tasks` / `cancel_task`

### 模组中心

搜索：界面直连 Modrinth（`https://api.modrinth.com/v2`，对方提供 CORS）
安装：`start_download`（带 `instanceId`）
已装：`list_mods` / `set_mod_enabled` / `delete_mod`
整合包：`start_install_modpack`

### 设置

`get_config` / `set_config(patch)` / `get_system_info` / `read_log` / `open_path`

---

## 7. 兼容与迁移

| v1 用法 | v2 | 处理 |
|---|---|---|
| `install_version(v)` | `start_install(v)` | v1 命令保留，内部转 Task，返回 `()`；**新代码用 v2** |
| `launch_version(v)` → PID | `start_launch(instanceId)` → Task | v1 保留到 v3 |
| `download_to(url, kind, name)` | `start_download({...})` | v1 保留，`instanceId` 缺省为全局目录 |
| `astral://progress` | `astral://task` | 保留到 v3 |
| `set_config(full)` | `set_config(patch)` | 传完整对象仍然合法（等价于全量 patch） |

**破坏性变更只允许在双方合并的同一次里发生**，禁止单边先合。

---

## 8. 变更记录

| 版本 | 日期 | 内容 |
|---|---|---|
| v1 | 2026-10-23 | 初版：9 个命令、2 个事件、提案接口 |
| v2 | 2026-10-23 | 引入 Task 模型与 `astral://task` / `astral://game-exit`；实例 id 与目录解耦；补 `list_mods` / `list_saves` / `get_stats` / `update_instance` 等；错误码化；`msa_poll` 改状态机 |
