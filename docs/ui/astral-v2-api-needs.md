# Astral V2：给 DSH 的接口需求（提案，尚未实现）

日期：2026-10-24。以 main `6868a47` 的契约 v3.3 和实际注册／实现为准。此文不修改契约或共享层，也不声称下列新命令存在。命名可由 DSH 调整，但返回语义、隔离和失败恢复需要双方确认。

**本轮不修改 bridge.ts / types.ts；等 DSH 给出共享层代码块后再应用。** 新能力必须通过 get_api_info.features 门控，不能凭 apiVersion≥3 推断所有组件可用。保留 legacy 原签名，不将其“返回即完成”语义改成 Task。

## P0-A：下载 Minecraft → 新版本配置的完整安装计划

建议 DTO：

```ts
type LoaderSelection = { kind: "fabric"|"quilt"|"forge"|"neoforge"; version: string };
type ComponentSelection = { provider: "modrinth"|"official"; projectId: string; versionId: string };
type GameInstallRequest = {
  name: string; gameVersion: string; loader: LoaderSelection | null;
  shaderSupport: ComponentSelection | null; optionalComponents: ComponentSelection[];
};
type PlanIssue = { code: string; message: string; blocking: boolean; componentId?: string };
type PlannedFile = { id: string; displayName: string; size: number|null;
  role: "required"|"optional"; action: "add"|"reuse"|"replace"; relativePath: string };
type GameInstallPlan = {
  planId: string; revision: number; expiresAt: string; request: GameInstallRequest;
  target: { proposedInstanceId: string; displayPath: string; isolated: true };
  files: PlannedFile[]; issues: PlanIssue[]; downloadBytes: number|null;
};
// prepare_game_install({request: GameInstallRequest}): GameInstallPlan
// start_game_install({planId: string, revision: number}): Task
// Task.result = {kind:"game-install", instance: Instance}
```

Rust 分配安全 ID 和真实目录，拒绝覆盖现有目录；串起游戏、Loader、光影支持与必要依赖。失败不把半成品标为健康，也不删除用户既有数据。计划过期或已有文件变化要重新校验，不能只信前端路径或 flags。完成才发 instance-changed，并以实际返回实例加入列表。下载阶段总量不明返回 null／0，不生成整体假百分比。

先解决仅原版和 Fabric 的完整链路；Forge/NeoForge 不满足时保留禁用与原因。不能靠前端先 create 再 install 便宣布整体完成。

## P0-B：组件目录与兼容性

现有 `list_loaders({gameVersion})` / `start_install_loader({instanceId,kind,version})` 先补共享 TS 包装。现有目录实际只返回 Fabric，需核对契约提到的 Quilt；创建时写 loaderKind/version 并不会完成安装。

建议独立扩展命令，避免破坏旧返回：

```ts
type ComponentBuild = { id: string; kind: string; version: string;
  gameVersions: string[]; loaders: string[]; publishedAt: string|null;
  stability: "stable"|"beta"|"alpha"|"unknown";
  recommended: boolean; recommendationReason: string|null;
  required: { projectId: string; versionId: string|null }[];
  available: boolean; unavailableReason: string|null };
// get_component_catalog({gameVersion:string, loader?:LoaderSelection|null}): {
//   mainLoaders: ComponentBuild[]; shaderSupport: ComponentBuild[];
//   optionalComponents: ComponentBuild[]; fetchedAt:string; stale:boolean
// }
```

需要列出所有实际兼容构建，推荐只是入口；无 stable 字段则 unknown，不推断。必须区分“尚未实现”“暂时无法查询”“确实无适配”。光影支持组件 Iris/Oculus/OptiFine 与 shaderpack 不同。Fabric API 不应对所有 Fabric 版本一律标必需，依据实际依赖解析。OptiFine 的分发授权与获取策略由核心核实，未落实不开放。

## P0-C：资源详情与安全定向安装

```ts
type ResourceKind = "mod"|"resourcepack"|"shaderpack"|"datapack";
type ResourceTarget = { instanceId:string; worldId?:string };
type ResourceFile = { versionId:string; projectId:string; version:string;
  channel:"release"|"beta"|"alpha"; publishedAt:string; featured:boolean;
  gameVersions:string[]; loaders:string[]; fileId:string; fileName:string;
  size:number; hashes:{sha512?:string;sha1?:string}; dependencies:unknown[] };
// get_resource_project({provider:"modrinth",projectId:string}): {
//   id:string;title:string;authors:{name:string,url:string|null}[];
//   description:string;downloads:number;updatedAt:string;homepage:string;
//   license:{id:string,url:string|null};iconUrl:string|null
// }
// list_resource_files({provider:"modrinth",projectId:string,target:ResourceTarget,
//   channel?:"release"|"beta"|"alpha",cursor?:string}): {
//   files:ResourceFile[];nextCursor:string|null;issues:PlanIssue[]
// }
// prepare_resource_install({target:ResourceTarget,kind:ResourceKind,
//   provider:"modrinth",projectId:string,versionId:string,fileId:string}): {
//   planId:string;revision:number;expiresAt:string;target:ResourceTarget;
//   files:PlannedFile[];dependencies:{projectId:string;versionId:string|null;
//     role:"required"|"optional"|"incompatible";present:boolean|null}[];
//   issues:PlanIssue[];downloadBytes:number|null
// }
// start_resource_install({planId:string,revision:number,
//   selectedOptionalDependencyIds:string[],approvedReplacementIds:string[]}): Task
// Task.result = {kind:"resource-install",instanceId,worldId?,files:[...]}
```

仅 managed===true 可写；路径必须由 Rust 从目标 ID 解析，Mod→mods、资源包→resourcepacks、光影包→shaderpacks；数据包必须 worldId→saves/<安全世界目录>/datapacks。禁止 worldName/绝对路径替代 worldId，拒绝穿越、symlink/junction 逃逸、同名静默覆盖。按文件 ID 而非“总是最新版”安装，验证哈希与下载来源，写临时文件后原子替换。运行中/写任务中遵守 INSTANCE_BUSY。

已有 list_saves 缺少安全稳定的世界 ID 时，请扩展独立 world descriptor；显示名称不作路径标识。需判断世界版本／数据包格式兼容性，不能仅以项目声称支持游戏版本作为最终通过。

建议错误：UNSUPPORTED、NETWORK、INSTANCE_NOT_FOUND、INSTANCE_BUSY、VERSION_NOT_FOUND、INCOMPATIBLE、DEPENDENCY_CONFLICT、PLAN_EXPIRED、TARGET_CHANGED、INVALID_PATH、FILE_EXISTS、HASH_MISMATCH、DISK_FULL。可沿用或新增实际错误码；message 面向玩家，detail 可复制，retriable 明确，失败结果说明已写文件及回滚情况。

## P1-D：整合包专用向导

```ts
// inspect_modpack({provider:"modrinth",projectId:string,versionId:string}): {
//   gameVersion:string;loader:LoaderSelection|null;files:PlannedFile[];
//   overrides:string[];issues:PlanIssue[];diskBytes:number|null
// }
// prepare_modpack_install({provider:"modrinth",projectId:string,
//   versionId:string,name:string}): GameInstallPlan // 可定义对应专用 Plan
// start_install_modpack({planId:string,revision:number}): Task
```

始终创建新配置，默认不覆盖已有世界。检查 zip-slip、压缩炸弹、外链与覆盖文件、许可／下载授权，支持失败清理与用户文件保护；不要仅将压缩包下载到 mods 便显示成功。复制配置另需专门命令和是否包含 saves 的显式选项，未支持时禁用。

## P1-E：账号、Java 与启动行为

- 已有 start_launch、get_active_session、get_tasks、astral://task、astral://game-exit、instance-changed 继续用。
- `get_account(): {activeId:string|null,accounts:{id:string,kind:"offline"|"microsoft",name:string,avatarUrl:string|null,status:"offline"|"authenticated"|"expired"|"unavailable",ownsGame:boolean|null}[],capabilities:{microsoftAuth:boolean}}`。只 authenticated 且真实授权／所有权检查通过才显示正版已登录。需要配套开始授权、轮询、取消、选择和退出账号，token 不给前端或日志。
- 启动进度目前只有 resolve/jvm/assets/download 等事实事件；无独立账号检查阶段时不编造。需要 `launch-check` 细分可增加枚举和 label，total=null 表示非确定。**默认继续显示“游戏进程运行中”**，若要说“游戏窗口已打开”，需独立可信状态 `process-running/window-visible/game-ready/unknown` 及来源、instanceId、sessionId；OS 窗口存在仍不能代表主菜单就绪。
- javaPath 当前未消费：先落实校验与启动消费，再开放自定义 Java。建议 `list_java_runtimes(): {id,path,major,architecture,valid,reason}[]`、`validate_java({path,instanceId}): {compatible,reason,major,architecture}`，路径用户选择且由 Rust 校验。
- 全局下载设置可另加 get/update_download_settings（source、maxConcurrent、bandwidthLimit、retries），有范围校验与持久化。现有 FullConfig 没这些字段，不凭旧 LauncherConfig 字段推定已支持。
- 启动后保持／最小化／关闭窗口需要宿主策略与进程监听生命周期，关闭 UI 不应杀游戏或丢退出事件；落实后再暴露设置。动画、壁纸、主题为 UI 偏好，与游戏配置分开。

## P1-F：真实队列、恢复、重试与诊断

- Task IDs 当前仅进程内序号；若要跨启动恢复，增加全局唯一 ID 与持久化任务快照、boot/session epoch。restart 后明确 interrupted／reconcile，不能把旧 running 一直亮着。
- 取消须 worker 协作停止并确认不再写，当前不开放。retry 应创建新 Task 并指向 retryOf，保留原失败详情；无命令不做假重试按钮。
- Task 总阶段、阶段内单位、bytes/totalBytes 缺失要区分 unknown，不把文件数百分比当总体安装百分比。
- 日志／诊断建议 `list_logs({instanceId,sessionId?})`、`read_log({logId,limit})`、`prepare_diagnosis({logIds}) → {previewId,redactedText,warnings}`，用户确认后才分析／上传；文件路径白名单、内容脱敏，不读取任意私聊或密钥。原型复制的是明确标记的示例错误。

## 页面接入顺序与 DSH 最小交付

1. **首页／版本管理**：复用当前 useInstances + mergeTask +真实 Session；增加账号能力门控；不拆掉已工作恢复逻辑。health.reasons[0] 简述，其余“查看原因”；缺文件引导修复/下载，绝不改 ok。
2. **原版下载／安装二级页**：先 listVersions + Fabric 的 TS wrapper、再 P0-A 的安全安装计划。list_loaders 的 Quilt 差异先解决；只点亮已验证组合。
3. **资源详情**：项目与全部具体文件查询→目标／世界→依赖计划→Task。优先 Mod，再资源包/光影包，最后存档级数据包；各目录规则不能模糊。
4. **整合包／队列／高级设置**：有专用计划、恢复和运行时语义后接入。

DSH 请以追加的 DTO、bridge 函数、错误枚举、feature 名称和真实注册命令对应清单交付，不直接在 UI 工作树编辑共享文件。契约描述与实际返回需一致，提供成功、未知进度、失败、迟到事件、INSTANCE_BUSY 与兼容性冲突样例，UI 再做接入。
