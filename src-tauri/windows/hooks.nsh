!macro NSIS_HOOK_PREUNINSTALL
  DeleteRegValue HKCU "SOFTWARE\Microsoft\Windows\CurrentVersion\Run" "Keyboard Lock OSD"
  DeleteRegValue HKCU "SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "Keyboard Lock OSD"
!macroend
