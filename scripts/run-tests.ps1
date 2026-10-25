# 运行 Rust 单元测试（绕过两个本机环境问题）
#
# 1) debug 配置在本机 MSVC 14.51 上编译 ring 会报 D8050（c1.dll 无法执行）；
#    用 --release，或设置 CARGO_PROFILE_DEV_DEBUG=0，都能绕开。本脚本默认 release。
# 2) 测试二进制如果直接在仓库目录里运行，创建临时目录会报"拒绝访问"（工作区路径 ACL 限制），
#    所以先复制到仓库外再跑。
#
# 用法：pwsh -NoProfile -File scripts/run-tests.ps1 [-Filter <名字>]

param([string]$Filter = "")

$root = Join-Path $PSScriptRoot '..\app\src-tauri'
Push-Location $root
# cargo 会把进度写到 stderr，Stop 模式下会被当成终止错误，这里统一放宽
$ErrorActionPreference = 'Continue'
try {
    $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"

    Write-Output '== 构建测试二进制（release）=='
    $out = cargo test --release --lib --no-run 2>&1
    $buildCode = $LASTEXITCODE
    ($out | Where-Object { "$_" -match 'Executable|^error' } | Select-Object -Last 3) |
        ForEach-Object { Write-Output "$_" }
    if ($buildCode -ne 0) {
        Write-Output "构建失败 exit=$buildCode"
        exit 1
    }

    $exe = Get-ChildItem 'target\release\deps\astral_lib-*.exe' -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if (-not $exe) { Write-Output '找不到测试二进制'; exit 1 }

    $runDir = Join-Path $env:TEMP 'astral-tests'
    New-Item -ItemType Directory -Force -Path $runDir | Out-Null
    $copy = Join-Path $runDir 'astral_lib_tests.exe'
    Copy-Item $exe.FullName $copy -Force

    Write-Output "== 运行 $($exe.Name) =="
    if ($Filter -ne '') { & $copy $Filter --nocapture } else { & $copy --nocapture }
    $code = $LASTEXITCODE
    Write-Output "exit=$code"
    exit $code
}
finally {
    Pop-Location
}
