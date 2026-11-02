<#
.SYNOPSIS
  Phase 0 benchmark matrix for the spike (release build). Results go to ..\out\summary.tsv,
  ..\out\startup.tsv and ..\out\idle-external.tsv. See docs/PERFORMANCE.md for the method.

.EXAMPLE
  .\scripts\measure.ps1 -Exe .\target\release\sx-spike-phase0.exe -Tag igpu-165hz-upstream
#>
param(
    [Parameter(Mandatory)][string]$Exe,
    [Parameter(Mandatory)][string]$Tag,
    [int]$StartupRuns = 5,
    [int]$IdleRuns = 3,
    [int]$ScrollRuns = 1,
    [switch]$SkipScroll
)
$ErrorActionPreference = 'Stop'
$Exe = (Resolve-Path $Exe).Path
$out = Join-Path $PSScriptRoot '..\out'
New-Item -ItemType Directory -Force $out | Out-Null
$name = [IO.Path]::GetFileNameWithoutExtension($Exe)

function Run-Bench([string[]]$argList) {
    $p = Start-Process -FilePath $Exe -ArgumentList $argList -PassThru
    if (-not $p.WaitForExit(90000)) { $p.Kill(); Write-Warning "timeout: $($argList -join ' ')" }
}

function Private-WS([int]$processId) {
    # Same figure as Task Manager's "Memory (active private working set)". Uses the WMI perf
    # class because counter paths like "\Process(*)\..." are localized on non-English Windows.
    $row = Get-CimInstance Win32_PerfFormattedData_PerfProc_Process -Filter "IDProcess=$processId" -ErrorAction SilentlyContinue
    if (-not $row) { return [double]::NaN }
    [double]$row.WorkingSetPrivate / 1MB
}

Write-Host "== startup x$StartupRuns"
for ($i = 0; $i -lt $StartupRuns; $i++) { Run-Bench @('--bench', 'startup', '--tag', $Tag); Start-Sleep -Milliseconds 800 }

Write-Host "== idle x$IdleRuns (external sampling of private working set and CPU)"
$idleFile = Join-Path $out 'idle-external.tsv'
if (-not (Test-Path $idleFile)) { "tag`trun`tprivate_ws_mb_median`tcpu_ms_over_10s" | Out-File $idleFile -Encoding utf8 }
for ($i = 0; $i -lt $IdleRuns; $i++) {
    $p = Start-Process -FilePath $Exe -ArgumentList @('--bench', 'idle', '--seconds', '20', '--tag', $Tag) -PassThru
    Start-Sleep -Seconds 10
    $p.Refresh(); $cpu0 = $p.TotalProcessorTime
    $ws = for ($s = 0; $s -lt 10; $s++) { Private-WS $p.Id; Start-Sleep -Seconds 1 }
    $p.Refresh(); $cpu1 = $p.TotalProcessorTime
    $median = ($ws | Sort-Object)[[int]($ws.Count / 2)]
    "{0}`t{1}`t{2:N1}`t{3:N0}" -f $Tag, $i, $median, ($cpu1 - $cpu0).TotalMilliseconds | Out-File $idleFile -Append -Encoding utf8
    [void]$p.WaitForExit(30000)
}

if (-not $SkipScroll) {
    $matrix = @(
        @('datatable', 'scroll-slow'), @('datatable', 'scroll-fast'), @('datatable', 'scroll-jump'),
        @('uniform', 'scroll-slow'), @('uniform', 'scroll-fast'), @('uniform', 'scroll-jump'), @('uniform', 'scroll-pixel')
    )
    foreach ($run in 1..$ScrollRuns) {
        foreach ($m in $matrix) {
            Write-Host "== $($m[1]) on $($m[0]) (run $run)"
            Run-Bench @('--bench', $m[1], '--table', $m[0], '--seconds', '10', '--tag', $Tag)
            Start-Sleep -Milliseconds 800
        }
    }
}
Write-Host "done → $out"
