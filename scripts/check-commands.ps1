# 校验：每个 #[tauri::command] 都必须在 generate_handler! 里注册。
#
# 起因：V2 开发中有 16 个命令写完却忘了注册，编译通过、自检通过，
# 但前端 invoke 会直接报 "command not found"。这个脚本把这类错误挡在 CI 里。

$ErrorActionPreference = 'Stop'
$file = Join-Path $PSScriptRoot '..\app\src-tauri\src\lib.rs'
$lines = Get-Content $file -Encoding UTF8

$handlerStart = ($lines | Select-String -Pattern 'generate_handler!\[' | Select-Object -First 1).LineNumber
if (-not $handlerStart) { Write-Error 'generate_handler! 未找到'; exit 1 }
$handlerEnd = ($lines | Select-String -Pattern '^\s*\]\)' |
    Where-Object { $_.LineNumber -gt $handlerStart } | Select-Object -First 1).LineNumber

$registered = @()
for ($i = $handlerStart; $i -lt $handlerEnd; $i++) {
    if ($lines[$i] -match '^\s+(\w+),?\s*$') { $registered += $matches[1] }
}

$commands = @()
for ($i = 0; $i -lt $lines.Count; $i++) {
    if ($lines[$i] -match '#\[tauri::command\]') {
        for ($j = $i + 1; $j -lt [Math]::Min($i + 6, $lines.Count); $j++) {
            if ($lines[$j] -match '^\s*(pub\s+)?(async\s+)?fn\s+(\w+)') { $commands += $matches[3]; break }
        }
    }
}

$missing = $commands | Where-Object { $registered -notcontains $_ }
$extra = $registered | Where-Object { $commands -notcontains $_ }

Write-Output "命令: $($commands.Count)  已注册: $($registered.Count)"
if ($extra.Count -gt 0) {
    Write-Output "注册了但找不到定义: $($extra -join ', ')"
}
if ($missing.Count -gt 0) {
    Write-Error "以下命令没有注册到 generate_handler!: $($missing -join ', ')"
    exit 1
}
Write-Output 'OK: 全部命令已注册'
exit 0
