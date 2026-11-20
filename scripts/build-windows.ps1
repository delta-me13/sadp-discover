# 构建 Windows 分发包：便携 zip + Inno 安装器 + MSI + 校验和。
#
# 这是编排入口，真正的动作在 scripts/windows/ 下按职责拆开，可单独调用：
#   build.ps1      cargo build + 版本探针 + PE 架构 + 资源检查
#   package.ps1    dist/windows/payload(-arm64) + 便携 zip
#   installer.ps1  Inno Setup 编译安装器（含中文向导）
#   msi.ps1        WiX 3.14 编译 MSI
#   smoke-install.ps1 / smoke-msi.ps1  装一遍卸一遍（CI 用）
#
# 用法：
#   pwsh -File scripts/build-windows.ps1                       # x64 全家桶
#   pwsh -File scripts/build-windows.ps1 -Arch arm64           # ARM64 交叉编译
#   pwsh -File scripts/build-windows.ps1 -SkipBuild
#   pwsh -File scripts/build-windows.ps1 -SkipInstaller        # 不出 setup.exe
#   pwsh -File scripts/build-windows.ps1 -SkipMsi              # 不出 MSI
#   pwsh -File scripts/build-windows.ps1 -SkipInstaller -SkipMsi   # 只要便携 zip

[CmdletBinding()]
param(
    [switch]$SkipBuild,
    [switch]$SkipInstaller,
    [switch]$SkipMsi,
    [ValidateSet('x64', 'arm64')][string]$Arch = 'x64',
    [string]$Iscc,
    [string]$WixBin
)

. (Join-Path $PSScriptRoot 'windows/config.ps1')
$cfg = Get-SadpConfig -Arch $Arch

Write-Step "sadp-discover $($cfg.Version)（$($cfg.Arch)，输出：$($cfg.Dist)）"

$common = @{ Arch = $Arch }
& (Join-Path $PSScriptRoot 'windows/build.ps1') -SkipBuild:$SkipBuild @common
& (Join-Path $PSScriptRoot 'windows/package.ps1') @common

$artifacts = @($cfg.Zip)

if (-not $SkipInstaller) {
    $installerArgs = @{ Arch = $Arch }
    if ($Iscc) { $installerArgs.Iscc = $Iscc }
    & (Join-Path $PSScriptRoot 'windows/installer.ps1') @installerArgs
    $artifacts += $cfg.Setup
}

if (-not $SkipMsi) {
    $msiArgs = @{ Arch = $Arch }
    if ($WixBin) { $msiArgs.WixBin = $WixBin }
    & (Join-Path $PSScriptRoot 'windows/msi.ps1') @msiArgs
    $artifacts += $cfg.Msi
}

Write-Step '计算校验和'
$lines = foreach ($file in $artifacts) {
    if (-not (Test-Path $file)) { throw "校验和阶段缺少产物：$file" }
    $hash = (Get-FileHash -Path $file -Algorithm SHA256).Hash.ToLower()
    $hash + '  ' + (Split-Path $file -Leaf)
}
$lines | Set-Content -Path $cfg.Checksums -Encoding utf8
Get-Content $cfg.Checksums | ForEach-Object { Write-Host "    $_" }

Write-Step 'done'
Get-ChildItem $artifacts | ForEach-Object {
    Write-Host ('    {0}  {1:N1} MB' -f $_.Name, ($_.Length / 1MB))
}
