# 后端测试报告（V2.5）

> 分支 `feature/astral-v2-core-next`
> 数据采集时间：2026-10-26
> 结论：**自检 185 项全过，单元测试 88 通过 / 6 忽略，clippy 与 rustfmt 干净。**

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

## 2. 自检覆盖（185 项）

按前缀分组：

| 分组 | 项数 | 覆盖内容 |
|---|---|---|
| `download` | 12 | 普通下载 + SHA1、复用已验证文件、坏哈希拒绝、取消提前停止、断点续传、503 重试、非法重定向拦截、同目标串行化、内网地址拦截、`.part.json` 清理、并发上限、单域名上限 |
| `task` | 13 | 生命周期、快照、bootId、取消接受/终态/粘滞、持久化、令牌回收、取消中不被进度覆盖、健康度、去重、重试规则、阶段映射、阶段标签 |
| `verify` | 13 | 缺 jar、缺库、库大小校验、缺资源索引、四态报告、修复计划定位、不动玩家目录、浅检信任大小、深检发现内容损坏、复查反映修复、干净版本无动作、备份列表 |
| `java` | 11 | 主版本表、satisfies 规则（Java 8 上限）、审计标记路径丢失、报告结构、删除需确认、缓存统计、清空需确认、只删目标目录 |
| `instance` | 10 | 创建、发现（托管/遗留）、健康、mods 计数、名称校验、删除进回收站、健康原因 |
| `session` | 9 | 开始、退出、统计、对账、PID 复用识别、活进程不误判、死亡不活跃、已对账 |
| `plan` | 8 | 安装计划准备/复用/取用/单次、坏名字、Forge、修订守卫 |
| `modpack` | 8 | 路径穿越、符号链接、正常 overrides、env 过滤、本地导入拒绝/解析、来源记录、磁盘预检 |
| `world` | 7 | 备份创建、运行中拒绝、恢复需确认、损坏备份拒绝、存档列表、世界 id、解析 |
| `deps` | 7 | 拓扑序、via 链、环检测、缺失依赖、深度上限、已装跳过、可选跳过 |
| `update` | 6 | 历史、回滚恢复旧文件、回滚清历史、不碰其他 mod、挡 beta 升级 |
| `trash` | 5 | 列表、冲突改名恢复、恢复后清空、只删指定项、拒绝穿越 |
| `meta` | 5 | 新鲜缓存不发请求、断网回落、统计、修剪保留新鲜、清空需确认 |
| `retry` | 4 | 拒绝运行中、拒绝缺 spec、拒绝未知类型、接受已知类型 |
| `loader` | 4 | 解析、空安全、兼容性、NeoForge 提示 |
| `mod` | 4 | 列表、启用标记、禁用、启用、删除、路径守卫 |
| `cache` | 3 | 第二次走缓存、统计与淘汰、只清缓存不动文件 |
| `diagnosis` | 3 | 结构化报告、干净日志保持安静、脱敏 |
| `installer` | 3 | Forge/NeoForge 下载地址 |
| 其余 | 30+ | 时间、错误编解码、URL 白名单、路径越界、用户名规则、账户、组件目录、调度器、磁盘用量、导出回环、阶段标签、平台规则等 |

---

## 3. 单元测试（94 个）

| 模块 | 数量 | 内容 |
|---|---|---|
| `deps` | 9 | 嵌套顺序、环、双版本冲突、深度/节点上限、已装跳过、release 优先、缺失、via 链 |
| `verify` | 5 | 相对路径、需要修复、干净不需、计划不碰玩家文件、空计划 |
| `logs` | 6 | 脱敏（邮箱/IP/token/用户目录）、常见故障分类、cause/confidence、新规则、干净日志 |
| `net` | 5 | 白名单、非法 URL、公网 IP 判定、`.part` 路径、磁盘满判定 |
| `recycle` | 3 | 名字拆分、怪名字容忍、拒绝穿越 |
| `world_backup` | 3 | sanitize 安全字符、Zip Slip、备份 id 前缀 |
| `modpack` | 4 | index 解析、路径守卫、坏包拒绝、穿越 |
| `migrate` | 3 | 类别白名单、settings only、计划 id |
| `java_audit` | 3 | 主版本表、Java 8 上限、短版本解析 |
| `cache` | 2 | 哈希优先 SHA512、拒绝非 hex |
| `update_plan` | 2 | 批次 id、历史往返 |
| `task` | 2 | 错误解码（带/不带 detail） |
| `session` | 2 | 时长、统计累计 |
| `error` | 3 | 编码带/不带 detail、detail 含分隔符 |
| 其余 | 各 1–3 | 账户、组件、实例、Java、加载器、mods、计划、资源、时间、统计、世界、导出、jdk |

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
