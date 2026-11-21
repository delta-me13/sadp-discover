# 用 WiX Toolset 3.14 编译 MSI：dist/windows/sadp-discover-<版本>-windows-<arch>.msi
#
# WiX 在 GitHub 的 windows-latest 镜像上预装（默认路径 Program Files (x86)\WiX Toolset v3.14）。
# 本地可用 WIX 环境变量或 -WixBin 指向解压出来的 binaries 目录（无需安装）。
#
# 只打包、不签名：未签名的 MSI 与 setup.exe 一样会触发 SmartScreen，
# 证书到位后在 light.exe 之后补 signtool 即可。

[CmdletBinding()]
param(
    [string]$WixBin,
    [ValidateSet('x64', 'arm64')][string]$Arch = 'x64'
)

. (Join-Path $PSScriptRoot 'config.ps1')
$cfg = Get-SadpConfig -Arch $Arch

if (-not $WixBin) { $WixBin = Find-SadpWixBin }
if (-not $WixBin) {
    throw '找不到 WiX Toolset 3：用 -WixBin 指定 candle.exe 所在目录，或装 WiX Toolset v3.14'
}
$candle = Join-Path $WixBin 'candle.exe'
$light = Join-Path $WixBin 'light.exe'
foreach ($tool in @($candle, $light)) {
    if (-not (Test-Path $tool)) { throw "缺少 WiX 工具：$tool" }
}
if (-not (Test-Path $cfg.Payload)) {
    throw "缺少 $($cfg.Payload)（先跑 scripts/windows/package.ps1 -Arch $Arch）"
}

$wxs = Join-Path $cfg.Root 'packaging/windows/msi/product.wxs'
if (-not (Test-Path $wxs)) { throw "缺少 $wxs" }

$work = Join-Path $cfg.WindowsDist "msi-$($cfg.ArchSlug)"
if (Test-Path $work) { Remove-Item $work -Recurse -Force }
New-Item -ItemType Directory -Force -Path $work | Out-Null
$wixobj = Join-Path $work 'product.wixobj'

Write-Step "candle：$(Split-Path $cfg.Msi -Leaf)（$($cfg.Arch)）"
& $candle -nologo `
    -arch $cfg.Arch `
    "-dVersion=$($cfg.MsiVersion)" `
    "-dPlatform=$($cfg.Arch)" `
    "-dPayload=$($cfg.Payload)" `
    "-dRepoRoot=$($cfg.Root)" `
    -out $wixobj `
    $wxs
if ($LASTEXITCODE -ne 0) { throw "candle 失败（退出码 $LASTEXITCODE）" }

Write-Step 'light'
# ICE38/43/57 是对「ProgramMenuFolder 属于用户目录」的静态判断；本包是 perMachine +
# ALLUSERS=1，运行时该属性解析到所有用户的「开始菜单\程序」，快捷方式与 HKLM KeyPath
# 都是一致的。静态 ICE 看不到 ALLUSERS，故只抑制这三条，其余 ICE 保持开启。
$lightArgs = @('-nologo', '-out', $cfg.Msi, '-sice:ICE38', '-sice:ICE43', '-sice:ICE57')
& $light @lightArgs $wixobj
if ($LASTEXITCODE -ne 0) { throw "light 失败（退出码 $LASTEXITCODE）" }
if (-not (Test-Path $cfg.Msi)) { throw "light 报成功但产物不存在：$($cfg.Msi)" }

Write-Host "    $($cfg.Msi)"
