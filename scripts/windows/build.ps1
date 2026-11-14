# 构建 Windows release 二进制，并验证它们真的能启动。
#
# 单独跑：pwsh -File scripts/windows/build.ps1
# 已经构建过：加 -SkipBuild 只做验证。

[CmdletBinding()]
param([switch]$SkipBuild)

. (Join-Path $PSScriptRoot 'config.ps1')
$cfg = Get-SadpConfig

$targetDir = Join-Path $cfg.Root 'target/release'
$exes = @(
    (Join-Path $targetDir 'sadp-discover.exe'),
    (Join-Path $targetDir 'sadp-discover-gui.exe')
)

if ($SkipBuild) {
    Write-Step '跳过 cargo build（-SkipBuild）'
} else {
    Write-Step "构建 sadp-discover $($cfg.Version)（release，全量 LTO）"
    Push-Location $cfg.Root
    try {
        & cargo build --release --locked -p sadp-cli -p sadp-gui
        if ($LASTEXITCODE -ne 0) { throw "cargo build 失败（退出码 $LASTEXITCODE）" }
    } finally {
        Pop-Location
    }
}

foreach ($exe in $exes) {
    Invoke-SadpVersionProbe $exe
}

# 资源有没有真的嵌进去：文件属性里的版本字符串来自 rc.exe，
# 没嵌上时这个字段是空的（那种 exe 在资源管理器里也没有图标）。
foreach ($exe in $exes) {
    $info = (Get-Item $exe).VersionInfo
    $name = [IO.Path]::GetFileName($exe)
    if ([string]::IsNullOrWhiteSpace($info.FileVersion)) {
        throw "$name 没有版本信息：rc.exe 没有把 packaging/windows/app.rc.in 编进去"
    }
    if (-not $info.FileVersion.StartsWith($cfg.Version)) {
        Write-Warn "$name 的 FileVersion 是 $($info.FileVersion)，与 Cargo.toml 的 $($cfg.Version) 不一致"
    }
}

Write-Step '二进制就绪'
