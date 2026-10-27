param([switch]$SkipInstall,[switch]$SkipTests)
$ErrorActionPreference='Stop'
$projectRoot=Split-Path $PSScriptRoot -Parent
Push-Location $projectRoot
try{
  if(!$SkipInstall){& npm.cmd ci --no-audit --no-fund;if($LASTEXITCODE -ne 0){throw '依赖安装失败'}}
  & npm.cmd run build;if($LASTEXITCODE -ne 0){throw '前端构建失败'}
  if(!$SkipTests){
    & npm.cmd test;if($LASTEXITCODE -ne 0){throw '前端回归失败'}
    & "$PSScriptRoot/cargo.ps1" test -p ayase-core --locked;if($LASTEXITCODE -ne 0){throw '内核回归失败'}
    & "$PSScriptRoot/cargo.ps1" test -p ayase-companion --locked;if($LASTEXITCODE -ne 0){throw '宿主集成回归失败'}
  }
  & "$PSScriptRoot/cargo.ps1" fmt --all -- --check;if($LASTEXITCODE -ne 0){throw 'Rust 格式检查失败'}
  & "$PSScriptRoot/cargo.ps1" check -p ayase-companion --locked
  if($LASTEXITCODE -ne 0){throw '宿主检查失败'}
  # Rust panic locations can retain build-machine paths even when symbols are stripped.
  $previousFlags=$env:CARGO_ENCODED_RUSTFLAGS
  try{
    $remapFlags=@()
    $prefixes=@((Split-Path $projectRoot -Parent),$env:USERPROFILE,$env:CARGO_HOME,$env:RUSTUP_HOME) | Where-Object {$_} | Select-Object -Unique
    $prefixIndex=0
    foreach($prefix in $prefixes){
      $prefixIndex++
      $normalized=[IO.Path]::GetFullPath($prefix).TrimEnd('\','/')
      foreach($variant in @($normalized,$normalized.Replace('\','/')) | Select-Object -Unique){
        $remapFlags+="--remap-path-prefix=$variant=/build/source-$prefixIndex"
      }
    }
    $separator=[string][char]31
    $env:CARGO_ENCODED_RUSTFLAGS=(@($previousFlags | Where-Object {$_})+($remapFlags -join $separator)) -join $separator
    & "$PSScriptRoot/cargo.ps1" build --release -p ayase-companion --features custom-protocol --locked
    if($LASTEXITCODE -ne 0){throw '桌面构建失败'}
  }finally{$env:CARGO_ENCODED_RUSTFLAGS=$previousFlags}
  Write-Output '构建完成：target/release/ayase-companion.exe。未发布到 GitHub。'
}finally{Pop-Location}
