param([Parameter(Mandatory=$true)][string]$PackagePath,[switch]$SelfMedia,[switch]$BaseRestore)
$ErrorActionPreference='Stop'
$projectRoot=Split-Path $PSScriptRoot -Parent
$resolvedPackage=(Resolve-Path -LiteralPath $PackagePath).Path
$evidence=Join-Path $projectRoot ('artifacts/native-smoke-'+[Guid]::NewGuid().ToString('N'))
$oldData=$env:AYASE_TEST_DATA_DIR
$oldPackage=$env:AYASE_SMOKE_PACKAGE
$oldWebView=$env:WEBVIEW2_USER_DATA_FOLDER
$oldMedia=$env:AYASE_SMOKE_MEDIA
$oldBase=$env:AYASE_SMOKE_BASE_RESTORE
if($SelfMedia -and $BaseRestore){throw 'SelfMedia 与 BaseRestore 请分别运行'}
Push-Location $projectRoot
try{
  & npm.cmd run build;if($LASTEXITCODE -ne 0){throw 'Frontend build failed'}
  & "$PSScriptRoot/cargo.ps1" build -p ayase-companion --features 'custom-protocol,smoke-test' --locked
  if($LASTEXITCODE -ne 0){throw 'Native smoke build failed'}
  New-Item -ItemType Directory -Path $evidence | Out-Null
  $env:AYASE_TEST_DATA_DIR=$evidence
  $env:AYASE_SMOKE_PACKAGE=$resolvedPackage
  $env:WEBVIEW2_USER_DATA_FOLDER=Join-Path $evidence 'webview'
  if($SelfMedia){$env:AYASE_SMOKE_MEDIA='1'}else{$env:AYASE_SMOKE_MEDIA=$null}
  if($BaseRestore){$env:AYASE_SMOKE_BASE_RESTORE='1'}else{$env:AYASE_SMOKE_BASE_RESTORE=$null}
  $process=Start-Process -FilePath './target/debug/ayase-companion.exe' -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $evidence 'stdout.log') -RedirectStandardError (Join-Path $evidence 'stderr.log')
  for($step=0;$step -lt 8;$step++){
    if($process.WaitForExit(15000)){break}
    Write-Output 'Waiting for native presentation callbacks...'
  }
  if(!$process.HasExited){throw "Smoke timed out; inspect test process $($process.Id) and $evidence"}
  $result=Get-Content -Raw -LiteralPath (Join-Path $evidence 'native-smoke.json') | ConvertFrom-Json
  if($process.ExitCode -ne 0 -or $result.result -ne 'passed'){throw "Smoke failed: $evidence"}
  Write-Output "Native callbacks passed: $evidence"
}finally{
  $env:AYASE_TEST_DATA_DIR=$oldData
  $env:AYASE_SMOKE_PACKAGE=$oldPackage
  $env:WEBVIEW2_USER_DATA_FOLDER=$oldWebView
  $env:AYASE_SMOKE_MEDIA=$oldMedia
  $env:AYASE_SMOKE_BASE_RESTORE=$oldBase
  Pop-Location
}
