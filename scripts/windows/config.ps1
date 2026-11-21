# Windows 打包流水线的共享配置与工具函数（被点号引入，不单独执行）。
#
# 与 scripts/macos/config.sh 对齐：这一层只回答「事实」——版本号从哪来、
# 产物放哪、文件名怎么拼；动作拆到 build / package / installer / msi 四个脚本里，
# 彼此不互相调用，每个都能单独跑（对已有产物重打包、补校验和）。
#
# 所有函数都接受 -Arch x64|arm64（默认 x64）。两个架构共用 dist/ 与 dist/windows/，
# 靠文件名里的 x86_64/arm64 与 payload/payload-arm64 区分，互不覆盖。
#
# 用 powershell 而不是 bash：Windows runner 上 pwsh 是原生选择，
# 路径分隔符、哈希、压缩都不用绕道。

$ErrorActionPreference = 'Stop'

function Get-SadpConfig {
    param([ValidateSet('x64', 'arm64')][string]$Arch = 'x64')

    $root = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path

    # 版本号取自 cargo 本身，而不是逐行扫描 Cargo.toml。
    # 显式给 --manifest-path：脚本可以从任何目录被调用，不能依赖当前工作目录。
    $manifest = Join-Path $root 'Cargo.toml'
    $pkgid = & cargo pkgid -p sadp-cli -q --manifest-path $manifest 2>$null
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($pkgid)) {
        throw '无法从 cargo 读取版本号：cargo pkgid -p sadp-cli 失败'
    }
    $version = ($pkgid.Trim() -replace '.*[#@]', '')
    if ($version -notmatch '^[0-9]+\.[0-9]+\.[0-9]+') {
        throw "版本号格式异常：$version"
    }

    $dist        = Join-Path $root 'dist'
    $windowsDist = Join-Path $dist 'windows'

    # 文件名里的架构用 Rust 的习惯写法（x86_64 / arm64），与 macOS 侧一致。
    $archSlug = if ($Arch -eq 'arm64') { 'arm64' } else { 'x86_64' }
    $rustTarget = if ($Arch -eq 'arm64') { 'aarch64-pc-windows-msvc' } else { 'x86_64-pc-windows-msvc' }
    # x64 就是宿主架构，cargo 默认输出到 target/release；arm64 在 target/<triple>/release。
    $targetDir = if ($Arch -eq 'x64') {
        Join-Path $root 'target/release'
    } else {
        Join-Path $root "target/$rustTarget/release"
    }
    $tag = "sadp-discover-$version-windows-$archSlug"

    # 两个架构的 payload 不能共用：交叉构建时后跑的会覆盖先跑的。
    $payloadName = if ($Arch -eq 'x64') { 'payload' } else { 'payload-arm64' }
    # x64 沿用历史文件名；arm64 带后缀，避免同一份 SHA256SUMS 互相覆盖。
    $checksums = if ($Arch -eq 'x64') {
        Join-Path $dist 'SHA256SUMS-windows.txt'
    } else {
        Join-Path $dist "SHA256SUMS-windows-$archSlug.txt"
    }

    # Inno 的 VersionInfoVersion 只收 x.y.z.w 四段数字，预发布后缀要丢掉。
    $fileVersion = Get-SadpFileVersion $version
    # WiX 的 ProductVersion 只收三段数字（0.1.0-rc.1 → 0.1.0）：
    # 第四段和预发布后缀都会让 light 直接报错。
    $msiVersion = ($fileVersion.Split('.') | Select-Object -First 3) -join '.'

    [pscustomobject]@{
        Root        = $root
        Version     = $version
        FileVersion = $fileVersion
        MsiVersion  = $msiVersion
        Arch        = $Arch
        ArchSlug    = $archSlug
        RustTarget  = $rustTarget
        TargetDir   = $targetDir
        # Inno 的 ArchitecturesAllowed 取值：x64 用 x64compatible（ARM64 上可仿真安装），
        # arm64 用 arm64（在 x64 上拒绝安装）。
        InnoArch    = if ($Arch -eq 'x64') { 'x64compatible' } else { 'arm64' }
        Tag         = $tag
        Dist        = $dist
        WindowsDist = $windowsDist
        # 安装器、MSI 与 zip 共用的那批文件；zip 里这一层会被包成 $Tag 目录。
        Payload     = Join-Path $windowsDist $payloadName
        PayloadName = $payloadName
        Zip         = Join-Path $dist "$tag.zip"
        SetupName   = "$tag-setup"
        Setup       = Join-Path $windowsDist "$tag-setup.exe"
        Msi         = Join-Path $windowsDist "$tag.msi"
        Checksums   = $checksums
        Readme      = Join-Path $root 'README.md'
        Licenses    = @(
            (Join-Path $root 'LICENSE-MIT'),
            (Join-Path $root 'LICENSE-APACHE')
        )
    }
}

# '0.1.0' / '0.1.0-rc.1' → '0.1.0.0'：四段数字，缺的补 0。
function Get-SadpFileVersion {
    param([string]$Version)
    $parts = @($Version.Split('.') | Select-Object -First 3 | ForEach-Object {
        $digits = ($_ -split '-')[0]
        if ($digits -match '^[0-9]+$') { $digits } else { '0' }
    })
    while ($parts.Count -lt 3) { $parts += '0' }
    return ($parts + '0') -join '.'
}

function Write-Step {
    param([string]$Message)
    Write-Host "==> $Message"
}

function Write-Warn {
    param([string]$Message)
    Write-Host "warning: $Message" -ForegroundColor Yellow
}

# PE 头里的 Machine 字段：0x8664 = x64，0xAA64 = ARM64。
# 交叉编译最容易出的错是「命令成功了，但产物还是宿主架构」，这里直接读文件断言。
function Get-SadpPeMachine {
    param([string]$Path)

    $stream = [IO.File]::OpenRead($Path)
    try {
        $reader = [IO.BinaryReader]::new($stream)
        $stream.Position = 0x3C
        $peOffset = $reader.ReadInt32()
        $stream.Position = $peOffset + 4
        return [int]$reader.ReadUInt16()
    } finally {
        $stream.Dispose()
    }
}

function Assert-SadpPeArch {
    param([string]$Path, [ValidateSet('x64', 'arm64')][string]$Arch)

    $expected = if ($Arch -eq 'arm64') { 0xAA64 } else { 0x8664 }
    $actual = Get-SadpPeMachine $Path
    $name = [IO.Path]::GetFileName($Path)
    if ($actual -ne $expected) {
        $seen = switch ($actual) {
            0x8664 { 'x64' }
            0xAA64 { 'arm64' }
            0x14C { 'x86' }
            default { ('0x{0:X}' -f $actual) }
        }
        throw "$name 的 PE 架构是 $seen，期望 $Arch（交叉编译没有真正生效）"
    }
}

# 产物能不能真的跑起来：GUI 是 windows 子系统进程，PowerShell 不会等它，
# 所以统一走 Start-Process + 重定向，拿退出码与标准输出，避免竞态。
function Invoke-SadpVersionProbe {
    param([string]$Exe)

    if (-not (Test-Path $Exe)) { throw "缺少产物：$Exe" }

    $out = New-TemporaryFile
    try {
        $startArgs = @{
            FilePath               = $Exe
            ArgumentList           = '--version'
            NoNewWindow            = $true
            Wait                   = $true
            PassThru               = $true
            RedirectStandardOutput = $out.FullName
        }
        $proc = Start-Process @startArgs
        $text = (Get-Content $out.FullName -Raw) -replace '\s+$', ''
        if ($proc.ExitCode -ne 0) {
            throw "$([IO.Path]::GetFileName($Exe)) --version 退出码 $($proc.ExitCode)"
        }
        if ([string]::IsNullOrWhiteSpace($text)) {
            throw "$([IO.Path]::GetFileName($Exe)) --version 没有任何输出（控制台没挂上？）"
        }
        Write-Host "    $text"
    } finally {
        Remove-Item $out.FullName -ErrorAction SilentlyContinue
    }
}

# 找 WiX 3（candle/light）：本地可用 WIX 环境变量、默认安装目录，或 CI 镜像的
# Program Files (x86)\WiX Toolset v3.14\bin。找不到时返回 $null，由调用方决定是否跳过。
function Find-SadpWixBin {
    $candidates = @()
    if ($env:WIX) { $candidates += $env:WIX }
    $candidates += (Join-Path ${env:ProgramFiles(x86)} 'WiX Toolset v3.14/bin')
    $candidates += (Join-Path $env:ProgramFiles 'WiX Toolset v3.14/bin')
    foreach ($dir in $candidates) {
        if ($dir -and (Test-Path (Join-Path $dir 'candle.exe'))) { return $dir }
    }
    return $null
}
