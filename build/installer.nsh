; Custom NSIS macros for electron-builder (Windows).
; Downloads the toolchain DURING installation (not on first app launch).

!macro customInstall
  DetailPrint "EmbedIDE: downloading compilers (ARM GCC, OpenOCD, Zig, Rust)…"
  CreateDirectory "$APPDATA\embed-ide\toolchain"
  IfFileExists "$INSTDIR\resources\install-toolchain.ps1" 0 skip_tc
    nsExec::ExecToLog 'powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$INSTDIR\resources\install-toolchain.ps1" -Dest "$APPDATA\embed-ide\toolchain"'
    Pop $0
    DetailPrint "Toolchain installer exit code: $0"
    Goto done_tc
  skip_tc:
    DetailPrint "WARNING: install-toolchain.ps1 missing — use Settings → Toolchain after launch"
  done_tc:
!macroend
