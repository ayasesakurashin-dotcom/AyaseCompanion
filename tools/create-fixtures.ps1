param([string]$EmaSource='')
$ErrorActionPreference='Stop'
$projectRoot=Split-Path $PSScriptRoot -Parent
Push-Location $projectRoot
try{
  & "$PSScriptRoot/cargo.ps1" build -p ayase-core --bin ayase-pack --locked
  if($LASTEXITCODE -ne 0){throw '包工具编译失败'}
  & './target/debug/ayase-pack.exe' fixture 'artifacts/packages/static-unknown-0.1.0.ayase'
  if($LASTEXITCODE -ne 0){throw '静态样例生成失败'}
  & './target/debug/ayase-pack.exe' pack 'plugins/examples/nightly-rest' 'artifacts/packages/nightly-rest-0.2.0.ayplug'
  if($LASTEXITCODE -ne 0){throw '插件打包失败'}
  if($EmaSource){
    & './target/debug/ayase-pack.exe' convert-ema $EmaSource 'local-assets/ema-0.1.0.ayase'
    if($LASTEXITCODE -ne 0){throw 'Ema 转换失败'}
  }
}finally{Pop-Location}
