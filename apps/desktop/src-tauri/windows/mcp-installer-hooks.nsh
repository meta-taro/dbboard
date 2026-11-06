; Installer hooks for the MCP server the Windows installer carries (ADR-0169).
;
; An AI agent keeps dbboard-mcp.exe running for as long as it is open, and
; Windows refuses to overwrite a running executable. It does allow renaming
; one, so the old server is moved aside: the agent goes on using it until it
; restarts, and the next start picks up the new one. Without this an open
; agent would fail the update.

!macro NSIS_HOOK_PREINSTALL
  ; Ask about (or, in passive mode, close) a running app first, as the
  ; template does next. Renaming before that prompt would leave the install
  ; without a server if the person cancels it.
  !insertmacro CheckIfAppIsRunning "${MAINBINARYNAME}.exe" "${PRODUCTNAME}"

  ; Servers moved aside by earlier installs. One still running stays, and is
  ; tried again next time.
  Delete "$INSTDIR\dbboard-mcp.replaced-*.exe"
  IfFileExists "$INSTDIR\dbboard-mcp.exe" 0 +2
    Rename "$INSTDIR\dbboard-mcp.exe" "$INSTDIR\dbboard-mcp.replaced-${VERSION}.exe"
  ClearErrors
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  Delete "$INSTDIR\dbboard-mcp.replaced-*.exe"
!macroend
