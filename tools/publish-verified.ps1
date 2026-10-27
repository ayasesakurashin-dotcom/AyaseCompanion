param([Parameter(Mandatory=$true)][string]$Version,[Parameter(Mandatory=$true)][string]$ApprovalFile)
$ErrorActionPreference='Stop'
$projectRoot=Split-Path $PSScriptRoot -Parent
Push-Location $projectRoot
try{
  if($Version -notmatch '^0\.[1-9][0-9]*\.[0-9]+$|^1\.0\.0$'){throw '版本号无效'}
  $approval=Get-Content -Raw -LiteralPath $ApprovalFile | ConvertFrom-Json
  $approvalPath=(Resolve-Path -LiteralPath $ApprovalFile).Path
  git check-ignore --quiet -- $approvalPath
  if($LASTEXITCODE -ne 0){throw '验收文件必须位于忽略提交的本机目录'}
  $trackedApproval=git ls-files -- $approvalPath
  if($trackedApproval){throw '验收文件不得进入公开提交'}
  $head=(git rev-parse HEAD).Trim()
  if($approval.userApproved -ne $true -or $approval.version -ne $Version -or $approval.commit -ne $head -or !$approval.userConfirmation){throw '必须先记录用户明确验收通过的版本与当前提交；不能以自动测试代替。'}
  if($Version -eq '1.0.0' -and $approval.allDesignItemsVerified -ne $true){throw '1.0.0 需要全部约定设计项通过。'}
  if(git status --porcelain){throw '工作目录仍有未提交修改'}
  & node "$PSScriptRoot/audit-public-release.mjs" --ref $head
  if($LASTEXITCODE -ne 0){throw '公开历史审计失败；请将私人记录留在本机'}
  $tag='v'+$Version
  git rev-parse --verify --quiet "refs/tags/$tag" | Out-Null
  if($LASTEXITCODE -eq 0){throw '此标签已存在，不允许覆盖或移动'}
  $remoteTags=git ls-remote --tags origin "refs/tags/$tag"
  if($LASTEXITCODE -ne 0){throw '无法检查远端标签，请完成 GitHub 登录'}
  if($remoteTags){throw '远端标签已存在，不允许覆盖'}
  git tag -a $tag -m "User-verified AyaseCompanion $Version"
  if($LASTEXITCODE -ne 0){throw '创建标签失败'}
  git push --atomic origin HEAD:main "refs/tags/$tag"
  if($LASTEXITCODE -ne 0){throw '推送未完成；本地标签已保留，请检查远端状态后恢复，不要强制推送。'}
  Write-Output '源码与不可变版本标签已推送。随后在 GitHub 创建同名 Release，上传本版本独立产物与散列，不覆盖旧附件。'
}finally{Pop-Location}
