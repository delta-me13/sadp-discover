# 用 Inno Setup 编译安装器：dist/windows/sadp-discover-<版本>-windows-<arch>-setup.exe
#
# Inno Setup 6 在 GitHub 的 windows runner 上是预装的；本地没装时用 -Iscc 指定路径，
# 或设 ISCC 环境变量。中文向导依赖随仓库携带的 languages\ChineseSimplified.isl。

[CmdletBinding()]
param(
    [string]$Iscc,
    [ValidateSet('x64', 'arm64')][string]$Arch = 'x64'
)

. (Join-Path $PSScriptRoot 'config.ps1')
$cfg = Get-SadpConfig -Arch $Arch

if (-not $Iscc) {
    $candidates = @()
    if ($env:ISCC) { $candidates += $env:ISCC }
    $candidates += (Join-Path ([Environment]::GetFolderPath('ProgramFilesX86')) 'Inno Setup 6/ISCC.exe')
    $candidates += (Join-Path ([Environment]::GetFolderPath('ProgramFiles')) 'Inno Setup 6/ISCC.exe')
    $Iscc = $candidates | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
}
if (-not $Iscc) {
    throw '找不到 ISCC.exe（Inno Setup 6）：用 -Iscc 指定，或安装 Inno Setup 6'
}
if (-not (Test-Path $cfg.Payload)) {
    throw "缺少 $($cfg.Payload)（先跑 scripts/windows/package.ps1 -Arch $Arch）"
}

$iss = Join-Path $cfg.Root 'packaging/windows/installer/sadp-discover.iss'
if (-not (Test-Path $iss)) { throw "缺少 $iss" }

Write-Step "编译安装器：$($cfg.SetupName).exe（$($cfg.Arch)）"
& $Iscc `
    "/DAppVersion=$($cfg.Version)" `
    "/DAppFileVersion=$($cfg.FileVersion)" `
    "/DOutputBaseFilename=$($cfg.SetupName)" `
    "/DArchAllowed=$($cfg.InnoArch)" `
    "/DPayloadDir=$($cfg.PayloadName)" `
    $iss
if ($LASTEXITCODE -ne 0) { throw "ISCC 失败（退出码 $LASTEXITCODE）" }
if (-not (Test-Path $cfg.Setup)) { throw "ISCC 报成功但产物不存在：$($cfg.Setup)" }

Write-Host "    $($cfg.Setup)"
