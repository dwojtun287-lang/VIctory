# ztrx launcher: runs the game for normal play (no scripted presses, no capture).
param(
    [Parameter(Mandatory = $true)][string]$Game,
    [string]$Cache = "$env:LOCALAPPDATA\ztrx\cache",
    [string]$Data = "$env:LOCALAPPDATA\ztrx\data",
    [string]$Exe,
    [switch]$SkipPrecompile
)
$ErrorActionPreference = "Stop"
if (-not $Exe) { $Exe = Join-Path $PSScriptRoot '..\target\release\victory.exe' }
$Exe = (Resolve-Path -LiteralPath $Exe).Path
$Game = (Resolve-Path -LiteralPath $Game).Path
foreach ($v in "VICTORY_KEYS", "VICTORY_CAPTURE", "VICTORY_CAPTURE_FRAMES", "VICTORY_DUMP_TARGETS") {
    Remove-Item "Env:$v" -ErrorAction SilentlyContinue
}
New-Item -ItemType Directory -Force $Cache, $Data | Out-Null
$env:VICTORY_CACHE_DIR = $Cache
$env:VICTORY_DATA_DIR = $Data
$ErrorActionPreference = "Continue"
if (-not $SkipPrecompile) {
    & $Exe precompile ([System.IO.Path]::GetDirectoryName($Game)) 2>&1 | Tee-Object -FilePath "$env:TEMP\ztrx-precompile.log"
    if ($LASTEXITCODE -ne 0) {
        Write-Host "Precompilation failed; game was not started. Log: $env:TEMP\ztrx-precompile.log"
        exit $LASTEXITCODE
    }
}
& $Exe run $Game --execute 2>&1 | Out-File "$env:TEMP\ztrx.log" -Encoding utf8
Write-Host "Exited with $LASTEXITCODE; log: $env:TEMP\ztrx.log"
exit $LASTEXITCODE
