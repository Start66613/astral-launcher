# Astral Launcher 后端路线图（V2.5）

> 分支：`feature/astral-v2-core-next`（从 `feature/astral-v2-core` 的 `e0ebb7c` 切出）
> 本文记录"哪些做完了、哪些没做、为什么这个顺序"，给后来接手的人和 Codex 看。
> 契约细节见 `BACKEND-API-ADDENDUM.md`，测试证据见 `BACKEND-TEST-REPORT.md`。

---

## 0. 一句话现状

A1–A5、B1–B4、C1–C4、D1–D2 已完成并有自动化测试；D3 的测试矩阵已随各功能逐项落地；
D4 的 CI 已跑通（fmt / clippy / check / build / selftest / audit），发布检查清单与自动更新
签名方案尚未完成。共 103 个 Tauri 命令、202 项自检、116 个 Rust 单元测试（110 通过 6 忽略）。

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
  当前 116 个，110 通过、6 个 `#[ignore]`（真实联网的 Mojang 安装测试）。
- **自检（`--selftest`）**：所有需要真实文件系统、真实 HTTP、真实进程的检查都放这里。
  用 `testsrv.rs` 起一个只监听 `127.0.0.1` 的临时服务器，能确定性制造 503、慢速、
  重定向、Range 响应。当前 202 项。
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
   资源搜索页可以直接照 §10 接。
2. 发布产物定型 → 定稿发布检查清单（`RELEASE-CHECKLIST.md` 已在，产物一栏待填）→ 决定自动更新方案。
3. JDK 自动下载接到"运行库"页面（后端已通）。
4. MSA 登录（等 client id）。
