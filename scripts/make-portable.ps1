# Builds the portable zip from a release build: FrameWise without an installer and without the
# game-profile watcher (nothing runs in the background or starts with Windows). Run after
# `npx tauri build`. Output: target\release\bundle\portable\FrameWise_<version>_x64_portable.zip
param([string]$Root = (Split-Path $PSScriptRoot -Parent))
$ErrorActionPreference = 'Stop'
$rel = Join-Path $Root 'target\release'
$version = (Get-Content (Join-Path $Root 'src-tauri\tauri.conf.json') -Raw | ConvertFrom-Json).version
$stage = Join-Path $rel 'bundle\portable\FrameWise'
$zip = Join-Path $rel "bundle\portable\FrameWise_${version}_x64_portable.zip"
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Force $stage | Out-Null
foreach ($f in 'framewise.exe', 'presentmon.exe') {
    $src = Join-Path $rel $f
    if (-not (Test-Path $src)) { throw "Missing $src. Run 'npx tauri build' first." }
    Copy-Item $src $stage
}
Copy-Item (Join-Path $Root 'vendor\presentmon\LICENSE.txt') (Join-Path $stage 'presentmon-LICENSE.txt')
Copy-Item (Join-Path $Root 'LICENSE') (Join-Path $stage 'LICENSE.txt')
# The marker: FrameWise turns off the watcher and self-updates when it finds this next to the exe.
Set-Content -Path (Join-Path $stage 'framewise.portable') -Value 'Portable build: no background watcher, no automatic updates.' -Encoding ascii
Set-Content -Path (Join-Path $stage 'README.txt') -Encoding utf8 -Value @"
FrameWise $version (portable)

Run framewise.exe. Nothing is installed: delete this folder to remove FrameWise.
Your scans, history and benchmarks are kept in %LOCALAPPDATA%\FrameWise.

The portable build has no game profiles (they need a small background watcher) and doesn't update
itself: download the newest zip from https://github.com/danieladekunle123/FrameWise/releases.
Fixes that need admin rights still ask through Windows' UAC prompt, once per task.
"@
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip
Write-Output $zip
