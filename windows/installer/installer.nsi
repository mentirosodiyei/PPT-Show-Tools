; PPT Show Tools for Windows: per-user installer (no administrator rights needed).
Unicode true
!include "MUI2.nsh"

!ifndef VERSION
  !define VERSION "0.9.0"
!endif
!ifndef EXE
  !define EXE "PPTShowTools.exe"
!endif
!ifndef OUTFILE
  !define OUTFILE "PPTShowTools-Setup-${VERSION}.exe"
!endif

Name "PPT Show Tools"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\PPT Show Tools"
InstallDirRegKey HKCU "Software\PPT Show Tools" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
BrandingText "PPT Show Tools ${VERSION}"

VIProductVersion "${VERSION}.0"
VIAddVersionKey /LANG=1033 "ProductName" "PPT Show Tools"
VIAddVersionKey /LANG=1033 "FileDescription" "PPT Show Tools Setup"
VIAddVersionKey /LANG=1033 "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=1033 "ProductVersion" "${VERSION}"
VIAddVersionKey /LANG=1033 "CompanyName" "David Redondo García"
VIAddVersionKey /LANG=1033 "LegalCopyright" "© 2026 David Redondo García · github.com/mentirosodiyei/PPT-Show-Tools"

!define MUI_ICON "..\assets\app.ico"
!define MUI_UNICON "..\assets\app.ico"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\PPTShowTools.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Open PPT Show Tools"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

; This app was called Deckwright up to 0.8.0. Keep its settings and remove it,
; so there are not two copies installed.
Section "-Replace Deckwright"
  SetShellVarContext current
  IfFileExists "$APPDATA\Deckwright\settings.json" 0 +4
  IfFileExists "$APPDATA\PPT Show Tools\settings.json" +3 0
  CreateDirectory "$APPDATA\PPT Show Tools"
  CopyFiles /SILENT "$APPDATA\Deckwright\settings.json" "$APPDATA\PPT Show Tools\settings.json"
  ReadRegStr $R1 HKCU "Software\Deckwright" "InstallDir"
  StrCmp $R1 "" done
  IfFileExists "$R1\Uninstall Deckwright.exe" 0 done
  ExecWait '"$R1\Uninstall Deckwright.exe" /S _?=$R1'
  Delete "$R1\Uninstall Deckwright.exe"
  RMDir "$R1"
  done:
SectionEnd

Section "PPT Show Tools" SecMain
  SectionIn RO
  SetShellVarContext current
  SetOutPath "$INSTDIR"
  File "/oname=PPTShowTools.exe" "${EXE}"
  File "/oname=PPTShowTools.ico" "..\assets\app.ico"
  ; FFmpeg (LGPL) for the blurred video backgrounds; see FFMPEG-NOTICE.txt.
  File "/oname=ffmpeg.exe" "${FFMPEG}\ffmpeg.exe"
  File "/oname=FFMPEG-LICENSE.txt" "${FFMPEG}\LICENSE.txt"
  File "/oname=FFMPEG-NOTICE.txt" "${FFMPEG}\FFMPEG-NOTICE.txt"
  ; Marks this copy as installed: reports and logs go to %LOCALAPPDATA%\PPT Show Tools\Logs
  ; (deleted after 30 days). The portable zip has no marker and writes nothing.
  FileOpen $0 "$INSTDIR\PPTShowTools.installed" w
  FileWrite $0 "installed"
  FileClose $0
  WriteUninstaller "$INSTDIR\Uninstall PPT Show Tools.exe"
  CreateShortcut "$SMPROGRAMS\PPT Show Tools.lnk" "$INSTDIR\PPTShowTools.exe" "" "$INSTDIR\PPTShowTools.exe" 0
  WriteRegStr HKCU "Software\PPT Show Tools" "InstallDir" "$INSTDIR"
  !define UNINST "Software\Microsoft\Windows\CurrentVersion\Uninstall\PPT Show Tools"
  WriteRegStr HKCU "${UNINST}" "DisplayName" "PPT Show Tools"
  WriteRegStr HKCU "${UNINST}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINST}" "Publisher" "David Redondo García"
  WriteRegStr HKCU "${UNINST}" "URLInfoAbout" "https://github.com/mentirosodiyei/PPT-Show-Tools"
  WriteRegStr HKCU "${UNINST}" "HelpLink" "https://github.com/mentirosodiyei/PPT-Show-Tools"
  WriteRegStr HKCU "${UNINST}" "DisplayIcon" "$INSTDIR\PPTShowTools.exe"
  WriteRegStr HKCU "${UNINST}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINST}" "UninstallString" '"$INSTDIR\Uninstall PPT Show Tools.exe"'
  WriteRegDWORD HKCU "${UNINST}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST}" "NoRepair" 1
SectionEnd

Section "Desktop shortcut" SecDesktop
  SetShellVarContext current
  CreateShortcut "$DESKTOP\PPT Show Tools.lnk" "$INSTDIR\PPTShowTools.exe" "" "$INSTDIR\PPTShowTools.exe" 0
SectionEnd

LangString DESC_Main ${LANG_ENGLISH} "The application: merge presentations, PDF to PowerPoint, resize slides, extract notes, images to PowerPoint and extract media."
LangString DESC_Desktop ${LANG_ENGLISH} "Adds a PPT Show Tools icon to the desktop."
!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecMain} $(DESC_Main)
  !insertmacro MUI_DESCRIPTION_TEXT ${SecDesktop} $(DESC_Desktop)
!insertmacro MUI_FUNCTION_DESCRIPTION_END

Section "Uninstall"
  SetShellVarContext current
  Delete "$INSTDIR\PPTShowTools.exe"
  Delete "$INSTDIR\PPTShowTools.ico"
  Delete "$INSTDIR\ffmpeg.exe"
  Delete "$INSTDIR\FFMPEG-LICENSE.txt"
  Delete "$INSTDIR\FFMPEG-NOTICE.txt"
  Delete "$INSTDIR\PPTShowTools.installed"
  Delete "$INSTDIR\Uninstall PPT Show Tools.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\PPT Show Tools.lnk"
  Delete "$DESKTOP\PPT Show Tools.lnk"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\PPT Show Tools"
  DeleteRegKey HKCU "Software\PPT Show Tools"
  ; Settings saved by the app (slide size).
  Delete "$APPDATA\PPT Show Tools\settings.json"
  RMDir "$APPDATA\PPT Show Tools"
  ; Reports and error logs.
  RMDir /r "$LOCALAPPDATA\PPT Show Tools\Logs"
  RMDir "$LOCALAPPDATA\PPT Show Tools"
SectionEnd
