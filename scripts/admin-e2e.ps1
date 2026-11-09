# Runs the end-to-end tests as admin, after one UAC prompt, so the tests that need admin rights run
# too: frame capture (the benchmark of the test scene) and clearing the standby list. Writes the
# results to scripts\admin-e2e-results.txt. Build first:
#   npx tauri build --no-bundle
#   cargo build --release -p framewise-checks --example test_scene
# Then:
#   powershell -ExecutionPolicy Bypass -File scripts\admin-e2e.ps1
#
# What it changes: Game Mode is turned off for the run (so the safe-fix test has something to fix)
# and put back to what it was afterwards. The standby list (Windows' cache of recently used files)
# is emptied once. The tests use a throwaway data folder, so your own FrameWise data isn't touched.
# WebView2 ignores WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS in elevated apps, so the window's debug port is
# set through WebView2's machine-wide per-app policy for framewise.exe during the run, and removed afterwards.

param([string]$Out)
$ErrorActionPreference = 'Continue'
$root = Split-Path $PSScriptRoot -Parent
if (-not $Out) { $Out = Join-Path $PSScriptRoot 'admin-e2e-results.txt' }

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

Set-Location $root
$k = 'HKCU:\Software\Microsoft\GameBar'
$before = (Get-ItemProperty $k -Name AutoGameModeEnabled -ErrorAction SilentlyContinue).AutoGameModeEnabled
# Elevated apps read the machine-wide policy (HKLM), not the per-user one.
$pol = 'HKLM:\SOFTWARE\Policies\Microsoft\Edge\WebView2\AdditionalBrowserArguments'
$polBefore = (Get-ItemProperty $pol -Name 'framewise.exe' -ErrorAction SilentlyContinue).'framewise.exe'
try {
    New-Item $pol -Force -ErrorAction SilentlyContinue | Out-Null
    Set-ItemProperty $pol -Name 'framewise.exe' -Value '--remote-debugging-port=9555' -Type String
    New-Item $k -Force -ErrorAction SilentlyContinue | Out-Null
    Set-ItemProperty $k -Name AutoGameModeEnabled -Value 0 -Type DWord
    Get-Process framewise -ErrorAction SilentlyContinue | Stop-Process -Force -Confirm:$false -ErrorAction SilentlyContinue
    $env:NO_COLOR = '1'
    $log = & npx.cmd playwright test --reporter=list 2>&1 | ForEach-Object { "$_" }
    $code = $LASTEXITCODE
} finally {
    if ($null -eq $polBefore) { Remove-ItemProperty $pol -Name 'framewise.exe' -ErrorAction SilentlyContinue } else { Set-ItemProperty $pol -Name 'framewise.exe' -Value $polBefore }
    if ($null -eq $before) { Remove-ItemProperty $k -Name AutoGameModeEnabled -ErrorAction SilentlyContinue }
    else { Set-ItemProperty $k -Name AutoGameModeEnabled -Value $before -Type DWord }
}
$after = (Get-ItemProperty $k -Name AutoGameModeEnabled -ErrorAction SilentlyContinue).AutoGameModeEnabled
$lines = @("Admin end-to-end tests - $(Get-Date -Format s)", "") + $log + @("", "Exit code: $code",
    "Game Mode setting: before $(if ($null -eq $before) { 'not set' } else { $before }), after $(if ($null -eq $after) { 'not set' } else { $after })")
$lines | Set-Content -Encoding UTF8 $Out
