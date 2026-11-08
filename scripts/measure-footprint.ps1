<#
.SYNOPSIS
  Launches FrameWise, lets it settle, and reports the private working set (Task Manager's "Memory")
  of the app and every process it starts, by role. Used to check the 100 MB interface budget.

.EXAMPLE
  ./scripts/measure-footprint.ps1
  ./scripts/measure-footprint.ps1 -Target low -Runs 3
  ./scripts/measure-footprint.ps1 -BrowserArgs "--in-process-gpu" -Runs 2
#>
param(
  [string]$Exe = "$PSScriptRoot\..\target\release\framewise.exe",
  # Optional WebView2 arguments for experiments (overrides the app's own via the documented env var).
  [string]$BrowserArgs = "",
  [int]$SettleSeconds = 12,
  [int]$Runs = 1,
  [double]$BudgetMb = 100,
  # WebView2 memory target to pin: "normal" (window focused) or "low" (FrameWise behind a game or
  # minimised). Pinned through FRAMEWISE_MEMORY_TARGET because automation can't reliably move focus.
  [ValidateSet("normal", "low", "auto")]
  [string]$Target = "normal"
)

function Get-Tree([int]$RootId) {
  $all = Get-CimInstance Win32_Process
  $ids = @($RootId)
  do {
    $n = $ids.Count
    $ids = @($ids + ($all | Where-Object { $ids -contains $_.ParentProcessId } | ForEach-Object ProcessId) | Select-Object -Unique)
  } while ($ids.Count -gt $n)
  $all | Where-Object { $ids -contains $_.ProcessId }
}

function Get-Role($p) {
  if ($p.Name -notmatch 'msedgewebview2') { return 'app core' }
  if ($p.CommandLine -match '--type=([\w-]+)') {
    $t = $Matches[1]
    if ($t -eq 'utility' -and $p.CommandLine -match '--utility-sub-type=([\w.]+)') { return "utility ($($Matches[1].Split('.')[-1]))" }
    return $t
  }
  return 'browser'
}

$totals = @()
for ($r = 1; $r -le $Runs; $r++) {
  if ($BrowserArgs) { $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = $BrowserArgs } else { Remove-Item Env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS -ErrorAction SilentlyContinue }
  if ($Target -ne "auto") { $env:FRAMEWISE_MEMORY_TARGET = $Target } else { Remove-Item Env:FRAMEWISE_MEMORY_TARGET -ErrorAction SilentlyContinue }
  $proc = Start-Process -FilePath $Exe -PassThru
  Start-Sleep -Seconds $SettleSeconds

  # Three samples a second apart, averaged, to smooth out momentary spikes.
  $samples = @{}
  $tree = @()
  for ($s = 0; $s -lt 3; $s++) {
    $tree = Get-Tree $proc.Id
    foreach ($p in $tree) {
      $perf = Get-CimInstance Win32_PerfFormattedData_PerfProc_Process -Filter "IDProcess=$($p.ProcessId)" -ErrorAction SilentlyContinue
      if ($perf) { $samples[$p.ProcessId] += @([double]$perf.WorkingSetPrivate / 1MB) }
    }
    Start-Sleep -Seconds 1
  }

  $rows = foreach ($p in $tree) {
    $v = $samples[$p.ProcessId]
    if ($v) { [pscustomobject]@{ Role = Get-Role $p; Pid = $p.ProcessId; MB = [math]::Round(($v | Measure-Object -Average).Average, 1) } }
  }
  $total = [math]::Round(($rows | Measure-Object MB -Sum).Sum, 1)
  $totals += $total
  Write-Output "--- run $r - memory target $Target$(if ($BrowserArgs) { "  [$BrowserArgs]" })"
  $rows | Sort-Object MB -Descending | Format-Table -AutoSize | Out-String -Width 160 | Write-Output
  Write-Output ("TOTAL {0} MB across {1} processes (budget {2} MB)" -f $total, @($rows).Count, $BudgetMb)

  # Close the whole tree so the next run starts clean (WebView2 refuses to reuse a profile with different arguments).
  $tree | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
  Start-Sleep -Seconds 2
}
Remove-Item Env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS -ErrorAction SilentlyContinue
Remove-Item Env:FRAMEWISE_MEMORY_TARGET -ErrorAction SilentlyContinue
if ($Runs -gt 1) { Write-Output ("AVERAGE {0:N1} MB over {1} runs" -f (($totals | Measure-Object -Average).Average), $Runs) }
