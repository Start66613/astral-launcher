# MSA 微软登录：现状与阻碍

> 结论先说：**没有 Azure 应用 ID，微软登录做不了，也不该假装能做。**
> 本轮只交付「如实报告状态 + 配置入口 + 校验」，不写假的登录流程。

---

## 1. 现在能做什么

| 能力 | 状态 |
|---|---|
| 离线档案（用户名 + 标准离线 UUID） | ✅ 可用，已在启动链路验证 |
| 读取当前账号 | ✅ `get_account()` 返回 `kind: "offline"` |
| 微软登录状态 | ✅ `msa_status()` 如实报告"未配置"并给出注册步骤 |
| 配置 Azure 应用 ID | ✅ 配置项 `msaClientId`，`set_config` 校验 GUID 格式 |
| 真正的 MSA 登录 | ❌ 阻塞（缺应用 ID） |

## 2. 为什么缺

微软要求每个启动器**用自己的 Azure 应用**走 OAuth 设备码流程：

1. 在 Azure Portal 注册一个应用（个人 Microsoft 账号即可，不需要企业订阅）；
2. 「身份验证」里启用 **允许公共客户端流**（`allowPublicClient: true`）；
3. 拿到「应用程序（客户端）ID」，填进启动器。

**不能借用 PCL / HMCL 等现有启动器的 client id**，原因是硬的，不是道德问题：

- 违反微软开发者条款，应用随时可能被吊销，用户会突然登不上；
- 令牌与授权记录挂在别人的应用名下，等于把账号授权交给第三方；
- 微软的风控会把异常 client id 直接拒绝（`invalid_client`）。

所以这一项**必须由用户自己提供**，与 DeepSeek API Key 的 BYOK 是同一逻辑。

## 3. 拿到应用 ID 之后怎么接

`portablemc` 已经内置完整实现，不需要自己写 OAuth：

```
Auth::new(app_id)
  .request_device_code()         // 返回 user_code + verification_uri
  .wait()                        // 阻塞直到用户在浏览器里授权
  -> Account { access_token, uuid, username, xuid }
```

要做的接线：

| 步骤 | 说明 |
|---|---|
| `msa_begin` | 返回 `userCode` / `verificationUri` / 过期时间；后台线程跑 `wait()` |
| `msa_poll` | 返回 `pending` / `ok` / `error(code)` |
| 启动时用账号 | `Installer::set_auth_msa(&Account)` 替换离线认证 |
| 持久化 | 存 `refresh_token`，access token 过期后自动刷新 |
| `logout` | 删除本地账号文件 |

## 4. 必须先解决的安全问题

**令牌不能明文落盘。** 目前计划里的 `accounts.json` 会包含 refresh token，
一旦泄露等于账号被完全接管。落地前必须：

1. 用 Windows **DPAPI**（`CryptProtectData`，按当前用户加密）保存；
2. 或者放进 Windows 凭据管理器；
3. 文件权限收紧到当前用户。

这条不做完，不应该把"登录"按钮放出去。

## 5. 界面上应该怎么显示

- 未配置：显示「微软登录不可用 · 未配置应用 ID」，点击展开注册步骤；
- 已配置未登录：显示「使用 Microsoft 登录」，走设备码流程（显示 8 位代码 + 链接）；
- 已登录：显示正版用户名与皮肤头；
- **任何情况下都不要显示"已登录"如果没有真实 token**。

## 6. 未做的部分（明确）

- 设备码流程的实际接线（等应用 ID）；
- 令牌加密存储；
- 正版皮肤/披风渲染；
- 多账号切换。

> 这些都属于"有真实凭据才能验证"的部分。在没有 client id 的情况下写出来，
> 只能是未经测试的代码，不如不写。
