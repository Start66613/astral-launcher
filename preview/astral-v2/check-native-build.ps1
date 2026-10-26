param([switch]$Build)
$ErrorActionPreference='Stop'
$project=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$tempRoot='C:\Users\ASUS\.codex\tmp\astral-integration-next'
New-Item -ItemType Directory -Force -Path $tempRoot | Out-Null
$vsRoot='C:\Program Files\Microsoft Visual Studio\2022\Community'
Import-Module ($vsRoot+'\Common7\Tools\Microsoft.VisualStudio.DevShell.dll')
Enter-VsDevShell -VsInstallPath $vsRoot -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64' | Out-Null
$env:PATH='C:\Users\ASUS\.cargo\bin;'+$env:PATH
$env:TEMP=$tempRoot; $env:TMP=$tempRoot; $env:SYSTEMTEMP=$tempRoot
$env:CARGO_BUILD_JOBS='2'; $env:CARGO_PROFILE_DEV_DEBUG='0'
$env:TAURI_CONFIG='{"build":{"devUrl":"http://127.0.0.1:5218"}}'
Push-Location $project
try {
  cargo check --manifest-path app/src-tauri/Cargo.toml
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
  # MSVC's linker needs an ASCII temp path; tests run in the writable test tree.
  cargo test --manifest-path app/src-tauri/Cargo.toml --lib --no-run
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
  $env:TEMP=Join-Path $PSScriptRoot 'native-test/temp'
  $env:TMP=$env:TEMP; $env:SYSTEMTEMP=$env:TEMP
  New-Item -ItemType Directory -Force -Path $env:TEMP | Out-Null
  $testExe=Get-ChildItem app/src-tauri/target/debug/deps/astral_lib-*.exe | Sort-Object LastWriteTime -Descending | Select-Object -First 1
  & $testExe.FullName --test-threads=1
  $testResult=$LASTEXITCODE
  $env:TEMP=$tempRoot; $env:TMP=$tempRoot; $env:SYSTEMTEMP=$tempRoot
  if ($Build) { cargo build --manifest-path app/src-tauri/Cargo.toml; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE } }
  exit $testResult
} finally { Pop-Location }
