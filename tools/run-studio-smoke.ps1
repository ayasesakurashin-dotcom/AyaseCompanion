param([switch]$SkipBuild)
$ErrorActionPreference='Stop'
$projectRoot=Split-Path $PSScriptRoot -Parent
$caseRoot=Join-Path $projectRoot ('artifacts/p4-native-'+[guid]::NewGuid().ToString('N'))
$names=@('AYASE_TEST_DATA_DIR','AYASE_SMOKE_P4','AYASE_SMOKE_PACKAGE','WEBVIEW2_USER_DATA_FOLDER','AYASE_SMOKE_P2','AYASE_SMOKE_MEDIA','AYASE_SMOKE_BASE_RESTORE')
$prior=@{}
foreach($name in $names){$prior[$name]=[Environment]::GetEnvironmentVariable($name)}
Push-Location $projectRoot
try{
 if(!$SkipBuild){
  & npm.cmd run build;if($LASTEXITCODE -ne 0){throw '前端构建失败'}
  & "$PSScriptRoot/cargo.ps1" build -p ayase-companion --features 'custom-protocol,smoke-test' --locked
  if($LASTEXITCODE -ne 0){throw '原生测试构建失败'}
 }
 New-Item -ItemType Directory -Path $caseRoot | Out-Null
 $env:AYASE_TEST_DATA_DIR=$caseRoot
 $env:AYASE_SMOKE_P4='1'
 $env:AYASE_SMOKE_P2=$null
 $env:AYASE_SMOKE_MEDIA=$null
 $env:AYASE_SMOKE_BASE_RESTORE=$null
 $env:AYASE_SMOKE_PACKAGE='isolated-studio-fixture'
 $env:WEBVIEW2_USER_DATA_FOLDER=Join-Path $caseRoot 'webview'
 $caseProcess=Start-Process -FilePath './target/debug/ayase-companion.exe' -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $caseRoot 'stdout.log') -RedirectStandardError (Join-Path $caseRoot 'stderr.log')
 @{path=$caseRoot;pid=$caseProcess.Id}|ConvertTo-Json|Set-Content -Encoding utf8 artifacts/p4-native-current.local.json
 for($step=0;$step -lt 24;$step++){if($caseProcess.WaitForExit(5000)){break}}
 if(!$caseProcess.HasExited){throw "原生检查超时，隔离进程编号 $($caseProcess.Id)，证据保留在 $caseRoot"}
 $result=Get-Content -LiteralPath (Join-Path $caseRoot 'p4-native-smoke.json') -Raw|ConvertFrom-Json
 if($caseProcess.ExitCode -ne 0 -or $result.result -ne 'passed'){throw "原生检查失败：$($result.error)；证据：$caseRoot"}
 Write-Output "工作台原生回执通过：$($result.cases.Count) 项；证据：$caseRoot"
}finally{foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$prior[$name])};Pop-Location}
