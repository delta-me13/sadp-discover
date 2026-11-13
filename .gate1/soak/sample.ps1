# Sample RESIDENT memory / handles / threads for sadp-discover.
# Usage: powershell -NoProfile -ExecutionPolicy Bypass -File sample.ps1 -Out soak.csv -Minutes 30 -IntervalSeconds 30
# Keep ASCII-only: PowerShell 5.1 treats BOM-less files as ANSI (see start-24h.ps1).
param(
    [Parameter(Mandatory = $true)][string]$Out,
    [int]$Minutes = 30,
    [int]$IntervalSeconds = 30,
    [string]$ProcessName = "sadp-discover"
)

"timestamp,working_set_bytes,private_bytes,handles,threads,cpu_seconds" | Out-File -Encoding utf8 $Out
$deadline = (Get-Date).AddMinutes($Minutes)
while ((Get-Date) -lt $deadline) {
    $p = Get-Process -Name $ProcessName -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($null -eq $p) {
        "$(Get-Date -Format o),0,0,0,0,0" | Add-Content $Out
        Write-Output "process exited before deadline"
        break
    }
    "$(Get-Date -Format o),$($p.WorkingSet64),$($p.PrivateMemorySize64),$($p.HandleCount),$($p.Threads.Count),$($p.CPU)" | Add-Content $Out
    Start-Sleep -Seconds $IntervalSeconds
}
Write-Output "sampling finished: $Out"
