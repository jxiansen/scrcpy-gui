param(
    [Parameter(Mandatory = $true)][string]$ExePath,
    [int]$TimeoutMs = 20000
)

$ErrorActionPreference = 'Stop'
if (-not (Test-Path $ExePath)) { throw "Executable not found: $ExePath" }

$proc = Start-Process -FilePath $ExePath -PassThru
$sw = [System.Diagnostics.Stopwatch]::StartNew()
while ($sw.ElapsedMilliseconds -lt $TimeoutMs) {
    $proc.Refresh()
    if ($proc.MainWindowTitle) { break }
    Start-Sleep -Milliseconds 50
}
$sw.Stop()

if ($proc.MainWindowTitle) {
    Write-Host ("STARTUP {0}: {1} ms (pid {2}, title '{3}')" -f $ExePath, $sw.ElapsedMilliseconds, $proc.Id, $proc.MainWindowTitle)
} else {
    Write-Host ("STARTUP FAILED/TIMEOUT {0}: > {1} ms (pid {2})" -f $ExePath, $TimeoutMs, $proc.Id)
}
