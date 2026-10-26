param([int]$Port = 9234)
$ErrorActionPreference = 'Stop'
$project = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if ((git -C $project branch --show-current) -ne 'feature/astral-v2-integration-next') { throw 'Wrong branch' }
$isolated = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot 'native-test/integration'))
if (!$isolated.StartsWith($project + [IO.Path]::DirectorySeparatorChar)) { throw 'Test directory escaped project' }
$exe = Join-Path $project 'app/src-tauri/target/debug/astral.exe'
if (!(Test-Path -LiteralPath $exe)) { throw 'Build this worktree first' }
New-Item -ItemType Directory -Force -Path "$isolated/AppData/Astral", "$isolated/LocalAppData", "$isolated/temp", "$isolated/game", "$isolated/state" | Out-Null
$configFile = Join-Path $isolated 'AppData/Astral/config.json'
if (!(Test-Path -LiteralPath $configFile)) {
  @{gameDir="$isolated/game";username='AstralTest';source='official';activeInstanceId=$null} | ConvertTo-Json | Set-Content -Encoding utf8NoBOM -LiteralPath $configFile
}
$cfg = Get-Content -Raw -LiteralPath $configFile | ConvertFrom-Json
if ([IO.Path]::GetFullPath($cfg.gameDir) -ne [IO.Path]::GetFullPath("$isolated/game")) { throw 'Game directory is not isolated' }
$env:APPDATA="$isolated/AppData"
$env:LOCALAPPDATA="$isolated/LocalAppData"
$env:TEMP="$isolated/temp"
$env:TMP="$isolated/temp"
$env:SYSTEMTEMP="$isolated/temp"
$env:ASTRAL_DATA_DIR="$isolated/state"
$env:WEBVIEW2_USER_DATA_FOLDER="$isolated/WebView2"
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=$Port"
$env:HTTP_PROXY='http://127.0.0.1:7892'
$env:HTTPS_PROXY='http://127.0.0.1:7892'
$process = Start-Process -FilePath $exe -PassThru -WindowStyle Hidden
$process.Id | Set-Content -LiteralPath "$isolated/pid.txt"
Write-Output "Isolated native PID $($process.Id); game directory: $($cfg.gameDir)"
