# Releasing FrameWise

## What a release contains

| File | For |
| --- | --- |
| `FrameWise_<version>_x64-setup.exe` | The recommended installer (NSIS). Installs for all users into Program Files. |
| `FrameWise_<version>_x64_en-US.msi` | The same app as an MSI, for people and IT tools that prefer one. |
| `FrameWise_<version>_x64_portable.zip` | No installer, no game profiles, no automatic updates. |
| `latest.json` and `*.sig` | The signed update description the app checks on launch. |

Both installers install **per machine, into Program Files**. That matters for security: FrameWise
starts its own exe with admin rights for machine-wide fixes, so the exe must sit where only
administrators can change it. Both installers also stop the game-profile watcher before replacing
files, and on uninstall stop it and remove its start-with-Windows entry
(`src-tauri/windows/hooks.nsh` for NSIS, `src-tauri/windows/watcher.wxs` for the MSI).

## One-time setup

1. **Back up the updater signing key.** It was generated into `%USERPROFILE%\.tauri\`:
   `framewise-updater.key` (private), `framewise-updater.key.password` and
   `framewise-updater.key.pub`. The public key is built into the app (`tauri.conf.json` >
   `plugins.updater.pubkey`). If the private key or its password is lost, installed copies can
   never accept another update. Never commit either file.
2. **Add two repository secrets** (GitHub > Settings > Secrets and variables > Actions), or with the
   GitHub CLI **from Command Prompt** (`cmd`):

   ```bat
   gh secret set TAURI_SIGNING_PRIVATE_KEY < "%USERPROFILE%\.tauri\framewise-updater.key"
   gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD < "%USERPROFILE%\.tauri\framewise-updater.key.password"
   ```

   Don't pipe the files in from Windows PowerShell 5 (`Get-Content ... | gh secret set`): it adds
   an invisible byte-order mark, and the release build then fails with "failed to decode base64
   secret key: Invalid symbol 239".

3. **Updates need a public repository.** The app downloads
   `https://github.com/danieladekunle123/FrameWise/releases/latest/download/latest.json`.
   GitHub only serves release files to people who aren't signed in when the repository is public.
   While it's private, the update check fails quietly and people update by downloading the new
   installer.

## Cutting a release

1. Bump the version in `package.json`, `src-tauri/tauri.conf.json` and the workspace `Cargo.toml`
   (and the browser-preview string in `src/mock.ts`).
2. Commit, then tag and push:

   ```bash
   git tag v0.4.0
   git push origin main v0.4.0
   ```

3. The **Release** workflow (`.github/workflows/release.yml`) runs the tests, checks that the tag
   matches the app version, builds both installers and the update files, adds the portable zip,
   and creates a **draft** release.
4. Review the draft on GitHub (download and try the installer), edit the notes, then click
   **Publish**. Installed copies offer the update the next time they start.

To build everything locally instead:

```bash
set TAURI_SIGNING_PRIVATE_KEY=<contents of framewise-updater.key>
set TAURI_SIGNING_PRIVATE_KEY_PASSWORD=<contents of framewise-updater.key.password>
npx tauri build
powershell -File scripts/make-portable.ps1
```

`scripts/release-tests.ps1` checks the installers inside a disposable Windows VM: silent install
and uninstall of both, the watcher stopping, the startup entry being removed, and the portable zip.

## Code signing (not set up yet)

Without a code-signing certificate, Windows treats FrameWise as from an unknown publisher:

- SmartScreen shows "Windows protected your PC" on the installer until enough people have run it.
- The UAC prompt for machine-wide fixes says **"Publisher: Unknown"** in yellow, instead of your
  name.
- Some antivirus products are quicker to flag unsigned tools that ask for admin rights.

The options, roughly from cheapest to most expensive (check current prices and eligibility):

| Option | Notes |
| --- | --- |
| **SignPath Foundation** | Free signing for open-source projects. Requires a public repository with an OSI licence (MIT qualifies) and their review. |
| **Certum Open Source Code Signing** | A low-cost certificate for individual open-source developers, issued in your own name. Uses a smart card or their cloud signing. |
| **Microsoft Artifact Signing** (formerly Trusted Signing) | A monthly subscription with signing from GitHub Actions. Identity validation and eligibility depend on where you live and whether you sign as an individual or a business. |
| **A standard OV certificate** | Yearly, from a certificate authority. Since 2023 these come on a hardware token or cloud HSM. |

Once there's a certificate, Tauri signs both installers and the exe during `tauri build`, through
`bundle.windows.certificateThumbprint` (a certificate in the Windows store) or
`bundle.windows.signCommand` (a cloud or HSM signing tool). Add the matching secrets to the
release workflow.
