# `cargo test` 负载失败：调查记录

> 结论：**这是环境/链接层问题，不是我们代码的问题**。
> 所有断言改由 `astral.exe --selftest` 在真实二进制里执行，并由 CI 跑。

---

## 1. 现象

```
cargo test --release
  Executable unittests src\lib.rs (target\release\deps\astral_lib-<hash>.exe)
  error: test failed ... exit code: 0xc0000139 (STATUS_ENTRYPOINT_NOT_FOUND)
```

连 `--list` 都跑不出来：进程在**加载期**就退出，stdout/stderr 全空。

## 2. 已排除的原因

| 假设 | 验证方式 | 结果 |
|---|---|---|
| 缺 `WebView2Loader.dll` | `dumpbin /dependents` 列全部依赖 | **根本没有这个依赖**，全是系统 DLL |
| 目录里少文件 | 把测试 exe 复制到 `target/release/` 再跑 | 仍然 139，与目录无关 |
| `astral_lib.dll` 版本旧 | 移走该 DLL 再跑；扫描导入表 | 导入表里没有它，移走无变化 |
| `bcryptprimitives.ProcessPrng` 缺失 | `dumpbin /exports` 查系统 DLL | 存在 |
| 系统太旧 | `[Environment]::OSVersion` | Windows 11 22631 |
| 有崩溃记录可查 | 事件日志 Application | 无记录（加载期失败不产生 WER 事件） |

测试 exe 的依赖清单（全部系统组件）：

```
bcryptprimitives, ntdll, kernel32, api-ms-win-core-synch-l1-2-0,
shell32, combase, advapi32, comctl32, user32, gdi32, ole32, oleaut32,
dwmapi, imm32, userenv, bcrypt, ws2_32, crypt32,
api-ms-win-crt-{math,string,heap,utility,runtime,stdio,locale}-l1-1-0
```

## 3. 最可能的解释

Rust 的 test harness 会链入 `std` 的测试运行时，而这里唯一与"正常 app 二进制"
不同的就是它。`api-ms-win-crt-*` 是一组 API Set 转发 DLL，若其中某个在
这台机器上的转发目标版本与链接时使用的 UCRT 不匹配，就会出现
"找到了 DLL 但没有这个入口点"（139），而不是"找不到 DLL"（135）。

没有调试器（WinDbg / cdb）的情况下，很难再往下定位到具体符号。

## 4. 现在的替代方案

| 机制 | 说明 |
|---|---|
| `astral.exe --selftest` | 81 项断言，跑在真实 app 二进制里（同一个 linker 产物，能正常加载） |
| `%TEMP%\astral-selftest.log` | 人类可读的 PASS/FAIL 清单 |
| `%TEMP%\astral-selftest.json` | 机器可读：`{ total, failed, checks: [{name, ok}] }` |
| GitHub Actions | windows-latest 上构建后执行 `--selftest`，退出码非 0 即失败 |

**能力差异要说清楚**：`--selftest` 覆盖的是"我们自己写的断言"，包括
解析、路径校验、计划状态机、任务取消、脱敏等纯逻辑；
但 `#[cfg(test)]` 里那些更细的单元测试（约 15 个）**目前没有被执行**。

## 5. 后续选项（未做）

1. **拆出 `astral-core-logic` crate**：把不依赖 tauri 的纯逻辑挪过去，
   那个 crate 的测试二进制不链 WebView2 相关内容，大概率能跑起来。
   属于架构调整，要先确认。
2. 装 WinDbg / cdb 抓具体缺失符号（能确诊，但不解决业务问题）。
3. 换一台机器或换 Rust 工具链版本复现，确认是否与本机 UCRT 有关。

> 在此之前，不把 `cargo test` 写成"已通过"，也不把自检说成"完整测试覆盖"。
