# Privacy policy

FrameWise has no account, no telemetry and no analytics. Everything it records stays on your PC.

## What's stored on your PC

- `%LOCALAPPDATA%\FrameWise\framewise.db` (SQLite) holds your scans, the changes you made and how to
  undo them, benchmarks, game profiles and settings. The portable version uses the same folder.
- Benchmark frame-time files are saved next to the database.
- If FrameWise crashes, a short report is saved in a `crashes` folder next to the database. It holds the
  error message, the FrameWise and Windows versions, and where in FrameWise's own code it happened.
  File paths and your Windows user name are removed before it's saved. It never includes a list of your
  programs or processes.

Uninstalling FrameWise doesn't delete this folder, so your history survives a reinstall. Delete the
folder to remove it.

## What goes over the network

FrameWise connects to the network for three things only:

| What | When | What's sent |
| --- | --- | --- |
| **Update check** | When FrameWise starts. Turn it off in **Settings > Updates**. | A download request for `latest.json` from this repository's GitHub releases. Updates are only downloaded when you click **Install and restart**. GitHub sees your IP address, as with any download. See [GitHub's privacy statement](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement). |
| **Ping check** | When you run a scan. | A TCP handshake to `1.1.1.1` (or `8.8.8.8` if that fails), timed to measure your connection's latency. No data is sent. |
| **Reports you choose to send** | Only when you click **Open on GitHub** on a crash report or a watcher over-budget report. | FrameWise shows you the exact text first, then opens a pre-filled GitHub issue in your browser. Nothing is sent unless you submit that issue yourself, from your own GitHub account. |

Links in the app, such as driver download pages and Microsoft documentation, open in your browser only
when you click them.

## Third-party components

- **Intel PresentMon** measures frame times while the monitor is on. It runs locally and sends nothing.
- **NVIDIA's NVML library**, part of the NVIDIA driver, is read locally for GPU temperature and clocks.
- **Microsoft Edge WebView2** displays FrameWise's window. It's part of Windows and follows
  [Microsoft's privacy statement](https://privacy.microsoft.com/privacystatement). FrameWise loads no web
  content into it, only its own interface.

## Changes

Any change to this policy is recorded in this file's history on GitHub.
