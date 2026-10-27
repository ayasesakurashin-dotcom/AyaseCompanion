# Preserve native flags without PowerShell common-parameter binding.
$CargoArgs=@($args)
$ErrorActionPreference='Stop'
$projectRoot=Split-Path $PSScriptRoot -Parent
$localWrapper=Join-Path (Split-Path $projectRoot -Parent) '.accelerate/companion-cargo.cmd'
if(Get-Command cargo.exe -ErrorAction SilentlyContinue){ & cargo.exe @CargoArgs }
elseif(Test-Path -LiteralPath $localWrapper){ & $localWrapper @CargoArgs }
else{ throw '未找到 Rust。请安装 stable-x86_64-pc-windows-msvc 与 Visual Studio C++ Build Tools，再重新运行。' }
exit $LASTEXITCODE
