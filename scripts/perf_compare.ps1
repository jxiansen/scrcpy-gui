param(
    [int]$Seconds = 30,
    [string]$OldName = 'scrcpy-gui-v3',
    [string]$NewName = 'scrcpy-gui'
)

$ErrorActionPreference = 'Stop'
$cores = (Get-CimInstance Win32_Processor | Measure-Object NumberOfCores -Sum).Sum
if (-not $cores) { $cores = [Environment]::ProcessorCount }

function Get-GroupIds {
    param([string]$Name, [switch]$IncludeWebView)
    $main = @(Get-Process -Name $Name -ErrorAction SilentlyContinue | ForEach-Object { $_.Id })
    if (-not $main) { return @() }
    if (-not $IncludeWebView) { return @($main) }
    $web = @(Get-CimInstance Win32_Process |
        Where-Object { $_.Name -eq 'msedgewebview2.exe' -and $_.CommandLine -match [regex]::Escape($Name) } |
        ForEach-Object { $_.ProcessId })
    return @($main) + @($web)
}

function Sample {
    param([int[]]$Ids)
    $procs = @(Get-Process -Id $Ids -ErrorAction SilentlyContinue)
    if (-not $procs) { return $null }
    return [pscustomobject]@{
        Cpu  = ($procs | Measure-Object CPU -Sum).Sum
        Ws   = ($procs | Measure-Object WorkingSet64 -Sum).Sum
        Count = $procs.Count
    }
}

$oldIds = Get-GroupIds $OldName -IncludeWebView
$newIds = Get-GroupIds $NewName
Write-Host "Old PIDs ($OldName + WebView2): $($oldIds -join ', ')"
Write-Host "New PIDs ($NewName): $($newIds -join ', ')"
if (-not $oldIds -or -not $newIds) {
    Write-Error 'Both apps must be running to compare.'
}

$oldSamples = @()
$newSamples = @()
$oldPrev = Sample $oldIds
$newPrev = Sample $newIds

for ($i = 0; $i -lt $Seconds; $i++) {
    Start-Sleep -Seconds 1
    $o = Sample $oldIds
    $n = Sample $newIds
    if ($o -and $oldPrev) {
        $oldSamples += [pscustomobject]@{
            CpuPct = [math]::Max(0, (($o.Cpu - $oldPrev.Cpu) / ($cores * 1)) * 100)
            WsMb   = $o.Ws / 1MB
        }
    }
    if ($n -and $newPrev) {
        $newSamples += [pscustomobject]@{
            CpuPct = [math]::Max(0, (($n.Cpu - $newPrev.Cpu) / ($cores * 1)) * 100)
            WsMb   = $n.Ws / 1MB
        }
    }
    $oldPrev = $o
    $newPrev = $n
}

function Report {
    param([string]$Label, $Samples)
    if (-not $Samples -or $Samples.Count -eq 0) { Write-Host "$Label : no samples"; return }
    $cpu = $Samples | Measure-Object CpuPct -Average -Maximum
    $ws = $Samples | Measure-Object WsMb -Average -Maximum
    Write-Host ("{0,-12} CPU avg {1,6:N2}%  peak {2,6:N2}%   WS avg {3,7:N1} MB  peak {4,7:N1} MB" -f `
        $Label, $cpu.Average, $cpu.Maximum, $ws.Average, $ws.Maximum)
}

Write-Host "`n=== $Seconds second sample ($cores cores) ==="
Report 'Tauri(old)' $oldSamples
Report 'Rune(new)'  $newSamples
