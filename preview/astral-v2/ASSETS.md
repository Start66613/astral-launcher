# 原型素材与来源记录

日期：2026-10-24。产品资源目录没有加入任何其他启动器的代码、图标、品牌、壁纸或截图。

## 森林插画：复用 Astral 先前原创素材

原始制作记录在 `../forest-tale/assets/prompts.json`：此前通过 image_gen 生成的背景插画，不是真实玩家存档截图，也不是 Mojang 官方美术。三张均为已有 WebP quality 88，本轮没有重新编码或覆盖。正式与评审统一从 app/public/forest/ 引用；补齐 A/C 两张，原素材保留，hash一致。

- 森林晨光：`app/public/forest/daylight.webp`，435,094 bytes。
  SHA256 `6C503476E6E30B92925C8867B940219153E522EC51B17CC2F2ADFD2C000AC76B`
- 深林秘境：`app/public/forest/nightfall.webp`，217,712 bytes。保留此前 B 案的月色湖泊与松绿氛围，统一对玩家使用新主题名。
  SHA256 `60A5044FA2D6307968A35AD06B79A6B5B58D5C5DD531F1BF5DF2F22924A56B7A`
- 林野手札：`app/public/forest/fieldnotes.webp`，536,042 bytes。根据原有纸感水粉森林素材命名。
  SHA256 `924017AAA12C829ACDEA70BA98C6A98CC32542E0DD12D8227B70BCE683542F59`

合计 1,188,848 bytes。背景交叉淡化只改变 opacity，不使用帧序列或视频。缩略图懒加载并复用缓存；图片失败后隐藏图片，保留 Token 背景和可操作控件。正式阶段可按真实 WebView2 内存测试再决定缩略图专用尺寸，而非重复下载原图。

## 图标

`app/src/v2/icons.tsx` 是本轮原创 SVG 路径，随 Astral 仓库 MIT 许可；不是从 PCL、HMCL、Prism、Refract 或 Minecraft 资源包提取。无 emoji，无图标字体依赖。

统一 24×24 viewBox、1.6px 线宽，普通 glyph 37×39、大 glyph 57×59；使用 currentColor 随主题变色。缺失图标回退到方块图形。颜色之外仍保留名称与选中控件。

- 正式版：立方块；快照：试验瓶；历史版本：沙漏；特殊版本：棱星；已安装：方块加勾。
- Vanilla、Fabric（织纹）、Forge（砧形）、NeoForge（原创抽象狐形）、Quilt（拼布格）、Iris（眼形）、Oculus（镜片）、OptiFine（原创字母线条）、Fabric API（拼接件）均为原创类别提示，并非官方标志。不可对外宣称获得各品牌背书；名称保留帮助识别。
- Mod、整合包、资源包、光影包、数据包分别使用拼接件、包裹、砖格、光线、代码盒形。状态与警告同一体系。

## 公开数据快照

- `data/version-catalog.json`：Mojang 官方 version_manifest_v2，保存版本 ID、类型、发布时间与 latest 指针，共 920 条；没有 Minecraft 二进制或游戏图片。
- `data/loader-catalog.json`：Fabric 官方 meta 的 Minecraft 1.21.1 构建目录，保存版本号和 stable 字段；其他游戏版本不冒充可查询。推荐是 stable 候选入口，不禁止其他构建。
- `data/resource-catalog.json`：Modrinth v2 的项目事实数据、公开团队名称与部分 1.21.1 发行文件元数据（每项目最多 16 个发行项）。保留来源、抓取时间、projectUrl 与项目许可字段；不保存项目正文、第三方图标、资源文件内容或下载文件。作者展示为公开团队中的 Owner 或少量维护者，完整作者列表见项目主页。

设计评审室中的游戏配置、存档和 Task/GameSession 为明确标记的样例，仅存于原型内存；正式入口使用核心真实返回，不会将这些样例回退成用户数据。真实项目的许可字段不意味着可以任意重新分发其资源文件，本轮没有重新分发。

## 外部研究图像

六个项目的截图来源链接在 `docs/ui/astral-v2-research.md`。检查时只在浏览器观看；浏览器产生的研究截图位于 Git 忽略的 `screenshots/references/`，不参与原型构建，不是产品资产，也不进入提交。引用网页和查看图像不等于获得重用其图片的许可。

## 实时图标
正式资源页按Modrinth公开元数据显示远程项目图标和来源，加载失败回退原创类别图标；未下载入库或复用为Astral品牌素材。项目许可仍以资源主页为准。

