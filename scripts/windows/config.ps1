# Windows 打包流水线的共享配置与工具函数（被点号引入，不单独执行）。
#
# 与 scripts/macos/config.sh 对齐：这一层只回答「事实」——版本号从哪来、
# 产物放哪、文件名怎么拼；动作拆到 build / package / installer 三个脚本里，
# 彼此不互相调用，每个都能单独跑（对已有产物重打包、补校验和）。
#
# 用 powershell 而不是 bash：Windows runner 上 pwsh 是原生选择，
# 路径分隔符、哈希、压缩都不用绕道。

$ErrorActionPreference = 'Stop'

function Get-SadpConfig {
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
    $arch        = 'x86_64'
    $tag         = "sadp-discover-$version-windows-$arch"

    [pscustomobject]@{
        Root        = $root
        Version     = $version
        # Inno 的 VersionInfoVersion 只收 x.y.z.w 四段数字，预发布后缀要丢掉。
        FileVersion = Get-SadpFileVersion $version
        Arch        = $arch
        Tag         = $tag
        Dist        = $dist
        WindowsDist = $windowsDist
        # 安装器与 zip 共用的那批文件；zip 里这一层会被包成 $Tag 目录。
        Payload     = Join-Path $windowsDist 'payload'
        Zip         = Join-Path $dist "$tag.zip"
        SetupName   = "$tag-setup"
        Setup       = Join-Path $windowsDist "$tag-setup.exe"
        Checksums   = Join-Path $dist 'SHA256SUMS-windows.txt'
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
