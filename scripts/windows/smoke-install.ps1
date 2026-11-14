# 装一遍、跑一遍、卸一遍。
#
# 这是唯一能验证「安装器布局真的对」的步骤：文件落在哪、快捷方式有没有、
# PATH 有没有加上、卸载后有没有留垃圾。release 与 dev 都用它做冒烟。

[CmdletBinding()]
param([string]$InstallDir)

. (Join-Path $PSScriptRoot 'config.ps1')
$cfg = Get-SadpConfig

if (-not $InstallDir) {
    $InstallDir = Join-Path $env:TEMP "sadp-discover-smoke-$(Get-Random)"
}
if (-not (Test-Path $cfg.Setup)) { throw "缺少 $($cfg.Setup)（先跑 scripts/windows/installer.ps1）" }

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
    return @($paths.Split(';') | Where-Object { $_ -eq $Value }).Count -gt 0
}

Write-Step "静默安装到 $InstallDir"
# 参数拼成一整条字符串而不是数组：Start-Process 用空格拼接数组且不加引号，
# 临时目录里一旦有空格（用户名带空格）就会被拆成两个参数。
$installArgs = '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /LOG "/DIR=' + $InstallDir + '"'
$proc = Start-Process -FilePath $cfg.Setup -Wait -PassThru -ArgumentList $installArgs
if ($proc.ExitCode -ne 0) { throw "安装器退出码 $($proc.ExitCode)" }

foreach ($file in @($cli, $gui)) {
    if (-not (Test-Path $file)) { throw "安装后缺少 $file" }
}
Invoke-SadpVersionProbe $cli
Invoke-SadpVersionProbe $gui

$startMenu = Join-Path ([Environment]::GetFolderPath('Programs')) 'sadp-discover.lnk'
if (-not (Test-Path $startMenu)) { throw "没有创建开始菜单快捷方式：$startMenu" }

if (-not (Test-PathListed $InstallDir)) { throw "PATH 里没有 $InstallDir（HKCU\Environment）" }
Write-Step '安装检查通过'

$uninstaller = Join-Path $InstallDir 'unins000.exe'
if (-not (Test-Path $uninstaller)) { throw "没有卸载器：$uninstaller" }

Write-Step '静默卸载'
$proc = Start-Process -FilePath $uninstaller -Wait -PassThru -ArgumentList '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART'
if ($proc.ExitCode -ne 0) { throw "卸载器退出码 $($proc.ExitCode)" }

# 卸载器会把自己复制到临时目录再干活，父进程先返回，所以这里轮询而不是立刻断言。
$deadline = (Get-Date).AddSeconds(60)
while ((Test-Path $cli) -and ((Get-Date) -lt $deadline)) { Start-Sleep -Milliseconds 500 }
if (Test-Path $cli) { throw "卸载后 $cli 还在" }
if (Test-PathListed $InstallDir) { throw "卸载后 PATH 里还有 $InstallDir" }

Remove-Item $InstallDir -Recurse -Force -ErrorAction SilentlyContinue
Write-Step '卸载检查通过'
