# Astral Launcher

一个 Windows 上的 Minecraft 启动器。界面是「森林物语 · 萤火夜航」，核心支持国内镜像与实例隔离，AI 崩溃分析用 BYOK（未接）。

> 当前进度：**W4 / 契约 v3.3** —— 实例、会话、任务、模组、Fabric 加载器都已可用；
> 森林物语界面已接入真实数据。整合包解包、MSA 登录、AI 诊断还没做。

![森林物语 · 首页](shots/ui-home.png)

更多截图：[版本管理](shots/ui-versions.png) · [下载中心](shots/ui-download.png) · [模组中心](shots/ui-mods.png) · [设置](shots/ui-settings.png)

> 以上为本机真实运行截图（Windows 11，读取本机 `.minecraft` 中的 1.8.9 与 26.3）。

## 已实现

| 能力 | 状态 | 说明 |
|---|---|---|
| 版本清单 | ✅ | Mojang 官方 / BMCLAPI 镜像，920 个版本 |
| 安装 | ✅ | 元数据 → 依赖库 → 资源索引 → 并行下载 + SHA1；缺 Java 自动装 Mojang JRE |
| 启动 | ✅ | 离线登录，约 0.9 秒开窗；进程退出推 `astral://game-exit` |
| 实例 | ✅ | 托管实例独立 `mods/config/saves/logs`；兼容实例从 `versions/` 直读并标注归属 |
| 会话 | ✅ | `GameSession` 记录 pid，重启后按 pid 对账，不再靠 PID 常亮 |
| 任务 | ✅ | `Task` + `revision` 单调递增，可补拉、终态粘滞 |
| 国内镜像 | ✅ | BMCLAPI 改写依赖库与资源；版本元数据/客户端 jar 仍走官方 |
| 模组 | ✅ | 列表 / 启停 / 删除；jar 元数据尽力解析（fabric/forge/neoforge） |
| 加载器 | ◐ | Fabric / Quilt 已通；Forge / NeoForge 未做 |
| 资源下载 | ✅ | Modrinth 搜索 + 下载到对应目录 |
| 界面 | ✅ | 森林物语：全幅场景、窄侧栏、启动坞；深浅主题 |
| AI 崩溃分析 | ◐ | 抽屉交互完成，未接 DeepSeek |
| 整合包安装 | ❌ | 只下 `.mrpack`，未解包建实例 |
| 微软登录 | ❌ | crate 有设备码流程，未接 UI |

## 结构

```
app/                       Tauri 2 + React 19 + TS + Rust
  src/                     界面：pages / components / styles（森林物语主题）
  src-tauri/src/
    core.rs                安装、启动、加载器装配（基于 portablemc）
    instance.rs            实例模型、目录、健康检查
    session.rs             游戏会话与 pid 对账
    task.rs                任务注册表（revision / 终态粘滞）
    mods.rs                模组列表、启停、删除
    loader.rs              Fabric meta 目录
    stats.rs / time.rs     存档与统计、时间工具
  src-tauri/vendor/portablemc   打了镜像与 JVM 参数补丁的上游 crate（Apache-2.0）
docs/                      方案书、决策记录、契约、实现记录
preview/                   设计原型与森林物语原型
shots/                     运行截图
```

## 开发

```powershell
cd app
pnpm install
pnpm dev            # 只看界面，不需要 Rust
pnpm tauri dev      # 原生窗口
pnpm tauri build    # 出 exe（约 11 MB）
```

前提：Node 18+、Rust stable（MSVC toolchain）、WebView2（Win10 1803+ 自带）。

> 如果 pnpm 在 PowerShell 里被执行策略挡住，用 `node <pnpm.mjs 路径> ...` 调用。

## 配置

`%APPDATA%\Astral\config.json`

```jsonc
{
  "gameDir": "C:\\Users\\你\\AppData\\Roaming\\.minecraft",
  "source": "official",        // official | bmclapi
  "username": "Player",
  "activeInstanceId": null
}
```

界面上在「设置」里改；`set_config` 支持只传要改的字段。

## 文档

| 文档 | 内容 |
|---|---|
| [api-contract.md](docs/api-contract.md) | **前后端接口契约 v3.3**（命令、事件、错误码、实例布局） |
| [api-contract-bridge.md](docs/api-contract-bridge.md) | `bridge.ts` / `types.ts` 的增量代码块 |
| [00](docs/00-方案对比与路线图.md) · [01](docs/01-决策记录与修订.md) | 方案论证与决策记录 |
| [02](docs/02-W1-实现记录.md) · [03](docs/03-W2-核心接入.md) · [04](docs/04-W3-启动与资源.md) · [05](docs/05-W4-核心实现.md) | 各阶段实现记录 |

## 合规

- 不包含、不分发任何 Minecraft 游戏文件；内容由用户自己的账号从官方渠道获取。
- 国内镜像只做**透明加速**（改地址），不替换内容、不做二次分发。
- 与 Mojang、Microsoft、网易均无关联。

## 许可证

本项目代码 [MIT](LICENSE)。第三方组件见 [THIRD_PARTY.md](THIRD_PARTY.md)（`portablemc` 为 Apache-2.0）。
