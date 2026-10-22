# Astral Launcher

一个 Windows 上的 Minecraft 启动器。界面走克制路线，下载支持国内镜像，内置 AI 崩溃分析（BYOK）。

> 当前进度：**W3** —— 能装、能启、能换源、能下 Mod。整合包解包与实例隔离还没做。

![启动页](shots/app-launch.png)

## 已实现

| 能力 | 状态 | 说明 |
|---|---|---|
| 版本清单 | ✅ | Mojang 官方 / BMCLAPI 镜像，920 个版本 |
| 安装 | ✅ | 元数据 → 依赖库 → 资源索引 → 并行下载 + SHA1 校验；缺 Java 时自动装 Mojang 官方 JRE |
| 启动 | ✅ | 离线登录，起进程返回 PID，约 0.9 秒开窗 |
| 国内镜像 | ✅ | BMCLAPI：依赖库 + 资源文件改写；版本元数据/客户端 jar 仍走官方 |
| Mod / 资源包 / 光影 | ✅ | Modrinth 搜索 + 下载到对应目录 |
| 主题 | ✅ | 亮/暗 · 4 强化色 · 3 档圆角 · 动效开关，实时生效 |
| AI 崩溃分析 | ◐ | 交互与结构完成，未接 DeepSeek |
| 整合包安装 | ❌ | 只下 `.mrpack`，未解包建实例 |
| 微软登录 | ❌ | crate 有设备码流程，未接 UI |

## 结构

```
app/                       Tauri 2 + React 19 + TS + Rust
  src/                     界面：tokens / 组件 / 五个页面
  src-tauri/src/core.rs    安装、启动、配置（基于 portablemc）
  src-tauri/src/lib.rs     Tauri 命令 + 进度事件
  src-tauri/vendor/portablemc   打了镜像补丁的上游 crate（Apache-2.0）
docs/                      方案书、决策记录、每周实现记录
preview/                   设计原型（v4 是定稿那版）
shots/                     运行截图
```

## 开发

```powershell
cd app
pnpm install
pnpm dev            # 只看界面，不需要 Rust
pnpm tauri dev      # 原生窗口
pnpm tauri build    # 出 exe（约 6 MB）
```

前提：Node 18+、Rust stable（MSVC toolchain）、WebView2（Win10 1803+ 自带）。

> 如果 `pnpm` 在 PowerShell 里被策略挡住，用 `pnpm.cmd` 或 `node <pnpm 路径> ...`。

## 配置

`%APPDATA%\Astral\config.json`

```jsonc
{
  "gameDir": "C:\\Users\\你\\AppData\\Roaming\\.minecraft",
  "source": "official",   // official | bmclapi
  "username": "Player"
}
```

界面上在「设置 → 下载与目录」里改。

## 文档

| 文档 | 内容 |
|---|---|
| [00-方案对比与路线图](docs/00-方案对比与路线图.md) | 四个技术方案的论证（历史） |
| [01-决策记录与修订](docs/01-决策记录与修订.md) | 产品决策、下载模块设计、界面 12 条规则、布局结构 |
| [02-W1-实现记录](docs/02-W1-实现记录.md) | 骨架 + 设计系统 |
| [03-W2-核心接入](docs/03-W2-核心接入.md) | 许可证矩阵、接入 portablemc、验证数据 |
| [04-W3-启动与资源](docs/04-W3-启动与资源.md) | 启动验证、镜像踩坑、Modrinth |

## 合规

- 不包含、不分发任何 Minecraft 游戏文件；所有内容由用户自己的微软账号从官方渠道获取。
- 国内镜像只做**透明加速**（改地址），不替换内容、不做二次分发。
- 与 Mojang、Microsoft、网易均无关联。

## 许可证

本项目代码 [MIT](LICENSE)。第三方组件见 [THIRD_PARTY.md](THIRD_PARTY.md)（`portablemc` 为 Apache-2.0）。
