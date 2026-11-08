# Release checks inside a disposable Windows test VM (run elevated, release files in C:\fwrelease):
# NSIS and MSI install, app start, watcher start/stop, uninstall clean-up, portable zip.
# Writes C:\fwrelease\release-results.txt.
$ErrorActionPreference = 'Continue'
$out = 'C:\fwrelease\release-results.txt'
$lines = New-Object System.Collections.Generic.List[string]
function Say([string]$s) { $lines.Add($s) }
$dir = "$env:ProgramFiles\FrameWise"
$run = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
if (-not (Test-Path $run)) { New-Item -Path $run -Force | Out-Null }
function Watchers { @(Get-CimInstance Win32_Process -Filter "Name='framewise.exe'" | Where-Object { $_.CommandLine -like '*--watcher*' }).Count }
function Files { if (Test-Path $dir) { (Get-ChildItem $dir -Recurse -File | ForEach-Object { $_.FullName.Replace("$dir\", '') }) -join ', ' } else { '(folder gone)' } }
function AppStart {
    $p = Start-Process "$dir\framewise.exe" -PassThru
    Start-Sleep 8
    $alive = -not $p.HasExited
    $kids = @(Get-CimInstance Win32_Process -Filter "ParentProcessId=$($p.Id)").Count
    $p.Refresh()
    $title = $p.MainWindowTitle
    if ($alive) { Stop-Process -Id $p.Id -Force }
    "app running after 8 s: $alive · child processes (WebView2): $kids · window title: '$title'"
}
function WaitGone([scriptblock]$cond, [int]$secs = 60) { for ($i = 0; $i -lt $secs; $i++) { if (& $cond) { return $true }; Start-Sleep 1 }; $false }

Say "Release tests · $(Get-Date -Format s) · $((Get-CimInstance Win32_OperatingSystem).Caption) $((Get-CimInstance Win32_OperatingSystem).BuildNumber)"

# ---------------- NSIS
Say ''; Say '== NSIS setup (silent, per machine) =='
$p = Start-Process 'C:\fwrelease\FrameWise_0.4.0_x64-setup.exe' -ArgumentList '/S' -PassThru -Wait
Say "setup exit code: $($p.ExitCode)"
Say "installed files: $(Files)"
$u = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*' -ErrorAction SilentlyContinue | Where-Object { $_.DisplayName -eq 'FrameWise' } | Select-Object -First 1
Say "uninstall entry: $($u.DisplayName) $($u.DisplayVersion) · publisher '$($u.Publisher)' · in Program Files: $($u.UninstallString -like "*Program Files*")"
Say (AppStart)
Start-Process "$dir\framewise.exe" -ArgumentList '--watcher'
Start-Sleep 3
Set-ItemProperty $run -Name 'FrameWise watcher' -Value "`"$dir\framewise.exe`" --watcher"
Say "watcher running: $(Watchers) · Run entry set: $([bool](Get-ItemProperty $run -Name 'FrameWise watcher' -ErrorAction SilentlyContinue))"
$p = Start-Process "$dir\framewise.exe" -ArgumentList '--stop-watcher' -PassThru -Wait
Say "--stop-watcher exit code $($p.ExitCode) · watchers left: $(Watchers)"
Start-Process "$dir\framewise.exe" -ArgumentList '--watcher'
Start-Sleep 3
Say "watcher started again for the uninstall test: $(Watchers)"
Start-Process "$dir\uninstall.exe" -ArgumentList '/S' -Wait
$gone = WaitGone { -not (Test-Path "$dir\framewise.exe") }
Say "after uninstall: exe removed: $gone · files: $(Files) · watchers left: $(Watchers) · Run entry left: $([bool](Get-ItemProperty $run -Name 'FrameWise watcher' -ErrorAction SilentlyContinue))"

# ---------------- MSI
Say ''; Say '== MSI (silent) =='
$p = Start-Process msiexec.exe -ArgumentList '/i', 'C:\fwrelease\FrameWise_0.4.0_x64_en-US.msi', '/qn', '/norestart', '/l*v', 'C:\fwrelease\msi-install.log' -PassThru -Wait
Say "msiexec install exit code: $($p.ExitCode)"
Say "installed files: $(Files)"
Say (AppStart)
Start-Process "$dir\framewise.exe" -ArgumentList '--watcher'
Start-Sleep 3
Set-ItemProperty $run -Name 'FrameWise watcher' -Value "`"$dir\framewise.exe`" --watcher"
Say "watcher running before uninstall: $(Watchers)"
$p = Start-Process msiexec.exe -ArgumentList '/x', 'C:\fwrelease\FrameWise_0.4.0_x64_en-US.msi', '/qn', '/norestart', '/l*v', 'C:\fwrelease\msi-uninstall.log' -PassThru -Wait
Say "msiexec uninstall exit code: $($p.ExitCode) (3010 = finishes after a restart)"
Say "after uninstall: files: $(Files) · watchers left: $(Watchers) · Run entry left: $([bool](Get-ItemProperty $run -Name 'FrameWise watcher' -ErrorAction SilentlyContinue))"
Get-CimInstance Win32_Process -Filter "Name='framewise.exe'" | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
Remove-ItemProperty $run -Name 'FrameWise watcher' -ErrorAction SilentlyContinue

# ---------------- Portable
Say ''; Say '== Portable zip =='
$pd = 'C:\fwportable'
if (Test-Path $pd) { Remove-Item $pd -Recurse -Force }
Expand-Archive 'C:\fwrelease\FrameWise_0.4.0_x64_portable.zip' -DestinationPath $pd
Say "zip contents: $((Get-ChildItem $pd | ForEach-Object Name) -join ', ')"
$p = Start-Process "$pd\framewise.exe" -PassThru
Start-Sleep 8
Say "portable app running after 8 s: $(-not $p.HasExited) · registry/startup entries added: $([bool](Get-ItemProperty $run -Name 'FrameWise watcher' -ErrorAction SilentlyContinue))"
if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force }

[IO.File]::WriteAllLines($out, $lines, (New-Object Text.UTF8Encoding $false))
