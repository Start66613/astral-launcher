# 第三方代码与许可证

本项目自身代码以 [MIT](LICENSE) 发布。下面是引入的第三方组件。

## portablemc

- 位置：`app/src-tauri/vendor/portablemc`
- 版本：5.0.6
- 许可证：**Apache-2.0**
- 上游：<https://github.com/theorzr/portablemc>
- 用途：Minecraft 版本解析、依赖下载、资源校验、Java 探测、启动参数拼装

为了让下载能走国内镜像，我们对它做了三处修改（均为增量，未改变原有行为）：

| 文件 | 修改 |
|---|---|
| `src/mirror.rs` | 新增：地址改写表（BMCLAPI）与全局开关 |
| `src/download.rs` `Entry::new` | 创建下载项时套用 `mirror::map()` |
| `src/download.rs` `Batch::download` | 镜像模式下并发从 40 降到 6（镜像站会对高并发重置连接） |

Apache-2.0 要求保留版权与许可声明；本文件与上游仓库链接即为此用途。

## 其他依赖

通过 Cargo / npm 引入的依赖见 `app/src-tauri/Cargo.toml`、`app/package.json`，各自遵循其原始许可证。

## 素材说明

- `preview/ref/` 中的图片仅作设计参考，不参与构建，不在发布产物中。
- 项目不包含任何 Minecraft 游戏文件；所有游戏内容均由用户账号从 Mojang 官方渠道下载。
