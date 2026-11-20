# 装一遍、跑一遍、卸一遍（MSI）。
#
# 与 smoke-install.ps1 覆盖同样的断言，只是走 msiexec。只能在主机架构 == 包架构时跑。
# 注意：MSI 与 Inno 安装器二选一，冒烟前确保机器上没有另一份安装。

[CmdletBinding()]
param([ValidateSet('x64', 'arm64')][string]$Arch = 'x64')

. (Join-Path $PSScriptRoot 'config.ps1')
$cfg = Get-SadpConfig -Arch $Arch

if (-not (Test-Path $cfg.Msi)) { throw "缺少 $($cfg.Msi)（先跑 scripts/windows/msi.ps1 -Arch $Arch）" }

# 必须装到默认目录：MSI 的 INSTALLFOLDER 覆盖值不会在卸载时保留下来，
# 用自定义目录会「装到 A、卸的时候去删 B」，文件与 PATH 都会残留。
$InstallDir = Join-Path $env:LOCALAPPDATA 'Programs/sadp-discover'
if (Test-Path $InstallDir) { throw "默认安装目录已存在，先手工清理：$InstallDir" }

$cli = Join-Path $InstallDir 'sadp-discover.exe'
$gui = Join-Path $InstallDir 'sadp-discover-gui.exe'

function Get-UserPath {
    $item = Get-ItemProperty -Path 'HKCU:\Environment' -Name Path -ErrorAction SilentlyContinue
    if ($null -eq $item) { return '' }
    return [string]$item.Path
}

function Test-PathListed([string]$Value) {
    $paths = Get-UserPath
    if ([string]::IsNullOrEmpty($paths)) { return $false }
    # MSI 的 Environment 表把目录属性格式化成带尾反斜杠的路径，Inno 不带；
    # 比较前统一去掉，避免因为一个 `\` 误判。
    $wanted = $Value.TrimEnd('\')
    return @($paths.Split(';') | Where-Object { $_.TrimEnd('\') -eq $wanted }).Count -gt 0
}

function Find-ProductCode {
    $roots = @(
        'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall',
        'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall',
        'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall'
    )
    foreach ($root in $roots) {
        if (-not (Test-Path $root)) { continue }
        foreach ($key in Get-ChildItem $root -ErrorAction SilentlyContinue) {
            $props = Get-ItemProperty $key.PSPath -ErrorAction SilentlyContinue
            if ($props.DisplayName -eq 'sadp-discover') { return $key.PSChildName }
        }
    }
    return $null
}

$log = Join-Path $env:TEMP "sadp-msi-install-$(Get-Random).log"
$installArgs = @('/i', "`"$($cfg.Msi)`"", '/qn', '/norestart', '/l*v', "`"$log`"")

Write-Step "静默安装 MSI 到 $InstallDir"
$proc = Start-Process -FilePath 'msiexec.exe' -Wait -PassThru -ArgumentList ($installArgs -join ' ')
if ($proc.ExitCode -notin @(0, 3010)) {
    if (Test-Path $log) { Get-Content $log -Tail 30 | Write-Host }
    throw "msiexec 安装退出码 $($proc.ExitCode)（日志：$log）"
}

foreach ($file in @($cli, $gui)) {
    if (-not (Test-Path $file)) { throw "安装后缺少 $file" }
}
Invoke-SadpVersionProbe $cli
Invoke-SadpVersionProbe $gui

$startMenu = Join-Path ([Environment]::GetFolderPath('Programs')) 'sadp-discover.lnk'
if (-not (Test-Path $startMenu)) { throw "没有创建开始菜单快捷方式：$startMenu" }
if (-not (Test-PathListed $InstallDir)) { throw "PATH 里没有 $InstallDir（HKCU\Environment）" }

$code = Find-ProductCode
if (-not $code) { throw '在卸载注册表里找不到 sadp-discover 的 ProductCode' }
Write-Step "安装检查通过（ProductCode $code）"

Write-Step '静默卸载'
$uninstallArgs = @('/x', $code, '/qn', '/norestart', '/l*v', "`"$log`"")
$proc = Start-Process -FilePath 'msiexec.exe' -Wait -PassThru -ArgumentList ($uninstallArgs -join ' ')
if ($proc.ExitCode -notin @(0, 3010)) { throw "msiexec 卸载退出码 $($proc.ExitCode)（日志：$log）" }

$deadline = (Get-Date).AddSeconds(60)
while ((Test-Path $cli) -and ((Get-Date) -lt $deadline)) { Start-Sleep -Milliseconds 500 }
if (Test-Path $cli) { throw "卸载后 $cli 还在" }
if (Test-PathListed $InstallDir) { throw "卸载后 PATH 里还有 $InstallDir" }
if (Find-ProductCode) { throw '卸载后注册表里还有 sadp-discover' }

if (Test-Path $InstallDir) { Remove-Item $InstallDir -Recurse -Force -ErrorAction SilentlyContinue }
Remove-Item $log -Force -ErrorAction SilentlyContinue
Write-Step '卸载检查通过'
