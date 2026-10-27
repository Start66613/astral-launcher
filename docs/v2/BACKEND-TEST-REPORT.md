# 后端测试报告（V2.5）

> 分支 `feature/astral-v2-core-next`
> 数据采集时间：2026-10-26
> 结论：**自检 396 项全过，单元测试 399 通过 / 6 忽略，clippy 与 rustfmt 干净。**

---

## 1. 怎么复现

### 单元测试

```powershell
# 在 D:\软件开发\astral-core\app\src-tauri 下
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
$env:CARGO_PROFILE_DEV_DEBUG = "0"
cargo test --release --lib --no-run
# 测试二进制必须先复制到仓库外再跑（仓库目录 ACL 会让临时目录创建失败）
.\scripts\run-tests.ps1
```

### 自检

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
$env:CARGO_PROFILE_DEV_DEBUG = "0"
cd D:\软件开发\astral-core\app\src-tauri
cargo build --message-format short
Copy-Item .\target\debug\astral.exe D:\Astral\Astral-core.exe -Force
$env:ASTRAL_DATA_DIR = "D:\Astral\selftest-data"
Start-Process D:\Astral\Astral-core.exe -ArgumentList "--selftest"
# 结果写到 %TEMP%\astral-selftest.log 与 %TEMP%\astral-selftest.json
```

`--selftest` 的退出码 = 失败项数，CI 直接用它判定。

---

## 2. 自检覆盖（396 项）

按前缀分组：

| 分组 | 项数 | 覆盖内容 |
|---|---|---|
| `download` | 13 | 普通下载 + SHA1、复用已验证文件、坏哈希拒绝、取消提前停止、断点续传、503 重试、非法重定向拦截、同目标串行化、内网地址拦截、`.part.json` 清理、并发上限、单域名上限、portablemc 批量下载器可取消 |
| `task` | 15 | 生命周期、快照、bootId、取消接受/终态/粘滞、持久化、令牌回收、取消中不被进度覆盖、健康度、去重、重试规则、阶段映射、阶段标签、字节数与速度写入、字节字段能序列化给前端 |
| `verify` | 13 | 缺 jar、缺库、库大小校验、缺资源索引、四态报告、修复计划定位、不动玩家目录、浅检信任大小、深检发现内容损坏、复查反映修复、干净版本无动作、备份列表 |
| `java` | 11 | 主版本表、satisfies 规则（Java 8 上限）、审计标记路径丢失、报告结构、删除需确认、缓存统计、清空需确认、只删目标目录 |
| `instance` | 10 | 创建、发现（托管/遗留）、健康、mods 计数、名称校验、删除进回收站、健康原因 |
| `duplicate` | 2 | 干净的实例也能复制且与原实例互相独立、重名被拒 |
| `session` | 9 | 开始、退出、统计、对账、PID 复用识别、活进程不误判、死亡不活跃、已对账 |
| `plan` | 8 | 安装计划准备/复用/取用/单次、坏名字、Forge、修订守卫 |
| `modpack` | 8 | 路径穿越、符号链接、正常 overrides、env 过滤、本地导入拒绝/解析、来源记录、磁盘预检 |
| `world` | 21 | 备份创建、运行中拒绝、恢复需确认、损坏备份拒绝、存档列表、世界 id、解析、坏 level.dat 仍能列出、从 level.dat 读版本/难度/硬核、优先用存档自报的上次游玩时间、存档详情能读、未知 id 被拒、改名与重名拒绝、非法名、复制、删除进回收站、恢复、恢复冲突、覆盖留底、永久删除需确认 |
| `deps` | 7 | 拓扑序、via 链、环检测、缺失依赖、深度上限、已装跳过、可选跳过 |
| `update` | 6 | 历史、回滚恢复旧文件、回滚清历史、不碰其他 mod、挡 beta 升级 |
| `trash` | 5 | 列表、冲突改名恢复、恢复后清空、只删指定项、拒绝穿越 |
| `meta` | 5 | 新鲜缓存不发请求、断网回落、统计、修剪保留新鲜、清空需确认 |
| `search` | 3 | 真实 HTTP 服务器验证 facets 与解析、二次命中缓存、非法过滤被拒 |
| `retry` | 4 | 拒绝运行中、拒绝缺 spec、拒绝未知类型、接受已知类型 |
| `loader` | 4 | 解析、空安全、兼容性、NeoForge 提示 |
| `mod` | 4 | 列表、启用标记、禁用、启用、删除、路径守卫 |
| `packs` | 8 | 解析 `pack.mcmeta` 的 pack_format 与描述、禁用/启用改名、删除需确认、非法类型与危险文件名被拒、图标标记随列表给出、图标读成 data URL、没有图标不算错误 |
| `modscan` | 5 | 重复 mod id、缺失依赖、版本不满足、计数与结论、干净目录保持安静 |
| `ledger` | 3 | 账本让 Astral 装的进 known、玩家手放的仍 unknown、文件删掉后不再算已装 |
| `cache` | 3 | 第二次走缓存、统计与淘汰、只清缓存不动文件 |
| `diagnosis` | 3 | 结构化报告、干净日志保持安静、脱敏 |
| `logs` | 3 | 归档 `.gz` 能列出并标记 compressed、解压后按行读取、实时跟随明确拒绝归档文件 |
| `live` | 5 | 只给完整行、无新内容不重复、追加增量、重写后 reset、半行留到下次 |
| `server` | 6 | 本地假 MC 服务器跑真实握手、颜色代码剥离、favicon 保留、host:port 写法、离线不报错、非法地址才 Err |
| `servers` | 8 | 增删改查、重复与非法地址、选择性更新、重排、批量刷新各自独立、lastSeen 只记成功、id 过滤、删除只删记录 |
| `screenshots` | 4 | 列表带尺寸、data URL 可显示、非法名被拒、删除需确认 |
| `options` | 4 | 读出带标签、改一项其余原样、非法键与越界整批拒绝、运行中拒绝 |
| `cover` | 4 | 图片复制进实例目录、绝对路径与 `..` 被拒、非图片被拒、能清除 |
| `crash` | 4 | 按报告格式解析、系统详情与原因链、可疑模组只认报告写的、没有报告时报错 |
| `support` | 8 | 预览列出全部小节、预览不落盘、导出 zip、条目齐全、报告能看懂、日志脱敏、机器可读那份带事实、不带日志时如实标注 |
| `preflight` | 4 | 7 项一次列全、缺版本文件时阻塞、内存建议不阻塞、预检不写文件 |
| `worldio` | 4 | 导出能读回来、导入落地到指定名字、重名默认拒绝且覆盖留底、不是存档的包被拒 |
| `serversdat` | 4 | 游戏格式读得出来（名字/地址/隐藏标记）、文件不在时安静返回、坏文件只给说明、能收进收藏列表且重复的被拒 |
| `disk` | 4 | 分类排序、按实例拆分与最大文件、可清理标记诚实、扫完不删文件 |
| `source` | 4 | 本地开端口能连上、关端口报不通、建议逻辑（接近时优先官方、差太多用最快、全不通为 null）、全部源都有标签 |
| `import` | 6 | 按内容认出资源包/光影包/Mod、坏路径被拒、复制到正确目录、重名不覆盖、一批里坏文件不影响其他、指定类型优先 |
| `installer` | 3 | Forge/NeoForge 下载地址 |
| `ai` | 11 | 初始未配置、坏 Key 被拒、存好带末四位提示、磁盘上不是明文、能读回、预览脱敏且不发请求、没 Key 明确报 AUTH_REQUIRED、真实假服务能解析出结论与用量、二次命中缓存不再请求、模型乱答报错、清除干净 |
| `stop` | 6 | 没在跑时明确报 NOT_FOUND、强制结束能真的杀掉进程、进程真的没了、玩家自己结束不算崩溃、pid 被复用绝不误杀、那个进程还活着 |
| `gamedir` | 4 | 空/相对/盘符根被拒、真实目录报告齐全、空目录不误判且可新建、指向文件被挡 |
| `join` | 4 | 1.20+ 用 quickPlay、1.19- 回退老参数、坏地址被拒、从收藏条目拼出带 id 与名字的目标 |
| `packupdate` | 4 | 查出新版本、计划差异正确（增删改与字节数）、拒绝当前版本、本地导入的包明说查不了 |
| `packapply` | 5 | 旧计划被拒、按计划下载新增并删除多余、真要动的文件才备份、来源记录跟着更新、回滚把文件和来源都还原 |
| `logsearch` | 4 | 关键词命中与行号、按级别过滤、大小写开关、非法级别与不存在的日志被拒 |
| `modicon` | 4 | 列表给出 hasIcon、图标读成 data URL、没有图标不算错、非法文件名被拒 |
| `projectdetail` | 4 | 详情字段齐全、正文与图集保留、第二次读走缓存、空 id 被拒 |
| `logmaint` | 5 | 占用统计、计划永远保留 latest.log、删前要确认、删完 latest.log 还在、计划里要删的真没了 |
| `datapack` | 4 | 数据包落进存档自己的 datapacks、存档不存在被拒、只认 zip、禁用靠改名 |
| `optbk` | 5 | 备份能列出、恢复把文件换回来、恢复前留底、删除要确认、恢复拒非法名 |
| `wstats` | 5 | 空存档给出说明、时长与各项计数正确、进度不算配方、坏文件只提示、未知存档被拒 |
| `wicon` | 3 | 没图标不算错、图标读成 data URL 且尺寸正确、能通过 worldId 找到 |
| `snapshot` | 4 | 整个实例打进 zip（含 saves，跳过 logs 并写明）、运行中拒绝、还原后文件回来且旧实例被留底、列表与删除需确认 |
| `jvm` | 7 | 正常内存通过、0 MB 被拒、超过物理内存被拒、超过建议上限只提醒、-Xmx 写进自定义参数被拒、空白参数被清理、上限随机器内存变 |
| `gameargs` | 4 | 窗口大小能填、认证类参数被拒、能存能读、非法值不落盘 |
| `order` | 6 | 一开始按名字排、置顶排最前、置顶标记能读回、手动拖过的顺序生效、取消置顶回落到手动顺序、不认识的 id 被忽略 |
| 其余 | 30+ | 时间、错误编解码、URL 白名单、路径越界、用户名规则、账户、组件目录、调度器、磁盘用量、导出回环、阶段标签、平台规则等 |

---

## 3. 单元测试（405 个）

| 模块 | 数量 | 内容 |
|---|---|---|
| `packs` | 9 | 文件名拆分、pack_format 对照表、description 文本组件、危险文件名、扩展名过滤、删除需确认、数据包只认 zip、world_id 找不到时报 TARGET_CHANGED、图标标记与读图 |
| `modscan` | 7 | 版本比较、常见范围、乱写范围判未知、Forge TOML 依赖、平台 id 白名单、重复+版本不满足、干净目录 |
| `ledger` | 5 | 记录读回、升级替换旧条目、改名与删除跟随文件、文件丢失不算已装、空字段拒绝 |
| `instance_order` | 9 | 文件不存在为空、置顶排最前、取消置顶归位、重复置顶不重复、手动排序未提到的接后面、丢掉未知 id、坏文件当没设过、无偏好时保持原序、置顶压过手动顺序 |
| `deps` | 9 | 嵌套顺序、环、双版本冲突、深度/节点上限、已装跳过、release 优先、缺失、via 链 |
| `verify` | 5 | 相对路径、需要修复、干净不需、计划不碰玩家文件、空计划 |
| `logs` | 9 | 脱敏（邮箱/IP/token/用户目录）、常见故障分类、cause/confidence、新规则、干净日志、`.gz` 解压读取与尾部截断 |
| `live` | 6 | 只给完整行、按 offset 续读、文件变短 reset、上限与 pending、日志不存在报错、挑最新日志 |
| `server` | 10 | VarInt 往返、字符串长度按字节、聊天组件拍平、颜色代码剥离、状态解析、坏 favicon、地址拆分、老协议解析、非法地址 |
| `nbt` | 9 | 标量与数组往返、LevelInfo 解析、缺字段保持 null、截断数据被拒、荒谬长度被拒、非 compound 根被拒、未压缩文件可读 |
| `worlds` | +4 | 详情读全、坏 level.dat 只丢信息、未知 id 报错、列表带版本 |
| `save_manage` | 9 | 改名、重名拒绝、非法名、复制、删除进回收站、恢复、恢复冲突、覆盖留底、永久删除需确认、运行中拒绝 |
| `screenshots` | 6 | base64 补齐、PNG 尺寸、只列图片、data URL、路径穿越与非图片被拒、删除需确认 |
| `world_io` | 7 | 导出回读、运行中与已存在拒绝、带目录包导入、平铺包按给定名导入、非存档包被拒、覆盖留底、运行中拒绝 |
| `servers_dat` | 5 | 读出名字与地址、跳过空地址、缺 servers 键不算错、文件不在时安静、写好的文件读得回 |
| `disk_usage` | 6 | 递归统计、缺目录为零、找最大文件、分类与实例排序、大缓存提醒、可清理标记 |
| `game_options` | 10 | 键值解析、往返不丢尾换行、范围与枚举校验、读文件保留未知键、坏值标记、只改白名单、整批拒绝、运行中拒绝、缺文件时新建、备份裁剪 |
| `cover` | 7 | 只认管理文件名、魔数识别、PNG/JPEG 宽高、设读往返、换格式删旧、拒非图片、清除 |
| `crash` | 8 | 头部字段、异常与栈帧、原因链与小节、系统详情、推断包名、显式可疑模组、None 不误报、普通日志不当崩溃报告 |
| `support` | 8 | 路径与用户名替换、长用户名才换、短名不乱换、先换路径再脱敏、超长留尾部、JSON 字段齐全、小节大小计算、文件名安全 |
| `preflight` | 7 | 原版大内存机器、加载器与 Mod 抬高建议、封顶一半内存、小机器保底、过小过大都标出、按 512 取整、错误码剥壳 |
| `export` | +4 | 递归收集顺序稳定、缺目录为空、条数与字节上限、slug 化 |
| `servers` | 9 | 增删改查、重复与非法地址、选择性更新、改回自己地址不算冲突、重排、lastSeen、坏文件读成空、名字截断 |
| `net` | 5 | 白名单、非法 URL、公网 IP 判定、`.part` 路径、磁盘满判定 |
| `recycle` | 3 | 名字拆分、怪名字容忍、拒绝穿越 |
| `world_backup` | 3 | sanitize 安全字符、Zip Slip、备份 id 前缀 |
| `modpack` | 5 | index 解析、路径守卫、坏包拒绝、穿越、纯 overrides 包不算空包 |
| `migrate` | 7 | 类别白名单、settings only、计划 id、复制实例默认勾选（带配置与资源包、不带存档、允许空复制）、复制保源、取消回滚、重名拒绝 |
| `search` | 6 | 百分号编码、facets 分组、limit/offset 夹取、非法类型与排序、响应解析、坏响应 |
| `modscan`（Maven 区间） | +1 | `[1.20,1.21)`、开闭区间、单边、精确 `[1.20.1]`、多段区间 |
| `java_audit` | 3 | 主版本表、Java 8 上限、短版本解析 |
| `cache` | 2 | 哈希优先 SHA512、拒绝非 hex |
| `update_plan` | 2 | 批次 id、历史往返 |
| `task` | 7 | 错误解码（带/不带 detail）、首次采样只记基线、两次采样算速度与 ETA、间隔太短不重算、进度回退重新采样、总量未知不报 ETA |
| `source_probe` | 8 | 开端口可达、关端口不可达、域名解析不了、挑最快、接近时优先官方、跳过不可达、全不通为 None、每个源都有标签与主机 |
| `local_import` | 7 | 内容判型、缺文件被拒、复制到对目录、重名加序号且不覆盖、批处理容错、显式类型优先、非法类型被拒 |
| `ai` | 7 | 坏 Key 被拒、裸 JSON、围栏 JSON、夹带废话、非 JSON 与空结论、消息里带本地规则结论、不同输入哈希不同 |
| `session` | 6 | 时长、统计累计、玩家自己结束不算崩溃、真非 0 退出算崩溃、0 退出不算崩溃、退出码未知不算崩溃 |
| `game_stop` | 2 | 没在跑时拒绝、等待时长与轮询间隔合理 |
| `game_dir` | 8 | 空/相对路径、指向文件、盘符根、正常目录、认出 Minecraft 目录、缺目录可新建、认出当前目录、普通目录不算 Minecraft |
| `quick_join` | 6 | 版本号解析、按版本选参数形式、两种模式的参数、主机与端口校验、地址解析（含 IPv6）、垃圾地址被拒 |
| `pack_apply` | 4 | 越界路径被拒、备份与还原一个文件、缺失文件备份为 0、没有记录时历史为空 |
| `log_search` | 6 | 关键词命中、级别过滤、大小写、条数上限与跳过数、非法级别与缺日志、级别标签识别 |
| `mod_icon` | 8 | Fabric 图标、图标对象取大图、Forge logoFile、根目录兜底、无图标、非法名与缺文件、声明缺失回退、超大图标被拒 |
| `project_detail` | 7 | 详情字段、图集过滤与封面优先、正文按字符截断、缺字段为空、非对象与空 id、缓存来源、图集上限 |
| `log_maintenance` | 9 | 占用分类统计、永远保留 latest.log、按份数保留、按天数保留、忽略非日志文件、删前要确认、只删计划内的、keep_days=0 不按天数保、空目录不炸 |
| `game_options` | +4 | 备份列表最新在前、恢复并留底、恢复拒运行中与非法名、删除要确认 |
| `world_stats` | 7 | 新格式统计、老格式统计、进度排除配方、多玩家合并排序、空存档、坏文件、未知存档 |
| `world_icon` | 4 | 读成 data URL、没图标返回 None、非 PNG 不给尺寸、超大文件被拒 |
| `batch_toggle` | 8 | 预览计数与只读、全量改名、文件仍在盘上、可反向启用、子集限定、运行中拒绝、非法种类 |
| `pack_update` | 5 | 差异分类（新增/删除/改动/未变）、同 sha1 不算改动、路径归类、长文本截断、没来源信息的实例被拒 |
| `snapshot` | 11 | 实例不存在、运行中拒绝、创建与列表、跳过 logs/截图、还原回环、旧实例留底、实例被删后能重建、删除需确认、坏 id、Zip Slip、坏 zip 被拦 |
| `jvm` | 12 | 正常内存、0/过小内存、超过物理内存、超过建议上限只提醒、系统内存未知时跳过比较、-Xmx/-Xms 写进参数被拒、换行与引号、条数与长度上限、javaagent 找不到只提醒、空白清理、validate 报第一个错、上限随机器 |
| `game_args` | 10 | 正常参数、认证类参数（含 `=` 写法）、换行与超长、条数上限、`--server` 只提醒、清洗空白、`validate` 取第一个错误、存读、非法值不落盘、坏文件当没配过 |
| `batch_toggle` | 11 | 拆名（jar/zip 与 .disabled）、扩展名过滤、预览计数与只读、全量改名、子集限定、运行中拒绝、目录不存在不算错、已是目标态不动、非法种类、复数种类归一 |
| `error` | 3 | 编码带/不带 detail、detail 含分隔符 |
| jdk | 4 | 主版本范围、正常解压落地、拒绝 `../` 穿越、拒绝符号链接条目 |
| 其余 | 各 1–3 | 账户、组件、实例、Java、加载器、mods、计划、资源、时间、统计、世界、导出 |

6 个 `#[ignore]`：真实联网的 Mojang 安装 / 启动测试（`core::tests::*`），
需要外网且耗时，不进 CI，用 `ASTRAL_LIVE_*` 环境变量手动触发。

---

## 4. 关键能力的证据链

### 4.1 下载可以真正取消（A1）

- `download cancel stops early`：4 MiB 慢速（32 KiB / 20 ms）下载，250 ms 后取消。
  断言：返回 `CANCELLED`、**没有正式文件**、`.part` 存在且小于总量、**总耗时 < 3 秒**。
- `download resumes from .part`：复用上一步的 `.part`（同一个 URL，带 ETag），
  断言 `resumed == true` 且最终 SHA1 正确。
- `download same target serialized`：两个线程同时下同一目标，两个都成功，
  文件锁保证不互相写坏。

### 4.2 缓存真的省流量（A2）

- `cache serves second copy offline`：第二次下载同一文件时，测试服务器的
  `served` 计数**不增加**，且 `reused == true`。
- `cache stats and budget eviction` / `cache clear empties only cache`：
  淘汰与清空只作用于 `cache/objects`，不动已下载的正式文件。

### 4.3 并发有上限（A3）

- `download concurrency capped`：12 个线程同时下载，峰值 `active <= 6`，
  结束后 `active == 0`，累计 `downloads >= 12`。
- `download per-host capped`：8 个线程同域名，峰值 `<= 3`。
- `download cancel while queued`：排队中被取消的请求不会真的发出。

### 4.4 安全边界（A4）

- `download blocks bad redirect`：302 到 `evil.example.com` 必须失败。
- `download blocks private ip`：`192.168.x`、`10.0.0.5`、`127.0.0.1` 全部拒绝。
- `modpack rejects zip traversal`：真实 zip 里写 `overrides/../../escaped.txt`，
  解压必须失败且文件不落到外面。
- `modpack rejects symlink entries`：zip 里的符号链接条目被拒绝
  （`zip` 9.0.5 的 `unix_permissions` 只保留低 16 位，所以测试用 `add_symlink`）。
- `path escape` / `path within`：`ensure_within` 的前缀检查与符号链接拒绝。

### 4.5 任务与会话不会说谎（A5）

- `task token released`：任务结束后令牌被回收（不会无限增长）。
- `task cancelling not reverted by progress`：取消中收到迟到进度，状态仍是 `cancelling`。
- `session detects pid reuse`：真实 pid + 伪造创建时间 → 判定为已退出。
- `session keeps live process`：真实 pid + 真实创建时间 → 不误判。
- `session records pid start time`：`begin()` 确实记录了进程创建时间。

### 4.6 依赖解析正确（B1）

- `deps resolve nested order`：`core → lib → root` 的拓扑序。
- `deps via chain`：`via == ["root", "lib"]`，`depth == 2`。
- `deps detects cycle` / `deps reports missing` / `deps depth limit` / `deps skips installed`。

### 4.7 批量更新可回滚（B2）

- `update rollback restores old file`：旧内容恢复，新文件被删。
- `update rollback leaves other mods`：无关的 beta 文件全程没被碰。
- `update plan blocks beta promotion`：稳定版不会被自动升成 beta。

### 4.8 文件检查四态分开（B3）

- `verify shallow trusts size`：浅检只比大小，内容被改但大小相同时判为 ok。
- `verify deep detects corrupt content`：同样文件，深检发现哈希不符。
- `verify plan never overwrites user files`：`overwritesUserFile` 恒 false。

### 4.9 整合包边界（B4）

- `modpack env filters client files`：4 个文件的 env 组合，默认装 2 个、
  全量装 3 个，四个计数全部对上。
- `modpack local import parses`：真实 zip + `modrinth.index.json` + overrides。
- `export roundtrip verify`：缺哈希被报为 issue，exports 外的文件被拒。
- `export plan counts nested config`：递归 config 3 个、资源包 1 个，预估字节与总数都对。
- `export plan is read only`：预览不产生任何文件。
- `export writes nested overrides`：导出后 overrides 条目数与 index 回读一致，重名时预览标为阻塞。

### 4.10 数据安全（C1–C4）

- `trash restore renames on conflict`：原 id 被占用时换新 id，占位实例保留，
  存档回来，元数据 id 已改写。
- `world restore needs overwrite then restores`：不带 overwrite 必失败；
  带 overwrite 后 `level.dat` 恢复，旧存档被改名留底而不是删除。
- `world restore rejects corrupted backup`：翻转 zip 最后一字节后 preview 有 blockers。
- `instance copy respects categories`：只复制勾选的类别，源文件仍在。
- `java remove managed deletes only target`：删 21 后 17 还在。

### 4.11 离线可用（D1）

- `meta cache serves fresh without network`：第二次请求 `served` 不增。
- `meta cache falls back when offline`：关掉服务器后仍能返回缓存，且标 `stale`。

### 4.12 诊断有用且诚实（D2）

- `diagnosis report is structured`：`worst == "error"`、`localOnly == true`、
  `summary` 含"可疑"、`nextSteps` 1–4 条、每条 `cause` 非空、`confidence` 合法。
- `diagnosis stays quiet on clean log`：干净日志 `findings` 空、`summary` 为 null。

---

## 5. 本机环境问题（不是代码问题）

| 现象 | 原因 | 处理 |
|---|---|---|
| `cargo test` 在仓库目录里跑报"拒绝访问" | 工作区路径 ACL 限制，测试进程创建临时目录失败 | `scripts/run-tests.ps1` 先复制到 `%TEMP%` 再跑；CI 无此问题 |
| `cargo check` 报 `cl: 命令行 error D8050` | MSVC 14.51 对 ring 的 C 编译命令行过长 | 设 `CARGO_PROFILE_DEV_DEBUG=0` |
| 没有 `pwsh` | 本机只有 Windows PowerShell 5.1 | 脚本用 `powershell -ExecutionPolicy Bypass` 或内联执行 |
| `cargo fmt` / `cargo clippy` 不存在 | 没装组件 | `rustup component add rustfmt clippy` |

---

## 6. 没测到的（诚实清单）

1. **真实 Mojang 安装链路的自动化**：6 个 `#[ignore]` 测试要外网，没进 CI。
   手动验证过（Forge 1.20.1、NeoForge 1.21.1、Fabric、Quilt 都成功安装并启动过）。
2. **MSA 真实登录**：阻塞在 Azure 应用 client id，只有骨架与假数据测试。
3. **超大整合包（> 4 GiB）**：只验证了上限判断逻辑，没有真的造 4 GiB 包。
4. **多任务长时间并发稳定性**：自检里是 12 线程几十秒，没有做几小时的压力测试。
5. **UI 侧联调**：本报告只覆盖 Rust 侧，前端接入后需要再跑一轮端到端。
