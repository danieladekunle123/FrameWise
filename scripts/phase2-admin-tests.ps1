# Runs every phase 2 check that needs admin rights, after one UAC prompt, and writes the results to
# scripts\phase2-admin-results.txt. Build the test tools first:
#   cargo build --release -p framewise-checks --example roundtrip --example cleanup_check
# Then:
#   powershell -ExecutionPolicy Bypass -File scripts\phase2-admin-tests.ps1
#
# What it changes: GPU scheduling, Memory Integrity, Game Mode, windowed optimisations, power plan and
# mode and two startup entries are each changed and put straight back (no restart, so nothing takes
# effect). It may create one Windows restore point. Cleanup permanently deletes Windows Temp files
# older than a day, Windows crash dumps and error reports, and Windows Update downloads older than
# 10 days. Driver packages are only listed.

param([string]$Out)
$ErrorActionPreference = 'Continue'
$root = Split-Path $PSScriptRoot -Parent
if (-not $Out) { $Out = Join-Path $PSScriptRoot 'phase2-admin-results.txt' }

$me = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
if (-not $me.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    if (Test-Path $Out) { Remove-Item $Out -Force -Confirm:$false }
    try {
        Start-Process powershell.exe -Verb RunAs -Wait -ArgumentList @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$PSCommandPath`"", '-Out', "`"$Out`"") -ErrorAction Stop
    } catch {
        Write-Output "Not run: $($_.Exception.Message)"
        exit 2
    }
    if (Test-Path $Out) { Get-Content $Out -Encoding UTF8 } else { Write-Output 'No results file was written.' }
    exit
}

[Console]::OutputEncoding = [Text.Encoding]::UTF8
$roundtrip = Join-Path $root 'target\release\examples\roundtrip.exe'
$cleanup = Join-Path $root 'target\release\examples\cleanup_check.exe'
$lines = New-Object System.Collections.Generic.List[string]
function Say([string]$s) { $lines.Add($s) }
function Points {
    try {
        Get-ComputerRestorePoint -ErrorAction Stop | Sort-Object SequenceNumber | Select-Object -Last 4 | ForEach-Object {
            Say ("  #{0}  {1}  {2}" -f $_.SequenceNumber, $_.ConvertToDateTime($_.CreationTime).ToString('yyyy-MM-dd HH:mm:ss'), $_.Description)
        }
    } catch { Say "  can't list restore points: $($_.Exception.Message)" }
}

$os = Get-CimInstance Win32_OperatingSystem
Say "FrameWise phase 2 admin checks · $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')"
Say "$($os.Caption) build $($os.BuildNumber)"
$sr = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\SystemRestore' -ErrorAction SilentlyContinue
Say "SystemRestorePointCreationFrequency: $(if ($null -ne $sr.SystemRestorePointCreationFrequency) { $sr.SystemRestorePointCreationFrequency } else { 'not set (24-hour limit)' })"
Say ''
Say 'Restore points before:'
Points
Say ''
Say '== Fix round trips (roundtrip --admin) =='
if (Test-Path $roundtrip) { & $roundtrip --admin 2>&1 | ForEach-Object { Say "$_" }; Say "exit $LASTEXITCODE" } else { Say "missing $roundtrip" }
Say ''
Say 'Restore points after:'
Points
Say ''
Say '== Cleanup (cleanup_check --clean) =='
if (Test-Path $cleanup) { & $cleanup --clean 2>&1 | ForEach-Object { Say "$_" }; Say "exit $LASTEXITCODE" } else { Say "missing $cleanup" }
[IO.File]::WriteAllLines($Out, $lines, (New-Object Text.UTF8Encoding $false))
