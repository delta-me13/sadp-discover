# 把 release 二进制收拾成 dist/windows/payload(-arm64)，并打一个便携 zip。
#
# payload 是安装器、MSI 与 zip 的公共输入；zip 里额外包一层带版本号的目录，
# 免得用户解压到下载目录时把 exe 撒一地。

[CmdletBinding()]
param([ValidateSet('x64', 'arm64')][string]$Arch = 'x64')

. (Join-Path $PSScriptRoot 'config.ps1')
$cfg = Get-SadpConfig -Arch $Arch

$sources = @(
    (Join-Path $cfg.TargetDir 'sadp-discover.exe'),
    (Join-Path $cfg.TargetDir 'sadp-discover-gui.exe'),
    $cfg.Readme
) + $cfg.Licenses

foreach ($src in $sources) {
    if (-not (Test-Path $src)) { throw "缺少 $src（先跑 scripts/windows/build.ps1 -Arch $Arch）" }
}

Write-Step "整理 payload（$($cfg.Arch)）"
if (Test-Path $cfg.Payload) { Remove-Item $cfg.Payload -Recurse -Force }
New-Item -ItemType Directory -Force -Path $cfg.Payload | Out-Null
foreach ($src in $sources) { Copy-Item $src $cfg.Payload }

Write-Step "打 zip：$(Split-Path $cfg.Zip -Leaf)"
$zipRoot = Join-Path $cfg.WindowsDist "zip/$($cfg.Tag)"
if (Test-Path $zipRoot) { Remove-Item $zipRoot -Recurse -Force }
New-Item -ItemType Directory -Force -Path (Split-Path $zipRoot) | Out-Null
Copy-Item $cfg.Payload $zipRoot -Recurse
if (Test-Path $cfg.Zip) { Remove-Item $cfg.Zip -Force }
Compress-Archive -Path $zipRoot -DestinationPath $cfg.Zip -CompressionLevel Optimal

# 顶层必须只有一个目录：解压出来应该是 sadp-discover-<版本>-windows-<arch>/。
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [IO.Compression.ZipFile]::OpenRead($cfg.Zip)
try {
    $tops = @($archive.Entries | ForEach-Object { ($_.FullName -split '/')[0] } | Sort-Object -Unique)
} finally {
    $archive.Dispose()
}
if ($tops.Count -ne 1 -or $tops[0] -ne $cfg.Tag) {
    throw "zip 顶层目录不对：$($tops -join ', ')（应当是 $($cfg.Tag)）"
}

Write-Host "    $($cfg.Zip)"
