# 前后端接口契约 v3.1

> **Rust 核心** 与 **React 界面** 之间唯一的对接约定。改接口先改本文档。
>
> 版本：**v3** · 2026-10-23 · 维护者：核心（`feature/core-features`）
> 前置：v2 已被界面侧评审，本版按 P0 意见修订。

---

## 0. 本版处理了哪些评审意见

| 评审 | 问题 | 本版处理 | 章节 |
|---|---|---|---|
| **P0-1** | 实例隔离未定义 | 新增第 7 节目录布局：共享 assets/libraries/JVM，实例独立 mods/config/saves/logs；`start_download` 必须带 `instanceId`，无效 id 报 `INSTANCE_NOT_FOUND`，**不回落全局** | 5.7 / 7 |
| **P0-2** | 删除与覆盖规则缺失 | `BUSY` 判定、删当前实例后 `activeInstanceId=null`、回收站先 rename 失败即回滚；下载冲突策略 `onConflict`；模组重命名冲突后缀；文件名校验规则与 `INVALID_NAME` | 5.3 / 5.6 / 5.7 |
| **P0-3** | 「先查询再监听」漏事件 | 订阅顺序改为 **先 listen 再 get_tasks 合并**；`Task.revision` 单调递增，按 revision 取大；终态粘滞 | 2.2 / 5.2 |
| **P0-4** | 运行状态无法恢复 | 新增 `GameSession` + `get_active_session()`；`game-exit` 带 `sessionId`；启动器重启后按 pid 对账 | 2.3 / 5.10 |
| **P0-5** | v1 兼容语义变了 | `set_config` 的 **RPC 参数名保持 `config`**，只改语义为部分合并；legacy `install_version`/`launch_version`/`download_to` **保持阻塞到完成** | 9 |

P1 吸收：完整 Config、请求 DTO、调用示例、`TaskKind` 可辨识联合、`get_api_info`、取消/重试/去重、`jvmArgs`/`javaPath`、`health` 带原因、诊断预览与脱敏。
P2 记录在 §10，本版不展开。

---

## 1. 协作方式

1. 核心写契约 → 提交 `feature/core-features` → 界面只读评审 → 核心修订 → 双方确认 → 实现。
2. 不让两个 AI 同时编辑本文件。
3. 界面要新接口：写明「命令名 / 参数 / 返回 / 用在哪一屏」。

### 文件归属

| 目录 / 文件 | 归属 |
|---|---|
| `app/src-tauri/src/**`、`vendor/portablemc/**`、`Cargo.toml`、`tauri.conf.json` | 核心 |
| `app/src/pages/**`、`components/**`、`styles/**` | 界面 |
| `app/src/lib/bridge.ts`、`types.ts` | **共享**（只新增；改签名要双方同一次合并） |
| `docs/**` | 各自维护 |

---

## 2. 运行时模型

- 桌面端 Tauri 2；`invoke(cmd, args)` / `listen(event, cb)`。
- 浏览器端 `inTauri === false` 返回演示数据，界面须能渲染并标注「浏览器预览」。
- 命令异步；重活儿在 Rust `spawn_blocking`。
- 长任务：`start_*` 立即返回 `Task`，状态走事件。**legacy 命令除外**（见 §9）。

### 2.1 错误

Rust 返回 `Result<T, String>`，字符串是结构化编码：

```
CODE|中文消息|可选细节
VERSION_NOT_FOUND|找不到版本 1.20.4|manifest 中无此 id
```

- 只按**前两个** `|` 切分，`detail` 里允许出现 `|`。
- `bridge.ts` 解析成 `ApiError extends Error`（带 `code`、`detail`），旧页面 `String(e)` 仍可直接显示。
- CODE 见 4.1；未知用 `INTERNAL`。

### 2.2 任务模型

- `start_*` 立即返回 `Task`（此时 `revision = 1`）。
- 每次状态变化推一帧**完整快照** `astral://task`，`revision` 自增。
- **订阅顺序（必须照做）**：

```ts
// 1. 先建立监听
const un = await listen<Task>("astral://task", (e) => merge(e.payload));
// 2. 再拉快照，与本地合并
const snapshot = await getTasks();
snapshot.forEach(merge);

function merge(t: Task) {
  const cur = map.get(t.taskId);
  if (cur && t.revision <= cur.revision) return;   // 旧帧丢弃
  if (cur && isTerminal(cur.state) && !isTerminal(t.state)) return; // 终态粘滞
  map.set(t.taskId, t);
}
```

- 反向顺序（先查再听）会漏掉中间的帧——**不允许**。
- 命令返回的 `Task` 也走同一个 `merge`，不要直接覆盖本地的更新版本。

### 2.3 游戏会话

`Task(kind=launch).state === "done"` 只表示**进程已创建**，不表示仍在运行。真实运行状态以 `GameSession` 为准：

- `start_launch` 的 `task.result` 带 `sessionId`；
- `get_active_session()` 随时可查（启动器重启后按 pid 对账：进程在 → `running`，不在 → 已结束）；
- 进程退出时推 `astral://game-exit`（带 `sessionId`），同时 session 落盘；
- 同一实例同时只允许一个 session，重复启动返回 `BUSY`。

---

## 3. 配置

`%APPDATA%\Astral\config.json`

```ts
interface Config {
  gameDir: string;               // 根目录，含 assets/libraries/runtime/instances
  source: "official" | "bmclapi";
  username: string;              // 离线名
  activeInstanceId: string | null;
  maxConcurrentDownloads: number; // 默认 8；镜像模式内部再降
  javaPath: string | null;        // 全局默认 Java，null = 自动探测
  theme?: never;                  // 主题归界面（localStorage），不进配置
}
```

**部分更新**：`set_config` 参数名保持 `config`，传入对象**只覆盖出现的字段**（等价 patch）。省略 = 不改；显式 `null` = 清空。

```ts
await invoke("set_config", { config: { source: "bmclapi" } }); // 只改下载源
```

---

## 4. 数据类型

### 4.1 `ApiErrorCode`

```
NETWORK | DISK_FULL | PERMISSION | INVALID_NAME | VERSION_NOT_FOUND
INSTANCE_NOT_FOUND | INSTANCE_BUSY | JAVA_MISSING | VERIFY_FAILED
AUTH_REQUIRED | AUTH_EXPIRED | CANCELLED | BUSY | UNSUPPORTED | INTERNAL
```

### 4.2 `Task`

```ts
type TaskKind =
  | "install" | "launch" | "loader" | "modpack"
  | "download" | "diagnose" | "create" | "duplicate";

type TaskState = "queued" | "running" | "done" | "error" | "cancelled";

interface Task {
  taskId: string;
  revision: number;            // 从 1 开始，单调递增
  kind: TaskKind;
  instanceId: string | null;
  gameVersion: string | null;
  title: string;               // 「安装 1.20.1」
  stage: string;               // resolve|download|assets|jvm|extract|run|analyze
  label: string;               // 「下载 132/652」
  done: number;
  total: number;
  bytes: number;
  totalBytes: number;
  state: TaskState;
  error: ApiError | null;
  result: TaskResult | null;   // 终态才有
  startedAt: string;
  endedAt: string | null;
  retryOf: string | null;      // 重试来源 taskId
}
```

`result` 是**按 kind 判别**的联合，`result.kind === task.kind`：

```ts
type TaskResult =
  | { kind: "install";   gameVersion: string }
  | { kind: "create";    instance: Instance }
  | { kind: "duplicate"; instance: Instance }
  | { kind: "launch";    sessionId: string; pid: number; startedAt: string }
  | { kind: "loader";    loader: { kind: LoaderKind; version: string } }
  | { kind: "modpack";   instance: Instance }
  | { kind: "download";  path: string; bytes: number; replaced: boolean }
  | { kind: "diagnose";  report: CrashReport };
```

> `done/total` 在 stage 切换时会归零，别用来算总进度；用 `label` + `stage`。

### 4.3 `GameSession`

```ts
interface GameSession {
  sessionId: string;
  instanceId: string;
  taskId: string;
  pid: number;
  startedAt: string;
  state: "starting" | "running" | "exited";
  exitCode: number | null;   // null = 拿不到（强杀/崩溃）
  endedAt: string | null;
  durationMs: number | null;
}
```

### 4.4 `VersionSummary`

```ts
interface VersionSummary { id: string; kind: "release" | "snapshot" | "old_beta" | "old_alpha"; released: string; }
```

### 4.5 `Instance`

```ts
type LoaderKind = "vanilla" | "fabric" | "forge" | "neoforge" | "quilt";

interface InstanceHealth {
  ok: boolean;
  reasons: string[];   // ok=false 时至少一条，可直接展示：「缺少 1.20.1.jar」
}

interface Instance {
  id: string;                  // 稳定 "inst_7f3a91c2"，与目录名无关
  name: string;
  gameVersion: string;
  loader: { kind: LoaderKind; version: string } | null;   // vanilla = null
  dir: string;                 // 实例根目录（见第 7 节）
  dirs: {                      // 常用子目录，UI 直接拿，不要自己拼
    mods: string; config: string; saves: string; logs: string;
    resourcepacks: string; shaderpacks: string; screenshots: string;
  };
  modsCount: number;
  modsDisabled: number;
  cover: string | null;
  health: InstanceHealth;      // 不再用单个 boolean
  lastPlayed: string | null;
  totalPlaytimeMs: number;
  memoryMb: number | null;     // null = 自动
  jvmArgs: string[];           // [] = 用默认
  javaPath: string | null;     // null = 跟随全局/自动探测
  createdAt: string;
}
```

**`update_instance` 的 null 语义**：省略 = 不修改；显式 `null` = 清回默认（`memoryMb`/`javaPath`/`cover`）；`jvmArgs` 传 `[]` 表示清空。

### 4.6 `ModFile`

```ts
interface ModFile {
  fileName: string;            // 实际文件名（禁用态带 .disabled）
  baseName: string;            // 无扩展名主体，重命名用
  name: string;
  version: string | null;
  loader: string | null;
  enabled: boolean;
  sizeBytes: number;
  iconPath: string | null;
  projectId: string | null;
}
```

### 4.7 `SaveEntry`

```ts
interface SaveEntry {
  name: string; dir: string;
  gameVersion: string | null;
  lastModified: string;
  sizeBytes: number;
  iconPath: string | null;
}
```

### 4.8 `Stats`

```ts
interface Stats {
  range: "7d" | "30d" | "all";
  playtimeMs: number; sessions: number;
  deaths: number | null; achievements: number | null;
  crashes: number;
  byInstance: { instanceId: string; name: string; playtimeMs: number }[];
  updatedAt: string;
}
```

### 4.9 `Account` / `MsaStatus`

```ts
type AccountStatus = "offline" | "pending" | "online" | "expired";

interface Account {
  status: AccountStatus;
  kind: "offline" | "microsoft";
  name: string; uuid: string;
  skinUrl: string | null;
  expiresAt: string | null;
}

type MsaStatus =
  | { status: "pending"; intervalSec: number }
  | { status: "success"; account: Account }
  | { status: "expired" }
  | { status: "denied"; message: string };
```

### 4.10 `LoaderCatalog`

```ts
interface LoaderCatalog { kind: LoaderKind; label: string; recommended: string | null; versions: string[]; }
```

### 4.11 `ApiInfo`

```ts
interface ApiInfo {
  apiVersion: number;      // 本版 = 3
  appVersion: string;      // "0.1.0"
  features: string[];      // ["instances","mods","modpacks","mirror","msa","diagnose","sessions"]
}
```

界面启动时取一次，缺失的 feature 隐藏对应入口（兼容旧核心）。

---

## 5. 命令

### 5.1 基础

| # | 命令 | 参数 | 返回 | 说明 |
|---|---|---|---|---|
| 1 | `get_api_info` | — | `ApiInfo` | 启动时调一次 |
| 2 | `get_system_info` | — | `SystemInfo` | |
| 3 | `get_config` | — | `Config` | |
| 4 | `set_config` | `config: Partial<Config>` | `Config` | 部分更新，返回合并结果 |
| 5 | `scan_versions` | — | `string[]` | legacy，实例化后优先 `list_instances` |
| 6 | `list_versions` | — | `VersionSummary[]` | 全量（约 920 条），前端本地筛选 |
| 7 | `open_path` | `path` | `()` | 不存在报 `PERMISSION` |

`SystemInfo = { os, cpuCores, totalMemGB, freeMemGB, demo }`

### 5.2 任务

| 命令 | 参数 | 返回 |
|---|---|---|
| `get_tasks` | `filter?: { state?, kind?, instanceId? }` | `Task[]`（最近 50 条 / 24 小时） |
| `get_task` | `taskId` | `Task \| null` |
| `cancel_task` | `taskId` | `{ accepted: boolean }` |
| `retry_task` | `taskId` | `Task`（新 taskId，`retryOf` 指向原任务） |

- 取消是**请求**：已开始的单文件下载会跑完，任务进入 `cancelled`。
- 终态保留 24 小时或最近 50 条，超出丢弃（`get_task` 返回 null）。
- 去重：同一 `instanceId` 上 `launch` / `create` / `duplicate` 只允许一个非终态任务，重复调用返回 `BUSY`。

### 5.3 实例

| 命令 | 参数 | 返回 |
|---|---|---|
| `list_instances` | — | `Instance[]` |
| `get_instance` | `id` | `Instance \| null` |
| `start_create_instance` | `{ name, gameVersion, loaderKind?, loaderVersion?, memoryMb? }` | `Task`（`result.kind="create"`） |
| `start_duplicate_instance` | `id, name` | `Task`（`result.kind="duplicate"`） |
| `update_instance` | `id, patch: InstancePatch` | `Instance` |
| `delete_instance` | `id, opts?: { trash?: boolean }` | `Config` |
| `set_active_instance` | `id \| null` | `Config` |
| `get_active_instance` | — | `Instance \| null` |

```ts
interface InstancePatch {
  name?: string;
  memoryMb?: number | null;
  jvmArgs?: string[];
  javaPath?: string | null;
  cover?: string | null;
}
```

**删除规则 [P0-2]**

| 情形 | 行为 |
|---|---|
| 该实例有运行中的 session | `INSTANCE_BUSY`，不删 |
| 该实例有非终态任务 | `INSTANCE_BUSY`，不删 |
| `activeInstanceId === id` | 删除成功后置 `null`，返回新 `Config`，界面应回到空状态 |
| `trash: true`（默认） | 先 `rename` 到 `instances/.trash/<id>-<ts>`；rename 失败 → 整体失败（`DISK_FULL`/`PERMISSION`），**原目录保持不动** |
| `trash: false` | 直接递归删；删除中途失败返回错误，已删部分不回滚（文档明示） |

### 5.4 加载器

| 命令 | 参数 | 返回 |
|---|---|---|
| `list_loaders` | `gameVersion` | `LoaderCatalog[]` |
| `start_install_loader` | `instanceId, kind, version` | `Task`（`result.kind="loader"`） |

### 5.5 整合包

| 命令 | 参数 | 返回 |
|---|---|---|
| `start_install_modpack` | `{ source, isUrl?: boolean, name?: string }` | `Task`（`result.kind="modpack"`） |
| `export_modpack` | `instanceId, format: "mrpack"` | `string`（路径） |

### 5.6 模组

| 命令 | 参数 | 返回 |
|---|---|---|
| `list_mods` | `instanceId` | `ModFile[]` |
| `set_mod_enabled` | `instanceId, fileName, enabled` | `ModFile` |
| `delete_mod` | `instanceId, fileName` | `()` |

**重命名冲突 [P0-2]**：启用/禁用靠改后缀，若目标名已存在，依次尝试 `name-1.jar[.disabled]`、`-2`…最多 20 次，仍冲突返回 `INVALID_NAME`。

### 5.7 下载

| 命令 | 参数 | 返回 |
|---|---|---|
| `start_download` | `{ url, kind, fileName, instanceId, onConflict? }` | `Task`（`result.kind="download"`） |

```ts
type ResourceKind = "mod" | "modpack" | "datapack" | "resourcepack" | "shaderpack";
type OnConflict = "overwrite" | "rename" | "skip";   // 默认 "rename"
```

**实例隔离 [P0-1]**

- `instanceId` **必填**（`kind="modpack"` 除外，可省略表示存到全局 `modpacks/`）。
- 无效 id → `INSTANCE_NOT_FOUND`，**不回落全局目录**。
- 落盘目录 = `instance.dirs[kind 对应目录]`（见 §7），下载前会 `create_dir_all`。

**文件名规则 [P0-2]**

`fileName` 由 Rust 校验，**不做静默修正**，不合法直接 `INVALID_NAME`：

1. 必须与 `Path::file_name()` 结果一致（不含 `/`、`\`、`..`）；
2. 不得为 Windows 保留名：`CON PRN AUX NUL COM1-9 LPT1-9`；
3. 不得以空格或 `.` 结尾；
4. 长度 ≤ 200 且总路径长度 ≤ 240；
5. 解析后的绝对路径必须仍在目标目录内（`canonicalize` 前缀检查，防符号链接逃逸）。

冲突处理：`overwrite` 覆盖；`rename` 追加 `-1`…；`skip` 不下载且 `result.replaced=false`、`bytes=0`。

### 5.8 登录

| 命令 | 参数 | 返回 |
|---|---|---|
| `msa_begin` | — | `{ deviceCode, userCode, verificationUri, expiresIn, intervalSec }` |
| `msa_poll` | `deviceCode` | `MsaStatus`（pending 是正常返回） |
| `get_account` | — | `Account \| null` |
| `logout` | — | `()` |

### 5.9 存档 / 统计 / 日志 / 诊断

| 命令 | 参数 | 返回 |
|---|---|---|
| `list_saves` | `instanceId?` | `SaveEntry[]`（省略 = 全部实例） |
| `get_stats` | `range` | `Stats` |
| `read_log` | `{ instanceId?, lines?, follow? }` | `string[]` |
| `prepare_diagnosis` | `instanceId?` | `DiagnosisPreview` |
| `start_diagnose` | `{ instanceId?, sendLogs: boolean }` | `Task`（`result.kind="diagnose"`） |

**诊断隐私 [P1]**

```ts
interface DiagnosisPreview {
  lines: number;
  estimatedTokens: number;
  preview: string;          // 前 100 行（已脱敏）
  redactions: string[];     // ["绝对路径→<GAME_DIR>","用户名→<USER>","IP→<IP>"]
}
```

流程固定：`prepare_diagnosis` → 界面展示预览与「将发送 N 行 / 约 M tokens」→ 用户确认 → `start_diagnose({ sendLogs: true })`。`sendLogs:false` 时只发规则命中摘要，不发日志正文。

`read_log(follow:true)` 只返回**当前快照**并附 `followToken`，后续增量走 `astral://log` 事件（P2 实现，先返回快照）。

### 5.10 会话

| 命令 | 参数 | 返回 |
|---|---|---|
| `get_active_session` | `instanceId?` | `GameSession \| null` |
| `list_sessions` | `{ instanceId?, limit? }` | `GameSession[]` |

### 5.11 约定路径

| 用途 | 来源 |
|---|---|
| 游戏根目录 | `get_config().gameDir` |
| 实例及子目录 | `instance.dir` / `instance.dirs.*` |
| 全局模组/资源 | 由 `start_download` 返回的 `path` |
| 启动器日志 | `%APPDATA%/Astral/logs` |

---

## 6. 事件

### 6.1 `astral://task`

```ts
payload: Task      // 完整快照；按 revision 合并
```

### 6.2 `astral://game-exit`

```ts
payload: {
  sessionId: string;
  taskId: string | null;
  instanceId: string;
  exitCode: number | null;
  crashed: boolean;
  durationMs: number;
  endedAt: string;
}
```

### 6.3 `astral://instance-changed`

```ts
payload: { instanceId: string; change: "created" | "updated" | "deleted"; }
```

删除/改名后即使界面没主动刷新也能同步（尤其是当前实例被删）。

### 6.4 legacy（保留到 v4）

| 事件 | payload | 替代 |
|---|---|---|
| `astral://progress` | `[version, Progress]` | `astral://task` |
| `astral://file` | `[filename, count, total, size, totalSize]` | `astral://task` |

---

## 7. 实例目录布局 [P0-1]

```
%gameDir%/
├─ assets/                     共享
├─ libraries/                  共享
├─ runtime/                    共享（自动安装的 JVM）
├─ modpacks/                   整合包缓存（全局）
└─ instances/
   ├─ .trash/<id>-<ts>/        回收站
   └─ inst_7f3a91c2/
      ├─ instance.json         元数据（id/name/gameVersion/loader/memoryMb/jvmArgs/javaPath/cover）
      ├─ mods/
      ├─ config/
      ├─ saves/
      ├─ logs/
      ├─ resourcepacks/
      ├─ shaderpacks/
      ├─ screenshots/
      └─ options.txt
```

启动参数：

- `--gameDir <instance.dir>` → 存档、config、logs、截图都落在这里；
- `--assetsDir <gameDir>/assets`、`--assetIndex` 走共享；
- `-Djava.library.path` 走共享 natives；
- `-Xmx` 取 `instance.memoryMb ?? 自动`；`jvmArgs` 追加在默认参数之后。

**隔离约定**：任何按实例的读写都必须以 `instance.dir` 为根；全局目录只放共享资源。跨实例写入 = bug。

---

## 8. 页面 → 命令映射

### 启动页

| 区块 | 命令 / 事件 |
|---|---|
| 账号 | `get_account` |
| 当前实例卡 | `get_active_instance`、`get_instance` |
| 启动按钮 | `start_launch` → `astral://task` → `astral://game-exit` |
| 运行状态恢复 | `get_active_session()`（不是靠任务） |
| 版本列表 | `list_instances` |
| 存档 | `list_saves` |
| 本周统计 | `get_stats("7d")` |
| AI 卡 | `get_tasks({ kind: "diagnose" })` |
| 快捷操作 | `open_path` |
| 公告 | 前端静态资源，不走命令 |

### 实例页

`list_instances` / `get_instance` / `start_create_instance` / `start_duplicate_instance` / `update_instance` / `delete_instance` / `set_active_instance` / `list_loaders` / `astral://instance-changed`

### 下载中心

`list_versions` / `start_install` / `get_tasks` / `cancel_task` / `retry_task`

### 模组中心

搜索：界面直连 Modrinth（`https://api.modrinth.com/v2`，对方提供 CORS）
安装：`start_download`（`instanceId` 必填）
已装：`list_mods` / `set_mod_enabled` / `delete_mod`
整合包：`start_install_modpack`

### 设置

`get_api_info` / `get_config` / `set_config` / `get_system_info` / `read_log` / `open_path`

---

## 9. 兼容与迁移 [P0-5]

### 9.1 参数名不变

`set_config` 的 RPC 参数**仍然是 `config`**：`invoke("set_config", { config })`。变的只是「传部分字段也合法」，传完整对象等价于全量更新。**旧调用零改动。**

### 9.2 legacy 命令保持阻塞语义

| legacy 命令 | 语义 | 返回 |
|---|---|---|
| `install_version(version)` | **等到真正装完**才返回 | `()` |
| `launch_version(gameVersion)` | 装完并起进程后返回 | `pid: number` |
| `download_to(url, kind, fileName)` | 等到文件落地 | `path: string` |

旧页面依赖「返回即完成」，这几条**永远不改语义**，内部转成 Task 只是实现细节。新代码用 `start_*`。

### 9.3 字段迁移

| v2 | v3 | 处理 |
|---|---|---|
| `Instance.healthy: boolean` | `Instance.health: {ok, reasons}` | v3 起只返回 `health`；`healthy` 不再提供 |
| `start_create_instance` 返回 `Task` | 同 | `result.kind` 明确为 `"create"` |
| `delete_instance` 返回 `()` | 返回 `Config` | 便于同步当前实例 |
| `download_to` 无 `instanceId` | `start_download` 必填 | legacy 命令仍写全局目录，仅供旧页面 |

### 9.4 版本号

`get_api_info().apiVersion = 3`。界面应：
- 缺失 `apiVersion` → 按 v1 处理；
- `< 3` → 隐藏实例/模组入口，只保留原版安装与启动；
- `>= 3` → 全功能。

---

## 10. 变更记录

| 版本 | 日期 | 内容 |
|---|---|---|
| v1 | 2026-10-23 | 初版：9 命令 / 2 事件 / 提案接口 |
| v2 | 2026-10-23 | Task 模型、`game-exit`、实例 id 解耦、错误码化、`msa_poll` 状态机 |
| v3 | 2026-10-23 | 按界面评审修订：实例目录布局与强制实例隔离、删除/覆盖/文件名规则、任务 revision 与订阅顺序、`GameSession` 与状态恢复、v1 参数与阻塞语义、`get_api_info`、`instance-changed`、诊断预览脱敏 |
| v3.1 | 2026-10-23 | 回填接入评审：`start_install`/`start_launch`/`start_create_instance` 签名、`get_tasks` 顶层字段、`Instance.managed`、能力清单以注册为准、5 项实现缺陷修正、取消语义说明 |
| v3.2 | 2026-10-23 | 模组命令落地：`list_mods` / `set_mod_enabled` / `delete_mod`（仅托管实例） |

### P2 待议（本版不实现）

- 回收站恢复（`restore_instance`）
- 封面与目录的统一访问层（`get_paths()`）
- 统计缓存与增量更新
- `read_log(follow)` 的增量订阅与取消（现只返回快照）

---

## 11. v3.1 修订（按界面接入评审回填）

> 界面在接入时核对了契约与实现，本版把差异回填，不改变既有语义。

### 11.1 新增/修正的签名

```ts
// 安装（原 §4.2 只写了命令名）
start_install({ gameVersion: string }): Task
//   result: { kind: "install", gameVersion: string }

// 启动（同上）
start_launch({ instanceId: string }): Task
//   result: { kind: "launch", sessionId, pid, startedAt }
//   done 仅表示进程已创建；运行状态看 §5.10

// 任务查询：IPC 是顶层字段，不是 filter 对象
get_tasks({ state?: TaskState, kind?: TaskKind, instanceId?: string }): Task[]

// 创建实例（v3 只在内部有 create，v3.1 公开为任务命令）
start_create_instance({
  name: string, gameVersion: string,
  loaderKind?: LoaderKind, loaderVersion?: string, memoryMb?: number
}): Task
//   result: { kind: "create", instance: Instance }
```

### 11.2 `Instance.managed`（新增字段）

```ts
managed: boolean   // true = instances/<id> 托管实例；false = 从 versions/ 兼容出来
```

- 兼容实例的 `dir` / `dirs.*` 指向**共享游戏目录**，不得开放改名、删除或实例内写入。
- 界面必须据此区分，不能假设所有实例都隔离。

### 11.3 能力清单以实际注册为准

`get_api_info().features` 只列真实注册的能力，当前为：

```
instances, sessions, tasks, saves, stats, mirror
```

**不含 `mods`**（模组命令尚未实现）。`apiVersion >= 3` 不等价于全功能，界面须以 `features` 门控入口。

### 11.4 已修正的实现缺陷

| 缺陷 | 修正 |
|---|---|
| `list_saves(instanceId)` 传了无效 id 会回落到全局 `saves/` | 改为返回 `INSTANCE_NOT_FOUND`，绝不跨实例展示 |
| `sessionId` 每进程从 1 计数，重启后与历史记录冲突 | 改为 `sess_<毫秒时间戳>_<序号>`，跨进程唯一 |
| 任务取消后，迟到的进度仍会覆盖终态 | `Task` 改为**终态粘滞**：`done/error/cancelled` 之后忽略任何更新 |
| 实例 `memoryMb` / `jvmArgs` 保存了但启动没消费 | `spawn` 现在拼 `-Xmx{memoryMb}M` + 自定义 JVM 参数（vendored `portablemc` 新增 `set_extra_jvm_args`）；`javaPath` 仍未消费，界面先不开放 |
| `astral://instance-changed` 从未发射 | `update_instance` / `delete_instance` / `start_create_instance` 成功后发射 |

### 11.5 取消语义（补充说明）

`cancel_task` 返回 `{ accepted: boolean }` 仅表示**请求已受理**。当前后台 worker 没有协作停止检查，已进入的下载可能继续完成；任务终态本身不会被后续进度覆盖（见 11.4）。**界面在 worker 支持停止前不要开放取消按钮。**

### 11.6 尚未实现（界面不要放入口）

`retry_task`、`start_duplicate_instance`、`list_loaders`、`start_install_loader`、`start_download`（实例级）、`start_install_modpack`、`export_modpack`、`get_account` / MSA、`prepare_diagnosis` / `start_diagnose`。

> 模组命令（`list_mods` / `set_mod_enabled` / `delete_mod`）已于 v3.2 实现，见 11.8。

### 11.8 模组命令（v3.2 实现）

| 命令 | 参数 | 返回 |
|---|---|---|
| `list_mods` | `instanceId` | `ModFile[]` |
| `set_mod_enabled` | `instanceId, fileName, enabled` | `ModFile` |
| `delete_mod` | `instanceId, fileName` | `()` |

- **仅托管实例**（`Instance.managed === true`）；兼容实例返回 `UNSUPPORTED`。
- 禁用通过把 `x.jar` 改名为 `x.jar.disabled` 实现；同名冲突依次加 `-1`…`-20`，调用方须以返回的 `fileName` 为准。
- `delete_mod` 为物理删除，无回收站。
- `ModFile` 的 `name` / `version` / `loader` 由 jar 内元数据（fabric/quilt json、forge/neoforge toml、mcmod.info）尽力解析，缺失为 `null`；`iconPath` 暂恒为 `null`。

### 11.7 桥接代码

`docs/api-contract-bridge.md` 提供 `types.ts` / `bridge.ts` 的完整新增块，界面可直接粘贴。

