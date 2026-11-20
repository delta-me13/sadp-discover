# 构建 Windows release 二进制，并验证它们真的能启动。
#
# 单独跑：pwsh -File scripts/windows/build.ps1
# 已经构建过：加 -SkipBuild 只做验证。
# 交叉编译：pwsh -File scripts/windows/build.ps1 -Arch arm64
#   （需要 VS 的 ARM64 C++ 组件；CI 的 windows-latest 自带）

[CmdletBinding()]
param(
    [switch]$SkipBuild,
    [ValidateSet('x64', 'arm64')][string]$Arch = 'x64'
)

. (Join-Path $PSScriptRoot 'config.ps1')
$cfg = Get-SadpConfig -Arch $Arch

$needsCross = ($Arch -ne 'x64')
$hostArch = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
$canRun = ($Arch -eq 'x64' -and $hostArch -eq 'X64') -or ($Arch -eq 'arm64' -and $hostArch -eq 'Arm64')

if ($needsCross) {
    . (Join-Path $PSScriptRoot 'msvc-env.ps1')
    Initialize-SadpMsvcEnvironment -Arch $Arch

    # 目标 std 不在时自动补；rustup 本身是幂等的，装过就秒回。
    $installed = & rustup target list --installed
    if ($installed -notcontains $cfg.RustTarget) {
        Write-Step "安装 Rust target：$($cfg.RustTarget)"
        & rustup target add $cfg.RustTarget
        if ($LASTEXITCODE -ne 0) { throw "rustup target add $($cfg.RustTarget) 失败" }
    }
}

$exes = @(
    (Join-Path $cfg.TargetDir 'sadp-discover.exe'),
    (Join-Path $cfg.TargetDir 'sadp-discover-gui.exe')
)

if ($SkipBuild) {
    Write-Step '跳过 cargo build（-SkipBuild）'
} else {
    Write-Step "构建 sadp-discover $($cfg.Version)（$($cfg.Arch)，release，全量 LTO）"
    Push-Location $cfg.Root
    try {
        $cargoArgs = @('build', '--release', '--locked', '-p', 'sadp-cli', '-p', 'sadp-gui')
        if ($needsCross) { $cargoArgs += @('--target', $cfg.RustTarget) }
        & cargo @cargoArgs
        if ($LASTEXITCODE -ne 0) { throw "cargo build 失败（退出码 $LASTEXITCODE）" }
    } finally {
        Pop-Location
    }
}

foreach ($exe in $exes) {
    if (-not (Test-Path $exe)) { throw "缺少产物：$exe（先跑 cargo build）" }
    # 交叉编译先断言架构：在 x64 上把 arm64 产物当 x64 探针跑，会得到
    # 「无法启动此程序」这种与根因无关的报错。
    Assert-SadpPeArch -Path $exe -Arch $cfg.Arch
    if ($canRun) {
        Invoke-SadpVersionProbe $exe
    } else {
        Write-Host "    跳过运行探针：$hostArch 主机不能执行 $($cfg.Arch) 产物"
    }
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

Write-Step "二进制就绪（$($cfg.Arch)）"
