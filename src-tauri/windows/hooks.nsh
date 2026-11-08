; FrameWise NSIS installer hooks (Tauri bundle.windows.nsis.installerHooks).
;
; The game-profile watcher (framewise.exe --watcher) keeps running after the window closes, so it
; has to stop before files are replaced or removed. `--stop-watcher` asks it to put back any active
; profile and exit, and waits up to 5 seconds.

!macro NSIS_HOOK_PREINSTALL
  ; Updating over an existing install: stop the old watcher so its exe isn't locked.
  IfFileExists "$INSTDIR\framewise.exe" 0 +2
    ExecWait '"$INSTDIR\framewise.exe" --stop-watcher'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  IfFileExists "$INSTDIR\framewise.exe" 0 +2
    ExecWait '"$INSTDIR\framewise.exe" --uninstall-cleanup'
  ; Its start-with-Windows entry (written per user when profiles are turned on).
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "FrameWise watcher"
!macroend
