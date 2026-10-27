param([Parameter(Mandatory=$true)][string]$PackagePath)
$ErrorActionPreference='Stop'
$projectRoot=Split-Path $PSScriptRoot -Parent
$package=(Resolve-Path -LiteralPath $PackagePath).Path
$evidence=Join-Path $projectRoot ('artifacts/p2-native-'+[Guid]::NewGuid().ToString('N'))
$names=@('AYASE_TEST_DATA_DIR','AYASE_SMOKE_PACKAGE','AYASE_SMOKE_P2','WEBVIEW2_USER_DATA_FOLDER')
$previous=@{}
foreach($name in $names){$previous[$name]=[Environment]::GetEnvironmentVariable($name)}
Push-Location $projectRoot
try{
  & npm.cmd run build;if($LASTEXITCODE -ne 0){throw '前端构建失败'}
  & "$PSScriptRoot/cargo.ps1" build -p ayase-companion --features 'custom-protocol,smoke-test' --locked
  if($LASTEXITCODE -ne 0){throw '原生测试构建失败'}
  New-Item -ItemType Directory -Path $evidence | Out-Null
  $env:AYASE_TEST_DATA_DIR=$evidence
  $env:AYASE_SMOKE_PACKAGE=$package
  $env:AYASE_SMOKE_P2='1'
  $env:WEBVIEW2_USER_DATA_FOLDER=Join-Path $evidence 'webview'
  $process=Start-Process -FilePath './target/debug/ayase-companion.exe' -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $evidence 'stdout.log') -RedirectStandardError (Join-Path $evidence 'stderr.log')
  for($step=0;$step -lt 30;$step++){if($process.WaitForExit(5000)){break}}
  if(!$process.HasExited){throw "P2 原生测试超时，隔离实例进程编号 $($process.Id)，证据保留在 $evidence"}
  $result=Get-Content -LiteralPath (Join-Path $evidence 'p2-native-smoke.json') -Raw | ConvertFrom-Json
  if($process.ExitCode -ne 0 -or $result.result -ne 'passed'){throw "P2 原生回执检查失败：$($result.error)；证据：$evidence"}
  Write-Output "P2 原生回执检查通过：$evidence"
}finally{
  foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$previous[$name])}
  Pop-Location
}
