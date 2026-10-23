# Forest Tale · 核心 v3 接入计划

执行进度：本文保留基于 `a3b0154` 的计划快照；后续 v3.2 的共享层与首页 / 实例管理实现、验证及现有限制见 [v3.2 接入记录](forest-tale-v3.2-integration.md)。本文的“本轮不修改”指当时的计划编写轮次。

日期：2026-10-23。UI 负责人：Codex；核心与共享桥接代码提供方：DSH。

本轮不修改 bridge.ts / types.ts，等 DSH 给出共享层代码块后再应用。

## 1. 核对范围与前提

- 工作区：`D:\软件开发\astral-ui`；分支：`feature/forest-ui`。
- 现有 UI 已先提交并推送：`3c10b9f`（正式 React 界面）和 `7d1c6be`（原型、素材、验证脚本与文档）。
- 只读核心基点：`a3b015470997ea3a60825b382e1668ead0ad85a4`，来自本地 `feature/core-features`；对象库共享，无需获取远程或进入核心工作树。
- 已读取 `docs/api-contract.md` v3、`docs/05-W4-核心实现.md`，并对照命令注册和 DTO。`git diff 8ce34f3..feature/core-features --stat` 为 12 个文件、2612 行新增、21 行删除；变化在 Rust、Cargo 和核心文档，共享 TS 文件未改变。
- 现有 UI 仍调用 v1 接口，当前 UI 分支的 Rust 仍是原基点。本文的“已实现”表示核心分支中已注册，**不表示 UI 工作区已具备这些接口或已完成原生联调**。本轮不合并、不复制核心代码、不操作核心索引。
- 保持 B「萤火夜航」的全幅场景、窄侧栏和启动坞。先接真实数据，存档与统计作为次级内容，不给首页堆满卡片。

## 2. 页面与组件 → 命令 / 事件（对照契约 §8）

### 首页 `Launch.tsx`、实例选择器、`useLocalLauncher.ts`

- **当前实例卡 / 你的世界**：`get_active_instance`、`get_instance({ id })`、`list_instances`；选择后调用 `set_active_instance({ id })`，等返回的 Config 确认再更新全局选中态。对照 §8：增加持久化切换步骤；现有 `scanVersions(): string[]` 和仅内存的版本选择，改为按稳定 `Instance.id` 管理。`gameVersion` 只用于展示和安装，不充当实例 ID。
- **进入世界 / 启动坞 / 底部状态栏**：`start_launch({ instanceId })`、`astral://task`、`get_tasks` / `get_task`、`get_active_session({ instanceId })`、`astral://game-exit`。对照 §8：补充任务快照和会话查询，避免只凭启动任务显示持续运行；具体顺序见第 4 节。
- **存档**：`list_saves({ instanceId })`，始终传当前实例 ID。对照 §8：先做当前实例视图；不使用“省略 ID = 全部实例”的承诺，因为 W4 实际省略时读的是全局 `saves/`。切换实例先清除旧列表，再读新列表。
- **本周游玩**：`get_stats({ range: "7d" })`。对照 §8：数据已有，第二期接小型摘要；`deaths` / `achievements` 为 null 时隐藏对应指标，不能显示 0。统计当前基于保留的已结束会话，不宣称完整历史或包含正在进行的游玩时长。
- **帐号 pill / 帐号弹窗 / Sidebar 头像**：目标为 `get_account` 与未来 MSA 流程。对照 §8：W4 未注册这些命令，继续显示“未登录 / 离线档案”，配置用户名不等于已验证身份；不主动调用未实现命令。
- **AI 入口**：点击只打开 `AiDrawer`。对照 §8：`get_tasks({ kind: "diagnose" })` 要等诊断实现后才显示分析记录；没有任务不生成成功记录或修复建议。
- **快捷目录**：使用 `Instance.dir` / `dirs.*` 和会报错的 `open_path` 新包装，不手工拼路径。封面 / 存档图标用经配置允许的本地资源访问方式；读取失败使用森林通用插画，不把插画称为真实存档截图。
- **公告**：目前没有独立公告区；若新增，遵循 §8 使用前端静态内容，不制造在线公告或在线人数。

### 版本管理 `Versions.tsx` → 实例管理

- 列表、搜索、详情：`list_instances` / `get_instance({ id })`；卡片显示名称、版本、加载器、`health`、`modsCount`、`modsDisabled` 和数据目录。搜索仍在前端进行。
- 切换：`set_active_instance({ id })` / `get_active_instance`。没有激活实例时允许浏览列表，提示“选择一个实例”，不静默写入第一个实例。
- 编辑：托管实例使用 `update_instance({ id, patch })`。仅发送用户修改字段；成功后以返回的 Instance 更新并重新读取当前实例。清空 `memoryMb` / `javaPath` / `cover` 的 null 语义先由 DSH 验证，JVM 参数与内存确实被启动命令消费后再开放相应设置。
- 删除：仅对明确 `managed === true` 的托管实例开放；确认框展示实例名称、目录以及 mods/config/saves 将一起移入应用回收站。只调用 `delete_instance({ id, opts: { trash: true } })`；不提供永久删除入口。成功后采用返回的 Config，重拉列表和当前实例；当前 ID 变 null 就回到未选状态。`BUSY` / `INSTANCE_BUSY` 时保留卡片并解释原因；失败不显示“已删除”，重拉实际状态处理部分成功。
- 新建 / 复制：契约 §8 的 `start_create_instance` / `start_duplicate_instance` 在 W4 的 invoke 注册中缺失，虽然存在内部 `instance::create` 函数，也不能直接从 UI 调用。暂不开放操作；等公开命令和 Task 结果就绪再做创建向导、复制确认与进度。
- 加载器：§8 的 `list_loaders` / `start_install_loader` 下一批实现。现阶段只展示返回的 loader，null 显示“原版”，不生成安装列表。
- 同步：契约要求 `astral://instance-changed`，W4 尚未发现事件发射。首期在写操作返回后、窗口重新聚焦时和手动刷新时重拉；有事件后订阅再取快照，合并重复刷新请求。
- **隔离差异必须可见**：W4 比契约多返回 `managed: boolean`。`true` 才是 `instances/<id>/` 的独立实例；`false` 是 `ver_<version>` 兼容实例，`dir` 及子目录仍指向共享游戏根目录。卡片标注“兼容版本 · 共享游戏目录”，不开放改名、删除或实例隔离写入，不承诺独立 mods/config/saves。缺失 `managed` 时按归属未知保守处理。

### 下载中心 `Download.tsx`、任务列表

- 版本目录：继续 `list_versions`，前端搜索、分类、分批渲染。
- 安装：目标 `start_install({ gameVersion })` → Task → `astral://task` / `get_tasks`。对照 §8：该命令已在 W4 注册，但契约 §5 缺正式签名条目；请 DSH 补齐 `{ gameVersion: string } -> Task`。安装共享版本资源并不等于已创建独立实例；完成后重拉实例 / 版本清单，不自动伪造托管实例。
- 进度：按 taskId 展示 stage + label；`done/total`、`bytes/totalBytes` 仅用于当前阶段且分母大于 0 时才显示比例。未知总量用不定进度；不把阶段百分比当总进度，不从累积 bytes 直接推断实时速度。
- 切页后任务继续在 App 级 store 中维护；页面重挂载和刷新靠快照补齐，不仅依赖当前已挂载页面的局部 state。
- 对照 §8：`cancel_task` 已注册，但 W4 取消只更新注册表，后台 worker 未看到协作停止检查；后续进度 / finish 仍可能覆盖终态。取消按钮先不开放，等停止写入与终态语义验证；`accepted: true` 只可表示请求受理。`retry_task` 尚未注册，不能用重新调用 start 命令伪装成带 retryOf 的重试。
- 迁移前继续沿用阻塞到完成的 `installVersion`；同一次用户操作只走 v1 或新 Task 路径之一，不重复安装。

### 模组中心与社区资源 `Resources.tsx`

- 搜索与详情：按 §8 保留前端 Modrinth 请求、加载 / 空 / 网络错误和取消过期搜索；搜索结果不代表本机已安装。
- 安装到实例：目标 `start_download({ url, kind, fileName, instanceId, onConflict: "rename" })`。选择明确的托管实例，提交时固定目标 ID；切换首页实例不改变已发起任务的目标。只有 Task 成功结果才能显示“文件已下载”；模组可用状态还需重新读取 `list_mods`，加载器兼容性未知不能标绿。
- 已装模组：目标 `list_mods` / `set_mod_enabled` / `delete_mod`；启禁用后采用返回的实际 fileName，不在 UI 猜重命名后缀。删除需明确确认，未声明回收站保证前不标成可撤销操作。
- 整合包：`.mrpack` 下载和 `start_install_modpack` 创建可玩实例分别展示。加载器、整合包安装 / 导出及本地模组 CRUD 均等下一批核心完成。
- 对照 §8：以上新命令 W4 尚未注册，不能因 `features` 包含 `mods` 就启用。旧 `downloadTo` 仍是兼容下载，不得用来实现“安装到某个独立实例”；现有“已下载”语义保留，入口明确目标是共享目录，浏览器禁止落盘。数据包缺少存档目标语义，待 DSH 明确世界级落盘规则后再宣称已安装到世界。

### 设置 `Settings.tsx`

- 能力：`get_api_info`；外观主题、强调色、圆角、减少动效仍由 UI localStorage 管理。
- 配置：`get_config` / `set_config({ config: patch })`。新包装只提交本次编辑的下载源和离线用户名，成功采用返回的合并 Config，避免旧快照覆盖刚切换的 activeInstanceId。旧 `getConfig` / `setConfig` 的签名和行为保持。
- 系统：继续 `get_system_info`，只在原生端且 `demo === false` 时展示设备值；不据此宣称所选 JVM 或实例内存设置已生效。
- 目录：读取核心给出的路径，用严格传播错误的 open_path 包装；本期不开放游戏根目录迁移。
- 对照 §8：`read_log` 尚未注册，诊断仍使用用户手选文件；W4 Config 未返回契约中的 `maxConcurrentDownloads` / `javaPath`，这些控件暂不开放，不能显示保存成功后实际无效的选项。

### AI 抽屉、帮助与共享外壳

- `AiDrawer.tsx`：保留当前本机文件预览、清除和未连接状态。后续按契约固定流程 `prepare_diagnosis` → 预览脱敏范围 / 行数 / 估算 tokens → 用户明确确认 → `start_diagnose` → Task；服务未实现时禁止分析和修复按钮，不上传日志。
- `More.tsx`：静态帮助无需核心命令；能力未开放的说明与实际门控同步。
- `App.tsx` / `Sidebar.tsx` / 状态栏：共用能力、实例、任务、会话 store；不会因为某页卸载而清空真实任务。路由、Modal、Icon、主题切换不新增后端依赖。TitleBar 的窗口操作继续使用现有桥接。

## 3. 希望 DSH 提供的共享层增量导出

以下为待应用的代码块需求，不是本轮修改清单。所有导出保持新增方式；保留 legacy 的 `scanVersions`、`launchVersion`、`installVersion`、`downloadTo`、`getConfig`、`setConfig`、`onProgress` 等既有签名和阻塞语义。

### `types.ts`：DTO 与错误分层

- 按契约 §4 新增 `ApiInfo`、`ApiErrorCode`、`LoaderKind`、`InstanceHealth`、`InstanceDirs`、`Instance`、`InstancePatch`、`TaskKind`、`TaskState`、`Task`、`TaskResult`、`GameSession`、`Stats`。Task 保留完整字段，result 按 `result.kind` 收窄；`stage` / `label` 是核心值，不固化成 UI 枚举。
- `Instance` 增加 W4 实际字段 `managed?: boolean`：缺失表示未确认，只有严格为 true 才允许托管写入。请 DSH 将该差异回填契约，后续双方约定后再收紧类型。
- 现有 `SaveEntry` 是 `lib/data.ts` 使用的设计示例类型（desc/when/size）。**不要直接替换同名类型**；新增 `CoreSaveEntry` 对应 §4.7（name/dir/gameVersion/lastModified/sizeBytes/iconPath），真实 UI 使用新类型。
- 新增 `CoreConfig extends LauncherConfig`：保留旧三字段，source 收窄为 `"official" | "bmclapi"`，增加 `activeInstanceId: string | null`。W4 尚缺的 `maxConcurrentDownloads?: number` / `javaPath?: string | null` 暂按可选解码；这是实现过渡，契约目标仍是必填。没有字段时不默认显示已配置值。`CoreConfigPatch` 首期仅允许修改 source / username，实例选择通过专用命令。
- 新增 `ApiErrorPayload = { code: ApiErrorCode; message: string; detail: string | null }`，供 Task.error 使用；JSON 对象不能直接当成运行时 Error 实例。桥接导出的 `ApiError extends Error` 用于 Promise 抛错，并保留 code / detail。
- 新增以下请求 / 事件类型，字段名使用实际 IPC 的 camelCase：

```ts
type Unlisten = () => void;
type StatsRange = "7d" | "30d" | "all";
type TaskFilter = { state?: TaskState; kind?: TaskKind; instanceId?: string };
type SessionFilter = { instanceId?: string; limit?: number };
type CoreConfigPatch = Partial<Pick<CoreConfig, "source" | "username">>;
type DeleteInstanceOptions = { trash?: boolean };
type GameExitPayload = {
  sessionId: string;
  taskId: string | null;
  instanceId: string;
  exitCode: number | null;
  crashed: boolean;
  durationMs: number;
  endedAt: string;
};
type InstanceChangedPayload = {
  instanceId: string;
  change: "created" | "updated" | "deleted";
};
```

### `bridge.ts`：首页与实例，第一优先

```ts
getApiInfo(): Promise<ApiInfo>;
listInstances(): Promise<Instance[]>;
getInstance(id: string): Promise<Instance | null>;
getActiveInstance(): Promise<Instance | null>;
setActiveInstance(id: string | null): Promise<CoreConfig>;
startLaunch(instanceId: string): Promise<Task>;
getActiveSession(instanceId?: string): Promise<GameSession | null>;
listSessions(filter?: SessionFilter): Promise<GameSession[]>;
onTask(cb: (task: Task) => void): Promise<Unlisten>;
onGameExit(cb: (event: GameExitPayload) => void): Promise<Unlisten>;
```

包装规则：`getInstance` / `setActiveInstance` 用 `{ id }`；启动和会话查询用 `{ instanceId }`；`listSessions` 展开为 `{ instanceId, limit }`，不套 `filter`。可选值省略，不把 undefined 转成字符串。监听 Promise 真正建立后才查询快照，卸载后迟到的 Unlisten 也必须立即执行。

浏览器的新写操作拒绝为 `UNSUPPORTED`，新查询不能生成真实实例 / 会话 / 成功任务；页面保留“浏览器预览 · 未连接”。若未来增加示例模式，必须由 UI 单独显式开启并与真实状态分开。

### `bridge.ts`：任务、管理与设置，第二优先

```ts
getTasks(filter?: TaskFilter): Promise<Task[]>;
getTask(taskId: string): Promise<Task | null>;
startInstall(gameVersion: string): Promise<Task>;
updateInstance(id: string, patch: InstancePatch): Promise<Instance>;
deleteInstance(id: string, opts?: DeleteInstanceOptions): Promise<CoreConfig>;
listSaves(instanceId: string): Promise<CoreSaveEntry[]>;
getStats(range: StatsRange): Promise<Stats>;
getCoreConfig(): Promise<CoreConfig>;
patchConfig(config: CoreConfigPatch): Promise<CoreConfig>;
openPathChecked(path: string): Promise<void>;
parseApiError(cause: unknown): ApiError;
```

- `getTasks({ kind, state, instanceId })` 要调用 `invoke("get_tasks", { kind, state, instanceId })`。契约写作 filter 对象，W4 实际 Rust 参数是顶层三字段，不能传 `{ filter }`。
- `startInstall` 使用 `{ gameVersion }`；`updateInstance` 使用 `{ id, patch }`；`deleteInstance` 使用 `{ id, opts }`；`patchConfig` **必须传 `{ config }`，不是 `{ patch }`**。旧 `setConfig(): Promise<void>` 不改；新函数返回合并结果。
- `listSaves` 的 UI 包装先要求显式非空实例 ID。核心仍需修复失效 ID 回落全局的问题；不能把返回的其他目录存档展示成当前实例的世界。
- `parseApiError` 只切前两个 `|`，detail 保留剩余分隔符；无编码的 legacy 文本和未知错误码按 INTERNAL 保留原消息。Task 中的 error DTO 单独解码 / 校验，不把对象 stringify 成 `[object Object]`。
- `openPathChecked` 传播错误，不能沿用现有 `openPath` 吞错路径；不在失败时展示已打开。

### 暂缓导出：等核心真实实现后再给代码块

```ts
cancelTask(taskId: string): Promise<{ accepted: boolean }>; // 后台停止语义验证后
retryTask(taskId: string): Promise<Task>;                  // 新 ID + retryOf
onInstanceChanged(cb: (event: InstanceChangedPayload) => void): Promise<Unlisten>;
startCreateInstance(request: CreateInstanceRequest): Promise<Task>;
startDuplicateInstance(id: string, name: string): Promise<Task>;
listLoaders(gameVersion: string): Promise<LoaderCatalog[]>;
startInstallLoader(instanceId: string, kind: LoaderKind, version: string): Promise<Task>;
startDownload(request: DownloadRequest): Promise<Task>;
listMods(instanceId: string): Promise<ModFile[]>;
setModEnabled(instanceId: string, fileName: string, enabled: boolean): Promise<ModFile>;
deleteMod(instanceId: string, fileName: string): Promise<void>;
startInstallModpack(request: InstallModpackRequest): Promise<Task>;
exportModpack(instanceId: string, format: "mrpack"): Promise<string>;
getAccount(): Promise<Account | null>;
prepareDiagnosis(instanceId?: string): Promise<DiagnosisPreview>;
startDiagnose(request: DiagnoseRequest): Promise<Task>;
```

请求 DTO 直接对齐契约：`CreateInstanceRequest` 为 name/gameVersion 及可选 loaderKind/loaderVersion/memoryMb；`InstallModpackRequest` 为 source/isUrl?/name?；`DiagnoseRequest` 为 instanceId?/sendLogs。`DownloadRequest` 做可辨识联合：kind 非 modpack 时 instanceId 必填；kind 为 modpack 时才可省略，保留 fileName（不要写成 legacy 的 filename）和 onConflict。MSA 与 read_log 在对应功能期再细化；当前 read_log 的字符串数组 / followToken 描述存在冲突，先不写会掩盖差异的包装。

## 4. 启动中 / 运行中 / 已退出与刷新恢复

三态描述一次真实启动周期；还需单独保留“未启动”“恢复查询中”“启动失败”“状态未知”，不把它们硬塞成已退出。

### 启动与事件顺序

1. App 级 store 先完成 `onTask`、`onGameExit` 订阅，再查询 `getTasks()` 和当前实例的 `getActiveSession()`。两个监听都成功前不启用新启动路径；监听失败展示错误，不能继续后宣称可恢复。
2. 用户点击时固定 instanceId，先加本地提交锁，按钮显示“正在提交启动请求”；await `startLaunch(instanceId)`。命令失败展示真实错误；仅本地点击不等于获得任务。
3. 命令返回的 Task 和事件 / 快照全部进入同一个 merge：按 taskId 查找，只接受更大 revision；终态不退回非终态。queued/running 的 launch Task 表示**启动中**，展示真实 stage/label，不重复提交。
4. launch Task 的 done 只表示进程已创建。读取 `result.kind === "launch"` 的 sessionId，再查询 `getActiveSession(instanceId)`；仅与该次启动对应且 state=running 的 GameSession 才显示**运行中**。会话尚未确认时显示“进程已创建，正在确认运行状态”；查询失败显示状态未知，不能凭 PID 常亮运行标记。
5. 匹配的 `astral://game-exit` 或查询到的 `GameSession.state === "exited"` 才进入**已退出**。退出事件同时校验 instanceId、sessionId；迟到的旧 session 不能清掉新 session。exitCode=null 显示“已结束，退出码未知”；不可自行推断为正常退出，崩溃措辞只依据核心返回且与未知退出码区分。
6. 进程可能在启动结果返回前退出：先缓存按 sessionId 记录的退出事件；拿到 Task.result 后合并。已退出的同一 session 不能被迟到的 running 查询快照复活。会话没有 revision，因此以 sessionId + 退出终态粘滞处理，不依赖事件到达顺序。

### 页面刷新 / 启动器重启

- 从“恢复查询中”开始，重建监听后取任务快照、`getActiveInstance()`、`getActiveSession(instanceId)`；不给 localStorage 中的 PID / 文案恢复运行状态的权威性。
- 若返回 running，显示该真实会话；若返回 starting 或仍有 queued/running 的 launch Task，显示启动中。W4 实际从 session::begin 直接返回 running，启动准备主要由 Task 表达。
- 若返回 exited，显示真实结束记录；若返回 null，则再查 `listSessions({ instanceId, limit: 1 })`，有已结束记录才展示“上次已退出”，没有记录就是未启动。null 不构成“刚刚退出”的证据。W4 会话查询在 PID 对账时可能返回一次 exited，后续查询又变 null，需覆盖这一分支。
- 存在非终态 Task 而会话为空时，只能表述正在准备；任务快照为空也不代表所有后台操作已成功。超出保留范围的 task 查询为 null，显示“任务记录不可用”，不补造完成状态。
- 页面切换保留 App 级 store；实例切换和异步响应带请求代次 / instanceId 校验，旧请求不可覆盖新选择。窗口重新聚焦时补查活动会话；不做高频全量轮询。
- W4 的 Task store 是进程内存，taskId 序列在进程重启后从头计数；首期不把任务缓存跨核心进程持久化，也不承诺重启恢复下载。GameSession 落盘但 sessionId 序列同样从 0 开始，存在与旧记录重复风险；**DSH 修正跨重启 sessionId 唯一性、并验证重启后退出探测前，不验收完整会话恢复**。这不是靠前端缓存可解决的问题。

## 5. 空态、未安装态、错误与 health.reasons

- **浏览器 / 旧核心**：保留森林场景和主题交互，明确未连接或兼容版本模式。没有真实查询结果就不展示已安装数量、游玩时长、帐号登录或运行状态。`get_api_info` 命令不存在可识别为旧核心；其他查询失败保留错误，不能全部伪装成 v1。
- **加载中**：使用占位和“正在读取”，不先显示 0；刷新可保留带“正在更新”标记的旧值，切换实例则先清除归属敏感数据。
- **实例空列表**：使用“暂未发现本地实例”，提供前往安装 / 检查目录 / 刷新。W4 扫描会跳过读取失败的目录，不能把 [] 宣称为已证明磁盘没有实例。未选 active 但列表非空时引导用户选择。
- **未安装 / 文件不完整**：没有 Instance 的远程版本是待安装候选；已有 Instance 且 health.ok=false 标“文件不完整 / 需要检查”，不当作损坏实例自动删除。显示第一条 reasons 摘要，“查看原因”展开完整原文列表。
- **health 的使用**：例如 `缺少版本文件 1.20.1.json`、`缺少客户端 1.20.1.jar`、`缺少 assets 资源目录` 都来自 W4 实际检查。提供“查看原因 / 前往下载中心校验补全 / 刷新”；由用户主动安装相同 gameVersion，成功后重新查询健康状态，不能直接在前端把 ok 设 true。
- 不对中文 reasons 做字符串匹配来决定删除、迁移或自动修复。health 不是安装状态枚举；ok=true 只代表该版本的有限文件检查通过，不代表 Java、模组兼容性、登录或全部校验已通过。展示“基础文件检查通过”，不展示保证性的“所有依赖已就绪”。
- health 缺失、ok=false 却没有 reasons、DTO 不合法：显示“状态无法确认”，保留详情，不默认健康；首期将普通“进入世界”改为检查 / 补全入口，真正再次启动仍以核心校验和真实结果为准。
- **错误**：NETWORK 保留重试读取；PERMISSION 展示受影响目录及手动检查；DISK_FULL 提示腾出空间；INSTANCE_NOT_FOUND 重拉并清空失效选择；BUSY / INSTANCE_BUSY 关联正在进行的任务 / 会话，不能提示删除重装；JAVA_MISSING 提示检查 Java 配置；AUTH_REQUIRED / AUTH_EXPIRED 引导尚未接入或已就绪的真实登录流程；VERIFY_FAILED 展示校验失败。未知错误原文可展开，不自动清理文件。
- **模组 / 存档 / 统计空态**：成功读取后仍没有条目才显示“暂未发现”；读取错误单独展示，不替换为 0。未知大小、版本、图标和时长使用“— / 未知”或占位。统计数值 0 只有真实返回时才使用；lastPlayed=null 显示“暂无游玩记录”。
- **操作成功**：保存、切换、安装、删除、下载均等真实返回 / 终态；按钮按下、请求受理和取消受理都不是操作完成。失败保留用户输入，提供安全重试，不伪造 Toast 成功。

## 6. 分期与接入前需要 DSH 对齐的差异

### 第一期：先恢复真实实例与启动状态

请 DSH 按“类型 / 错误与能力 → 首页实例 → 启动 / 会话 / 任务订阅”提供独立增量代码块。先接查询、选择、任务进度和会话状态；UI 保留 legacy 回退，能力不存在时不发新命令。

接入前核对：

1. **能力清单**：W4 features 包含 `mods`，但对应命令缺失；`apiVersion >= 3` 也不能直接等价全功能。DSH 应修正清单 / 给出细粒度能力（如创建、复制、重试），两侧以已实现能力启用。契约 §9.4 的“>=3 全功能”需结合 §4.11 的 features 约束。
2. **会话可靠性**：持久化 sessionId 必须跨进程唯一；重启后仍在运行的进程退出，应能经查询发现，即使未重新建立 child.wait。确认 PID 对账不会把无关进程当游戏；已结束会话不阻塞后续启动。以上做原生验收，不能只测 TS 假接口。
3. **实例归属**：把 managed 与兼容实例共享目录行为补入契约；创建 / 复制 API 未注册前不把“实例 CRUD 完整”作为前端假设。

### 第二期：实例编辑、下载任务、存档统计与设置

按 `Versions → Download → 首页次级内容 → Settings` 提供代码块；已有注册命令可先做展示和受限编辑，以下差异补齐后开放对应动作：

- `start_install` / `start_launch` 正式参数条目和 get_tasks 顶层 IPC 字段补进契约；目前按读取到的真实实现准备包装。
- 任务取消需 worker 停止检查和不可逆的终态约束；重试需真实新 ID / retryOf；明确任务保留 / 重启范围，不把内存注册表描述成持久任务恢复。
- `list_saves` 省略 ID 当前是全局而非全部实例；无效 ID 当前会回落全局，必须改成明确错误，防止跨实例误展示。扫描失败也应能区分权限错误与空列表。
- `astral://instance-changed` 尚未发射，先用主动刷新；以后补事件后再承诺外部变化自动同步。
- Config 缺少全局 javaPath / 并发字段；实例已有 memoryMb / jvmArgs / javaPath，但 W4 start_launch 调用 core::spawn 时没有传入这些设置，需 DSH 明确落实后再展示生效。null 清回默认要做 IPC 往返验证。
- 删除到回收站后保存 Config 若失败，可能出现磁盘已移动但调用报错；UI 必须重拉，不盲目重复删除，核心侧应补足一致性保证。

### 第三期：模组 → 加载器 → 整合包 → 帐号与诊断

等待 DSH 补齐真实命令、能力与验证后，再给对应 DTO / bridge 代码块。优先实例隔离的 start_download 和本地模组查询 / 启禁用；之后加载器和整合包创建；登录、诊断最后接。不能以 legacy 全局下载代替实例安装，也不能以安装成功替代可玩验证。

## 7. 验收与本轮验证

后续联调须在双方确认、具备新核心的集成环境执行；不擅自合并核心分支。本机测试使用独立测试实例 / 游戏目录，避免两个 AI 同时写用户配置与真实世界。

- 首页：无实例、未选实例、兼容实例、文件不完整、查询失败、任务返回早于 / 晚于事件、瞬间退出、重复点击、切换实例时旧请求返回。
- 任务与会话：先订阅后快照、revision 乱序、终态粘滞、退出先于启动结果、WebView 刷新、核心重启后 PID 对账 / 会话 ID 不冲突。取消不再继续写文件，重试 ID 与 retryOf 正确。
- 隔离：两个托管实例分别具有自己的 mods/config/saves；相同游戏版本不共享这些用户数据。兼容实例明确标共享，失效 ID 不回落到其他目录。
- 管理：运行中 / 任务中禁止删除，移入回收站失败保留原目录，删除当前实例清空 active，配置保存失败不显示成功。未获授权不测试永久删除。
- 视觉：保留 B 方案；接入后的长名称、多原因、未知总进度、深浅主题、窄屏、焦点和减少动效均截图复查。

本轮只进行了静态接口核对和文档编写，没有启动游戏、改用户游戏配置、运行核心自检或宣称原生联调通过。提交现有 UI 前重新执行 TypeScript `tsc --noEmit` 与 Vite 生产构建，均通过（52 模块）；已有阶段二浏览器验证记录为 75 项通过、0 失败，详见 `forest-tale-phase-2.md`。本轮未改变界面行为，因此未新增截图或重复浏览器测试；截图 / JSON 结果按用户要求留在本机并忽略。
