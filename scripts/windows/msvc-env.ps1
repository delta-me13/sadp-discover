# 交叉编译 ARM64 所需的 MSVC 环境（被 build.ps1 点号引入）。
#
# 为什么不调用 vcvarsall.bat：它只能通过 cmd.exe 跑，而某些受限会话里 cmd.exe 直接
# 返回 ERROR_ELEVATION_REQUIRED（os error 740）。这里改为自己拼 PATH / LIB / INCLUDE：
# 链接器用 Hostx64\x64\link.exe（link.exe 本身是跨目标架构的），库指向 MSVC 的
# lib\arm64 与 Windows SDK 的 ucrt/um\arm64。
#
# 前置条件：Visual Studio 安装了 Microsoft.VisualStudio.Component.VC.Tools.ARM64
# （CI 的 windows-latest 镜像自带；本地 Build Tools 需要手动勾选）。缺这个组件时
# lib\arm64 里只有 ASan 运行库、没有 msvcrt.lib，链接会报 LNK1104，这里提前给出人话。

function Initialize-SadpMsvcEnvironment {
    param([ValidateSet('arm64')][string]$Arch = 'arm64')

    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    if (-not (Test-Path $vswhere)) {
        throw '找不到 vswhere.exe：需要安装 Visual Studio Build Tools（含 C++ 工作负载）'
    }
    $vsPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($vsPath)) {
        throw 'vswhere 没有找到带 C++ 工具链的 Visual Studio'
    }

    $msvcRoot = Get-ChildItem (Join-Path $vsPath 'VC/Tools/MSVC') -Directory |
        Sort-Object Name -Descending | Select-Object -First 1
    if (-not $msvcRoot) { throw "找不到 MSVC 工具链目录：$vsPath\VC\Tools\MSVC" }

    $libDir = Join-Path $msvcRoot.FullName "lib/$Arch"
    $hasLocalCrt = Test-Path (Join-Path $libDir 'msvcrt.lib')

    # 没有装 VS 的 ARM64 组件时的退路：xwin 从微软的分发源下载 CRT/SDK 并解包，
    # 只借它的库与头文件，link.exe / rc.exe 仍用本机的（链接器本身跨架构）。
    #   xwin --accept-license splat --output D:\tools\xwin-splat --preserve-ms-arch-notation --disable-symlinks
    #   $env:SADP_XWIN_SPLAT = 'D:\tools\xwin-splat'
    $xwinRoot = $env:SADP_XWIN_SPLAT
    $useXwin = $false
    if (-not $hasLocalCrt) {
        if ($xwinRoot -and (Test-Path (Join-Path $xwinRoot "crt/lib/$Arch/msvcrt.lib"))) {
            $useXwin = $true
        } else {
            throw "缺少 $Arch 的 MSVC CRT（$libDir\msvcrt.lib）。安装 Visual Studio 组件 " +
                  "'Microsoft.VisualStudio.Component.VC.Tools.ARM64'，或设置 SADP_XWIN_SPLAT 指向 xwin splat 目录；" +
                  "CI 的 windows-latest 镜像已自带该组件。"
        }
    }

    # Windows SDK 的安装根：优先读注册表（32/64 两个视图都看），否则退回默认目录。
    $kitsRoot = $null
    foreach ($key in @(
            'HKLM:\SOFTWARE\Microsoft\Windows Kits\Installed Roots',
            'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows Kits\Installed Roots')) {
        $value = (Get-ItemProperty $key -ErrorAction SilentlyContinue).KitsRoot10
        if ($value -and (Test-Path $value)) { $kitsRoot = $value; break }
    }
    if (-not $kitsRoot) { $kitsRoot = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits/10' }
    if (-not (Test-Path $kitsRoot)) { throw "找不到 Windows SDK：$kitsRoot" }

    $sdkVersion = Get-ChildItem (Join-Path $kitsRoot 'Lib') -Directory |
        Where-Object { Test-Path (Join-Path $_.FullName "um/$Arch") } |
        Sort-Object Name -Descending | Select-Object -First 1
    if (-not $sdkVersion) { throw "Windows SDK 里没有 $Arch 库：$kitsRoot\Lib\*\um\$Arch" }

    $sdkBin = Join-Path $kitsRoot "bin/$($sdkVersion.Name)/x64"
    if (-not (Test-Path (Join-Path $sdkBin 'rc.exe'))) { throw "找不到 rc.exe：$sdkBin" }

    # Hostx64\x64 里有 link.exe / rc 之外的工具；SDK 的 x64 目录提供 rc.exe。
    # 这两个始终用本机的：链接器本身跨架构，rc.exe 只编资源、与目标架构无关。
    $env:PATH = "$($msvcRoot.FullName)\bin\Hostx64\x64;$sdkBin;$env:PATH"

    if ($useXwin) {
        $sdkInclude = Get-ChildItem (Join-Path $xwinRoot 'sdk/include') -Directory |
            Sort-Object Name -Descending | Select-Object -First 1
        if (-not $sdkInclude) { throw "xwin 目录里没有 sdk/include：$xwinRoot" }
        $env:LIB = @(
            (Join-Path $xwinRoot "crt/lib/$Arch")
            (Join-Path $xwinRoot "sdk/lib/ucrt/$Arch")
            (Join-Path $xwinRoot "sdk/lib/um/$Arch")
        ) -join ';'
        $env:INCLUDE = @(
            (Join-Path $xwinRoot 'crt/include')
            (Join-Path $sdkInclude.FullName 'ucrt')
            (Join-Path $sdkInclude.FullName 'um')
            (Join-Path $sdkInclude.FullName 'shared')
            (Join-Path $sdkInclude.FullName 'winrt')
        ) -join ';'
        Write-Step "交叉编译环境：XWin CRT/SDK $xwinRoot（$Arch），link.exe/rc.exe 用本机"
    } else {
        $env:LIB = @(
            $libDir
            (Join-Path $sdkVersion.FullName "ucrt/$Arch")
            (Join-Path $sdkVersion.FullName "um/$Arch")
        ) -join ';'
        $env:INCLUDE = @(
            (Join-Path $msvcRoot.FullName 'include')
            (Join-Path $kitsRoot "Include/$($sdkVersion.Name)/ucrt")
            (Join-Path $kitsRoot "Include/$($sdkVersion.Name)/um")
            (Join-Path $kitsRoot "Include/$($sdkVersion.Name)/shared")
            (Join-Path $kitsRoot "Include/$($sdkVersion.Name)/winrt")
        ) -join ';'
        Write-Step "交叉编译环境：MSVC $($msvcRoot.Name) + Windows SDK $($sdkVersion.Name)（$Arch）"
    }
}
