; NSIS installer hooks for Coding Agent Monitor.
;
; v0.3 removed the external ccusage sidecar supply chain: the installer only
; ever places the single product executable. Machines upgrading from v0.2 keep
; no orphaned sidecar binaries: the post-install hook deletes every sidecar
; name any earlier version could have staged next to the product EXE, and the
; post-uninstall hook does the same so an uninstall of an upgraded install
; never leaves them behind.

!macro _camDeleteStaleSidecars
  DetailPrint "Removing leftover ccusage sidecar binaries from previous versions (if any)"
  Delete "$INSTDIR\ccusage.exe"
  Delete "$INSTDIR\ccusage-antigravity.exe"
  Delete "$INSTDIR\ccusage-x86_64-pc-windows-msvc.exe"
  Delete "$INSTDIR\ccusage-antigravity-x86_64-pc-windows-msvc.exe"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro _camDeleteStaleSidecars
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro _camDeleteStaleSidecars
!macroend
