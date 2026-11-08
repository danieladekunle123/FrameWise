# Phase 2 tests inside a disposable Windows test VM. Copy the tools to C:\FrameWise\tools first and
# run elevated. Phases:
#   main   read-only probe and scan, every round trip with System Restore off and then on,
#          Cleanup, then the first half of the restart test
#   after  (after restarting the VM) the second half of the restart test
# Results go to C:\FrameWise\vm-results-<phase>.txt.
param([string]$Phase = 'main')
$ErrorActionPreference = 'Continue'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$t = 'C:\FrameWise\tools'
$out = "C:\FrameWise\vm-results-$Phase.txt"
$lines = New-Object System.Collections.Generic.List[string]
function Say([string]$s) { $lines.Add($s) }
function Run([string]$exe, [string[]]$a) {
    Say "--- $exe $($a -join ' ')"
    & (Join-Path $t $exe) @a 2>&1 | ForEach-Object { Say "$_" }
    Say "exit $LASTEXITCODE"
}
function Points {
    try {
        $p = Get-ComputerRestorePoint -ErrorAction Stop
        if (-not $p) { Say '  (none)' }
        $p | ForEach-Object { Say ("  #{0}  {1}  {2}" -f $_.SequenceNumber, $_.ConvertToDateTime($_.CreationTime).ToString('yyyy-MM-dd HH:mm:ss'), $_.Description) }
    } catch { Say "  can't list: $($_.Exception.Message)" }
}

$os = Get-CimInstance Win32_OperatingSystem
Say "FrameWise VM tests ($Phase) · $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss') · $($os.Caption) build $($os.BuildNumber) · user $env:USERNAME"

if ($Phase -eq 'main') {
    Run 'probe.exe' @()
    Run 'scan.exe' @()
    Say ''
    Say '== Round trips, System Restore off (Windows default in a new VM) =='
    Run 'roundtrip.exe' @('--admin', '--display')
    Say ''
    Say '== Round trips, System Restore on =='
    try { Enable-ComputerRestore -Drive 'C:\' -ErrorAction Stop; Say 'System Restore turned on for C:' } catch { Say "Enable-ComputerRestore failed: $($_.Exception.Message)" }
    Run 'roundtrip.exe' @('--admin')
    Say 'Restore points:'
    Points
    Say ''
    Say '== Cleanup =='
    Run 'cleanup_check.exe' @('--clean')
    Say ''
    Say '== Restart test, part 1 =='
    Run 'restart_check.exe' @('before')
} else {
    Say '== Restart test, part 2 =='
    Run 'restart_check.exe' @('after')
}
[IO.File]::WriteAllLines($out, $lines, (New-Object Text.UTF8Encoding $false))
