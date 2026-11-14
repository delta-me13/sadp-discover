# 构建 Windows 分发包：便携 zip + 安装器 + 校验和。
#
# 这是编排入口，真正的动作在 scripts/windows/ 下按职责拆开，可单独调用：
#   build.ps1      cargo build + 版本探针 + 资源检查
#   package.ps1    dist/windows/payload + 便携 zip
#   installer.ps1  Inno Setup 编译安装器
#   smoke-install.ps1  装一遍卸一遍（CI 用，等价于 macOS 的 pkg 安装冒烟）
#
# 用法：
#   pwsh -File scripts/build-windows.ps1
#   pwsh -File scripts/build-windows.ps1 -SkipBuild
#   pwsh -File scripts/build-windows.ps1 -SkipInstaller    # 只要便携 zip

[CmdletBinding()]
param(
    [switch]$SkipBuild,
    [switch]$SkipInstaller
)

. (Join-Path $PSScriptRoot 'windows/config.ps1')
$cfg = Get-SadpConfig

Write-Step "sadp-discover $($cfg.Version)（$($cfg.Arch)，输出：$($cfg.Dist)）"

& (Join-Path $PSScriptRoot 'windows/build.ps1') -SkipBuild:$SkipBuild
& (Join-Path $PSScriptRoot 'windows/package.ps1')
if (-not $SkipInstaller) {
    & (Join-Path $PSScriptRoot 'windows/installer.ps1')
}

Write-Step '计算校验和'
$artifacts = @($cfg.Zip)
if (Test-Path $cfg.Setup) { $artifacts += $cfg.Setup }
$lines = foreach ($file in $artifacts) {
    $hash = (Get-FileHash -Path $file -Algorithm SHA256).Hash.ToLower()
    $hash + '  ' + (Split-Path $file -Leaf)
}
$lines | Set-Content -Path $cfg.Checksums -Encoding utf8
Get-Content $cfg.Checksums | ForEach-Object { Write-Host "    $_" }

Write-Step 'done'
Get-ChildItem $artifacts | ForEach-Object {
    Write-Host ('    {0}  {1:N1} MB' -f $_.Name, ($_.Length / 1MB))
}
