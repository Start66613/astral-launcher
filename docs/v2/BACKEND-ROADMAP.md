# Astral Launcher 后端路线图（V2.5）

> 分支：`feature/astral-v2-core-next`（从 `feature/astral-v2-core` 的 `e0ebb7c` 切出）
> 本文记录"哪些做完了、哪些没做、为什么这个顺序"，给后来接手的人和 Codex 看。
> 契约细节见 `BACKEND-API-ADDENDUM.md`，测试证据见 `BACKEND-TEST-REPORT.md`。

---

## 0. 一句话现状

A1–A5、B1–B4、C1–C4、D1–D2 已完成并有自动化测试；D3 的测试矩阵已随各功能逐项落地；
D4 的 CI 已跑通（fmt / clippy / check / build / selftest / audit），发布检查清单与自动更新
签名方案尚未完成。共 177 个 Tauri 命令、380 项自检、379 个 Rust 单元测试（373 通过 6 忽略）。

---

## 1. 分批与完成状态

### 第一轮 A：下载与任务底座（已完成）

| 项 | 内容 | 状态 | 关键实现 |
|---|---|---|---|
| A1 | 真可取消下载引擎 | ✅ | `vendor/portablemc/src/stream.rs` + `net::download` + `portablemc::download::Batch::set_cancel` |
| A2 | 断点续传与下载缓存 | ✅ | `net::PartMeta` + `cache.rs` |
| A3 | 并发与调度 | ✅ | `net::acquire_slot` / `scheduler_stats` |
| A4 | 网络与文件安全 | ✅ | `net::validate_url` / `ensure_within` / `modpack` 解压防护 |
| A5 | Task 与 GameSession 可靠性 | ✅ | `task.rs` 状态机 + `session::pid_alive` |

**为什么先做 A**：后面所有功能（B/C/D）都要下载、都要报进度、都可能被取消。
如果底座是"取消只在阶段边界生效"，那么每个新功能都会继承这个缺陷，越往后越难改。

**A1 的关键设计**：没有去改 portablemc 原有的 `download.rs`（那是 Mojang 安装流程在用的
批量下载器，改它风险太大），而是新增 `stream.rs` 提供单个文件的 `fetch()`：
Range 续传、逐 chunk 取消检查、重定向逐跳过白名单。`net.rs` 在其上再加文件锁、
`.part` 元数据、原子提交、重试退避、缓存。旧的 `download_atomic` / `download_verified`
签名保留，内部换实现，所以 40 多处调用点不用一次性全改。

**后来补的一刀**：安装游戏本体 / 加载器走的是 portablemc 自己的 `Batch`，跟 `net::download`
是两条路。2026-10 补上了 `Batch::set_cancel` 与 `base::Installer::set_cancel`——
批量下载器在每个分片读完一块时检查取消，未派发的分片直接标记为 `EntryErrorKind::Cancelled`，
`install_dyn` 里再把它翻译成 `Error::DownloadResourcesCancelled`。这样"取消安装"才真的会停，
而不是界面显示已取消、后台继续下几百个库。


### 第二轮 B：Mod 与文件完整性（已完成）

| 项 | 内容 | 状态 | 关键实现 |
|---|---|---|---|
| B1 | 递归依赖解析 | ✅ | `deps.rs`（逻辑与 IO 分离，`DepSource` trait）+ `ledger.rs`（已装 Mod 的版本账本，解析能认出装的是哪个版本） |
| B2 | 批量更新与回滚 | ✅ | `update_plan.rs`（批次备份 + 整批回滚） |
| B3 | 完整文件检查与修复 | ✅ | `verify.rs`（四态分开报，不合成 healthy） |
| B4 | 整合包增强 | ✅ | `modpack.rs`（env 过滤 / 本地导入 / 来源记录 / 磁盘预检） |

**B1 为什么抽出 `DepSource`**：递归解析里最难的从来不是"怎么递归"，而是"怎么在不联网的
情况下证明递归是对的"。把取数抽成 trait，测试用假 source 就能确定性地造出
环、深链、同项目双版本、已装复用这些场景，不用打真 API。

**B2 为什么是"整批回滚"而不是"逐条回滚"**：批量更新一旦中途失败，玩家面对的是
"一半新一半旧"的 mods 目录，这比全旧更难排查。所以只要有任意一条失败，就整批回滚到
更新前，并把历史记录一并清掉（回滚过的批次不该再出现在"可回滚"列表里）。

**B3 为什么坚持四态**：`exists / complete / compatible / launchable` 是四件不同的事。
一个 jar 存在但大小不对是"存在不完整"；一个 jar 完整但需要 Java 21 而机器只有 17 是
"完整但不兼容"。合成一个 `healthy: bool` 会让界面只能说"有问题"，说不出是什么问题。

### 第三轮 C：数据安全（已完成）

| 项 | 内容 | 状态 | 关键实现 |
|---|---|---|---|
| C1 | 可恢复版本回收站 | ✅ | `recycle.rs` |
| C2 | 世界存档备份恢复 | ✅ | `world_backup.rs` |
| C3 | 版本复制导入迁移 | ✅ | `migrate.rs` |
| C4 | Java 增强 | ✅ | `java_audit.rs` |

**C 轮的统一原则**：默认不永久删除、覆盖前先留底、破坏性操作必须显式 `confirm`。
`purge_trash` / `purge_all_trash` / `delete_world_backup` / `remove_managed_java` /
`clear_java_cache` / `cache_clear` / `meta_clear` 全部要求 `confirm: true`，否则返回
`UNSUPPORTED|...需要明确确认`。这不是防玩家，是防"手滑点错"和"前端漏传参数时静默删库"。

### 第四轮 D：缓存、诊断、测试、发布（部分完成）

| 项 | 内容 | 状态 | 说明 |
|---|---|---|---|
| D1 | 离线缓存 | ✅ | `net::fetch_json_cached` + `meta_*`，标注数据来源与新鲜度；Modrinth 全部查询已切到缓存版 |
| D2 | 崩溃诊断增强 | ✅ | `logs.rs` 12 条规则 + `cause`/`confidence`/`category` |
| D3 | 测试矩阵 | 🟡 随功能落地 | 见 §3，没有单独补"凑数"测试 |
| D4 | CI 与发布 | 🟡 CI 完成，发布清单与签名方案未做 | 见 §4 |

### 第五轮 E：资源浏览（本轮新增）

| 项 | 内容 | 状态 | 说明 |
|---|---|---|---|
| E1 | Modrinth 资源搜索 | ✅ | `search.rs` + `search_resources`，标准百分号编码、白名单校验、离线回退 |
| E2 | 资源包/光影包文件管理 | ✅ | `packs.rs` + `list_packs`/`set_pack_enabled`/`delete_pack`，解析 `pack_format` 并给出适配版本区间 |
| E3 | 本地 Mod 静态扫描 | ✅ | `modscan.rs` + `scan_mods`，离线查重复 mod id、缺失依赖、版本不满足、声明冲突；不联网、不改文件 |
| E4 | 实时日志跟随 | ✅ | `live.rs` + `tail_log`，按 offset 增量读、只给完整行、重写自愈、超长行不死等 |
| E5 | 服务器状态查询 | ✅ | `server.rs` + `ping_server`，直连服务器列表协议（modern + legacy 回退），离线不报错 |
| E6 | 收藏的服务器列表 | ✅ | `servers.rs` + 6 个命令，自管 `.astral-servers.json`（不动 `servers.dat`），8 线程批量刷新、每条独立成败 |
| E7 | 存档详情（读 `level.dat`） | ✅ | `nbt.rs`（只读 NBT）+ `worlds::detail` + `get_world_info`，真实版本/难度/上次游玩，坏文件只丢信息 |
| E8 | 整合包导出增强 | ✅ | `export::plan` + `plan_modpack_export` 预览；config 递归收集、可带资源包/光影包；跳过符号链接、限条数与总量；`.part` 原子落地；修纯 overrides 包被误拒 |
| E9 | 存档管理（改名/复制/回收站） | ✅ | `save_manage.rs` + 7 个命令；删除只进回收站、恢复覆盖先留底、Windows 文件名规则、运行中一律拒绝 |
| E10 | 截图浏览 | ✅ | `screenshots.rs` + 3 个命令；PNG 头读尺寸不解码、data URL 走 IPC、8 MB 上限、删需确认 |
| E11 | 游戏设置读写 | ✅ | `game_options.rs` + 2 个命令；30 项白名单、其余行原样保留、整批校验、改前备份、运行中拒绝 |
| E12 | 实例封面 | ✅ | `cover.rs` + 3 个命令；图片收进实例目录、按魔数识别、旧任意路径值一律当没有 |
| E13 | 崩溃报告解析 | ✅ | `crash.rs` + `read_crash_report`；按报告格式拆时间/描述/异常/原因链/系统详情，可疑模组只认报告写的，推断部分明确标注 |
| E14 | 诊断包导出 | ✅ | `support.rs` + 2 个命令；一份 zip 收齐日志/崩溃报告/体检/Java/Mod/任务，先预览再导出，脱敏在本机做完 |
| E15 | 启动前预检 | ✅ | `preflight.rs` + `preflight_launch`；7 项一次列全（运行中/进行中的任务/用户名/版本文件/Java/内存/Mod 冲突），只读不启动，内存建议按机器与 Mod 数推算 |
| E16 | 归档日志（`.gz`）可读 | ✅ | `logs.rs` 不再跳过 `.log.gz` / `.txt.gz`；边解边留尾部、64 MB 解压上限、`compressed` 字段；实时跟随明确拒绝归档文件 |
| E17 | 下载速度与剩余时间 | ✅ | `task.rs` 的 `progress_bytes` 把字节数真正写进任务；两次采样算速度与 ETA（间隔 <200 ms 不重算、进度回退重新采样），安装/启动/整合包/资源/更新/修复全链路都接上 |
| E18 | 下载源测速 | ✅ | `source_probe.rs` + `probe_sources`；只测 DNS 与 TCP 连接延迟（不下文件），多次取样取中位数，官方源与最快镜像差距 <80ms 时优先官方，全不通时建议为 null |
| E19 | 本地文件导入 | ✅ | `local_import.rs` + 2 个命令；按内容（不是扩展名）判断是 Mod / 资源包 / 光影包，先预览再导入，重名加序号不覆盖，复制后回读校验并算 sha1，一个失败不影响同批其他 |
| E20 | 存档导出 / 导入 | ✅ | `world_io.rs` + 3 个命令；导出成 zip 发给别人、收别人的 zip 进来；先预览包内容（有没有 level.dat、解压多大、建议叫什么），导出先写 `.part`，导入逐条防 Zip Slip 与符号链接，重名默认拒绝、覆盖先留底，游戏运行中一律拒绝 |
| E21 | 游戏服务器列表导入 | ✅ | `servers_dat.rs` + 2 个命令；只读游戏自己的 `servers.dat`（未压缩 NBT），玩家勾选后收进启动器的收藏列表，已经在列表里的跳过并说明原因，不写游戏文件一个字节 |
| E22 | 磁盘占用分析 | ✅ | `disk_usage.rs` + `analyze_disk_usage`；按 assets/libraries/versions/instances/cache/jvm/logs/meta 分类，再按实例拆到 mods/saves/资源包，列出最大文件；只读，标清哪些能安全清理 |
| E23 | AI 崩溃诊断（BYOK） | ✅ | `ai.rs` + 6 个命令；密钥用 Windows DPAPI 加密存本机、明文永不出后端，只发脱敏内容且玩家先看预览，本地规则结论一并给模型减少幻觉，按输入哈希缓存 20 条；没 Key / 断网 / 模型乱答都明确报错不假装分析 |
| E24 | 结束正在运行的游戏 | ✅ | `game_stop.rs` + `stop_game`；默认发 `WM_CLOSE` 等游戏自己存盘退出（15 秒），超时不替玩家做决定而是回「没停下来，要不要强制」；强制结束才 `TerminateProcess`；动手前后都比对 pid 创建时间，pid 被复用绝不误杀；玩家自己结束不算崩溃 |
| E25 | 游戏目录识别与切换预检 | ✅ | `game_dir.rs` + `inspect_game_dir`，`set_config` 落盘前先校验；挡空路径/相对路径/文件/盘符根，报告是否像 Minecraft 目录、有几个版本/实例/存档、能不能写、还剩多少空间，切换前先给玩家看清单 |
| E26 | 整合包更新检查与更新计划 | ✅ | `pack_update.rs` + `check_modpack_update` / `plan_modpack_update`；查作者有没有发新版本，新旧 index 逐文件对比出新增/删除/改动，游戏版本或加载器变了直接判阻塞，本地导入的包明说查不了，计划里写清不碰存档 |
| E27 | 实例整体快照与还原 | ✅ | `snapshot.rs` + 5 个命令；把 mods/config/saves/资源包打包成一个 zip（logs 与截图不收，记录里写明跳过了什么），还原前校验 sha256、解到暂存目录、把现有实例整个搬到 `snapshots/replaced/` 而不是删除，删除快照需确认 |
| E28 | 内存与 JVM 参数校验 | ✅ | `jvm.rs` + `check_instance_jvm`；写配置时就挡掉 0 MB / 超过物理内存 / 把 `-Xmx` 写进自定义参数 / 引号没配对 / 参数条数超限，超过建议上限只提醒不拦；预检里多一项「JVM 参数可用」 |
| E29 | 一键进服 | ✅ | `quick_join.rs` + 3 个命令；1.20+ 用 `--quickPlayMultiplayer`，1.19- 用 `--server/--port`，版本号读不出来时走老参数；地址与端口校验，收藏与临时地址两种入口，`preview_server_join` 只读预览 |
| E30 | 整合包更新执行 | ✅ | `pack_apply.rs` + 3 个命令；按计划先备份再改、失败整批回滚，计划指纹对不上要求重新预览，回滚记录留 5 份，不碰存档 |
| E31 | 日志搜索与级别筛选 | ✅ | `log_search.rs` + `search_log`；关键词搜索（可选大小写敏感）、按 error/warn/info/debug/trace 过滤、各级别条数统计；超大日志只扫尾部并标明，命中过多只返最近的 |
| E32 | Mod 图标 | ✅ | `mod_icon.rs` + `read_mod_icon`；认 Fabric / Quilt / Forge-NeoForge 三种声明与根目录兜底，列表给 `hasIcon` 不解压图片，图标上限 512 KiB，无图标不是错误 |
| E33 | 项目详情页 | ✅ | `project_detail.rs` + `get_project_detail`；图集（只收 https、封面排最前、最多 20 张）、介绍正文（按字符截到 2 万）、分类与附加分类、客户端/服务端支持、关注数与相关链接；离线时标 `stale` |
| E34 | 日志清理 | ✅ | `log_maintenance.rs` + `log_usage` / `plan_log_cleanup` / `prune_logs`；先预览再删，`latest.log` 永远保留，只认 `logs/` 与 `crash-reports/` 里的文本日志，删前要确认 |
| E35 | 数据包管理 | ✅ | `packs::world_dir` + `list_packs`/`set_pack_enabled`/`delete_pack` 新增 `worldId`；数据包挂在存档自己的 `datapacks/` 里，只认 `.zip`，存档没了报 `TARGET_CHANGED` |
| E36 | 游戏设置备份恢复 | ✅ | `game_options::list_backups`/`restore_backup`/`delete_backup` + 3 个命令；列出历史备份（时间、行数、渲染距离）、恢复前把当前文件也留底、同名备份不覆盖、删除要确认、运行中拒绝 |

| E37 | 存档统计 | ✅ | `world_stats.rs` + `get_world_stats`；读游戏自己写的 `stats/` 与 `advancements/`，给出游戏时长、死亡、击杀、挖掘、距离、进度完成数；新旧两套键名都认，配方不算进度，坏文件只提示不拖垮页面 |
| E38 | 存档图标与打开文件夹 | ✅ | `world_icon.rs` + `read_world_icon` / `open_world_folder`；游戏自己写的 `icon.png` 读成 data URL（尺寸从 PNG 头读，不解码像素），没有图标返回 null 不算错；顺手能在文件管理器里打开存档目录 |
| E39 | 资源包图标 | ✅ | `packs::read_icon` + `read_pack_icon`；列表只给 `hasIcon` 布尔（打开 zip 顺手判断），真要看图再取 data URL；`pack.png`/`pack.jpg`/`logo.png` 都认，超过 512 KB 当没有，坏包不算错误 |
| E40 | 实例的游戏参数 | ✅ | `game_args.rs` + `get_game_args` / `set_game_args`；窗口大小这类参数终于有地方填了，存在实例自己的 `game-args.json`；`--username`/`--accessToken`/`--gameDir` 等 12 个启动器负责的参数一律拒绝（写错了会顶掉登录信息）；一键进服的参数排在自己配的后面 |
---

## 2. 关键取舍（写给后来的人）

1. **没有引入新的下载库**。`reqwest` 已经在依赖树里（portablemc 用），自己写
   `stream.rs` 比引入 `reqwest-download` 之类的库更可控：取消点在哪、重定向怎么校验、
   `.part` 什么时候落盘，全都能自己定。
2. **没有把 `net.rs` 拆成多文件**。它现在约 1400 行，确实偏大，但下载、校验、
   锁、缓存、调度、元数据缓存是同一件事的不同侧面，拆开会产生大量 `pub(crate)` 噪音。
   真要拆，建议按"下载 vs 元数据"切成 `net/` 目录，而不是按函数类型切。
3. **`lib.rs` 依然很胖**（约 5800 行）。它是命令层 + 自检，命令本身都是一层薄封装，
   真正的逻辑都在各模块里。自检放在 `lib.rs` 是因为它要能调用所有模块的内部函数；
   如果将来拆，建议把自检挪到 `selftest.rs`（`#[cfg(feature = "selftest")]`）。
4. **Tauri 命令参数类型必须 `Deserialize`**。`UpdatePlan`、`RepairPlan`、`CopyPlan`、
   `ModUpdate` 这些"计划对象"都要能从 JS 传回来，所以除了 `Serialize` 还得有
   `Deserialize`。这一条踩过坑（编译期报 `CommandArg` 不满足）。
5. **Windows 路径的 `\\?\` 前缀**。`canonicalize()` 在 Windows 上返回 verbatim 路径，
   和 `sysinfo` 给的 `C:\` 形式对不上，做"哪个盘还有多少空间"时必须先剥掉。

---

## 3. 测试策略

- **单元测试**：纯逻辑（解析、分类、命名、路径校验、状态机）放 `#[cfg(test)]`。
  当前 379 个，373 通过、6 个 `#[ignore]`（真实联网的 Mojang 安装测试）。
- **自检（`--selftest`）**：所有需要真实文件系统、真实 HTTP、真实进程的检查都放这里。
  用 `testsrv.rs` 起一个只监听 `127.0.0.1` 的临时服务器，能确定性制造 503、慢速、
  重定向、Range 响应。当前 380 项。
- **为什么不依赖 `cargo test` 跑全部**：本机 `cargo test` 的测试二进制在仓库目录里
  运行会因路径 ACL 报"拒绝访问"（见 `10-cargo-test-调查.md`），所以 `scripts/run-tests.ps1`
  会先把二进制复制到 `%TEMP%` 再跑。CI 上没有这个问题。
- **破坏性测试全部用隔离目录**：`%TEMP%\astral-*` 或 `D:\Astral\*`，从不碰真实 `.minecraft`。

---

## 4. 还没做的（诚实清单）

| 项 | 为什么没做 | 建议 |
|---|---|---|
| 发布产物形态 | 安装包 vs 免安装还没定，清单里的打包步骤就没法定稿 | 先定产物，`RELEASE-CHECKLIST.md` 已按两种都留了口子 |
| 自动更新签名 | 没有签名密钥；任务书要求"无签名不得开放自动下载安装" | 需要用户提供密钥或改用可信源校验（如 GitHub Release + 固定 SHA256） |
| JDK 自动下载 | `jdk.rs` 已能下载 Adoptium，但 UI 侧未接 | 后端接口 `install_java` 已就绪，等前端 |
| MSA 真实登录 | 阻塞在 Azure 应用 client id（用户侧提供） | `account::msa_*` 骨架已在，缺 client id |
| 依赖版本约束（semver 范围） | 当前是"取第一个兼容版本" | Modrinth 的依赖本身不带版本范围，够用；真要做得自己解析 `version_id` 约束 |
| `#[cfg(test)]` 里 6 个 `#[ignore]` 的真实网络测试 | 需要外网且慢，不进 CI | 保留 `ASTRAL_LIVE_*` 环境变量手动触发 |

---

## 5. 下一步建议顺序

1. 前端接入（Codex）：`BACKEND-API-ADDENDUM.md` 里的新命令按页面归组接进去，
   资源搜索页可以直接照 §10 接，日志窗口照 §14 接。
2. 发布产物定型 → 定稿发布检查清单（`RELEASE-CHECKLIST.md` 已在，产物一栏待填）→ 决定自动更新方案。
3. JDK 自动下载接到"运行库"页面（后端已通）。
4. MSA 登录（等 client id）。
