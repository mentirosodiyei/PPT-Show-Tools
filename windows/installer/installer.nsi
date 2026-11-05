; Deckwright for Windows: per-user installer (no administrator rights needed).
Unicode true
!include "MUI2.nsh"

!ifndef VERSION
  !define VERSION "0.7.0"
!endif
!ifndef EXE
  !define EXE "Deckwright.exe"
!endif
!ifndef OUTFILE
  !define OUTFILE "Deckwright-Setup-${VERSION}.exe"
!endif

Name "Deckwright"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\Deckwright"
InstallDirRegKey HKCU "Software\Deckwright" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
BrandingText "Deckwright ${VERSION}"

VIProductVersion "${VERSION}.0"
VIAddVersionKey /LANG=1033 "ProductName" "Deckwright"
VIAddVersionKey /LANG=1033 "FileDescription" "Deckwright Setup"
VIAddVersionKey /LANG=1033 "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=1033 "ProductVersion" "${VERSION}"
VIAddVersionKey /LANG=1033 "LegalCopyright" "Deckwright"

!define MUI_ICON "..\assets\app.ico"
!define MUI_UNICON "..\assets\app.ico"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\Deckwright.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Open Deckwright"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Section "Deckwright" SecMain
  SectionIn RO
  SetShellVarContext current
  SetOutPath "$INSTDIR"
  File "/oname=Deckwright.exe" "${EXE}"
  File "/oname=Deckwright.ico" "..\assets\app.ico"
  WriteUninstaller "$INSTDIR\Uninstall Deckwright.exe"
  CreateShortcut "$SMPROGRAMS\Deckwright.lnk" "$INSTDIR\Deckwright.exe" "" "$INSTDIR\Deckwright.exe" 0
  WriteRegStr HKCU "Software\Deckwright" "InstallDir" "$INSTDIR"
  !define UNINST "Software\Microsoft\Windows\CurrentVersion\Uninstall\Deckwright"
  WriteRegStr HKCU "${UNINST}" "DisplayName" "Deckwright"
  WriteRegStr HKCU "${UNINST}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINST}" "Publisher" "Deckwright"
  WriteRegStr HKCU "${UNINST}" "DisplayIcon" "$INSTDIR\Deckwright.exe"
  WriteRegStr HKCU "${UNINST}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINST}" "UninstallString" '"$INSTDIR\Uninstall Deckwright.exe"'
  WriteRegDWORD HKCU "${UNINST}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST}" "NoRepair" 1
SectionEnd

Section "Desktop shortcut" SecDesktop
  SetShellVarContext current
  CreateShortcut "$DESKTOP\Deckwright.lnk" "$INSTDIR\Deckwright.exe" "" "$INSTDIR\Deckwright.exe" 0
SectionEnd

LangString DESC_Main ${LANG_ENGLISH} "The application: merge presentations, PDF to PowerPoint, resize slides and extract notes."
LangString DESC_Desktop ${LANG_ENGLISH} "Adds a Deckwright icon to the desktop."
!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecMain} $(DESC_Main)
  !insertmacro MUI_DESCRIPTION_TEXT ${SecDesktop} $(DESC_Desktop)
!insertmacro MUI_FUNCTION_DESCRIPTION_END

Section "Uninstall"
  SetShellVarContext current
  Delete "$INSTDIR\Deckwright.exe"
  Delete "$INSTDIR\Deckwright.ico"
  Delete "$INSTDIR\Uninstall Deckwright.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\Deckwright.lnk"
  Delete "$DESKTOP\Deckwright.lnk"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Deckwright"
  DeleteRegKey HKCU "Software\Deckwright"
  ; Settings saved by the app (slide size).
  Delete "$APPDATA\Deckwright\settings.json"
  RMDir "$APPDATA\Deckwright"
SectionEnd
