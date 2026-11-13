# Gate 1 24h soak: launch the TUI, sample for N hours, write a summary.
# Usage: powershell -NoProfile -ExecutionPolicy Bypass -File .gate1/soak/start-24h.ps1 [-Hours 24] [-IntervalSeconds 60]
# NOTE: keep this file ASCII-only. Windows PowerShell 5.1 reads BOM-less files as ANSI,
# and UTF-8 Chinese bytes can decode into a backslash that escapes the closing quote.
param(
    [double]$Hours = 24,
    [int]$IntervalSeconds = 60
)

$ErrorActionPreference = "Stop"
$here = $PSScriptRoot
$repo = Split-Path -Parent (Split-Path -Parent $here)

$release = Join-Path $repo "target\release\sadp-discover.exe"
$debug = Join-Path $repo "target\debug\sadp-discover.exe"
if (Test-Path $release) {
    $exe = $release
}
elseif (Test-Path $debug) {
    $exe = $debug
}
else {
    throw "sadp-discover.exe not found; run: cargo build --release -p sadp-cli"
}

$battery = Get-CimInstance Win32_Battery -ErrorAction SilentlyContinue
if ($battery -and $battery.BatteryStatus -ne 2) {
    throw "not on AC power (BatteryStatus=$($battery.BatteryStatus)); a 24h soak would be cut short."
}

$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$csv = Join-Path $here "soak-$stamp.csv"
$summary = Join-Path $here "soak-$stamp.summary.txt"

Write-Output "launching $exe (repeat 3s, default port 37020)"
$proc = Start-Process -FilePath $exe -ArgumentList "--repeat", "3000", "--log-level", "debug" -WindowStyle Minimized -PassThru

try {
    & (Join-Path $here "sample.ps1") -Out $csv -Minutes ([int]($Hours * 60)) -IntervalSeconds $IntervalSeconds
}
finally {
    if (-not $proc.HasExited) { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue }
}

$rows = @(Import-Csv $csv | Where-Object { [int64]$_.working_set_bytes -gt 0 })
$lines = @()
if ($rows.Count -eq 0) {
    $lines += "no samples: process exited immediately"
}
else {
    $ws = $rows | ForEach-Object { [int64]$_.working_set_bytes }
    $pv = $rows | ForEach-Object { [int64]$_.private_bytes }
    $hd = $rows | ForEach-Object { [int]$_.handles }
    $th = $rows | ForEach-Object { [int]$_.threads }
    $first = $rows[0]
    $last = $rows[-1]
    $lines += "samples: $($rows.Count)  interval: ${IntervalSeconds}s"
    $lines += ("working_set: min {0:N0}  max {1:N0}  first {2:N0}  last {3:N0}" -f ($ws | Measure-Object -Minimum).Minimum, ($ws | Measure-Object -Maximum).Maximum, [int64]$first.working_set_bytes, [int64]$last.working_set_bytes)
    $lines += ("private_bytes: min {0:N0}  max {1:N0}  first {2:N0}  last {3:N0}" -f ($pv | Measure-Object -Minimum).Minimum, ($pv | Measure-Object -Maximum).Maximum, [int64]$first.private_bytes, [int64]$last.private_bytes)
    $lines += ("handles: min {0}  max {1}  first {2}  last {3}" -f ($hd | Measure-Object -Minimum).Minimum, ($hd | Measure-Object -Maximum).Maximum, $first.handles, $last.handles)
    $lines += ("threads: min {0}  max {1}  first {2}  last {3}" -f ($th | Measure-Object -Minimum).Minimum, ($th | Measure-Object -Maximum).Maximum, $first.threads, $last.threads)
    $lines += "process_exited_early: $($rows.Count -lt [math]::Floor($Hours * 3600 / $IntervalSeconds))"
}
$lines | Tee-Object -FilePath $summary
Write-Output "summary written to $summary"
