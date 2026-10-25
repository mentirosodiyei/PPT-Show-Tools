; PPT Merger for Windows: per-user installer (no administrator rights needed).
Unicode true
!include "MUI2.nsh"

!ifndef VERSION
  !define VERSION "0.6.0"
!endif
!ifndef EXE
  !define EXE "..\target\x86_64-pc-windows-msvc\release\PPTMerger.exe"
!endif
!ifndef OUTFILE
  !define OUTFILE "PPT-Merger-Setup-${VERSION}.exe"
!endif

Name "PPT Merger"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\PPT Merger"
InstallDirRegKey HKCU "Software\PPT Merger" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
BrandingText "PPT Merger ${VERSION}"

VIProductVersion "${VERSION}.0"
VIAddVersionKey /LANG=1027 "ProductName" "PPT Merger"
VIAddVersionKey /LANG=1027 "FileDescription" "Instal·lador de PPT Merger"
VIAddVersionKey /LANG=1027 "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=1027 "ProductVersion" "${VERSION}"
VIAddVersionKey /LANG=1027 "LegalCopyright" "PPT Merger"

!define MUI_ICON "..\assets\app.ico"
!define MUI_UNICON "..\assets\app.ico"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\PPTMerger.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Obre PPT Merger"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "Catalan"

Section "PPT Merger" SecMain
  SectionIn RO
  SetShellVarContext current
  SetOutPath "$INSTDIR"
  File "/oname=PPTMerger.exe" "${EXE}"
  File "/oname=PPTMerger.ico" "..\assets\app.ico"
  WriteUninstaller "$INSTDIR\Desinstal·la PPT Merger.exe"
  CreateShortcut "$SMPROGRAMS\PPT Merger.lnk" "$INSTDIR\PPTMerger.exe" "" "$INSTDIR\PPTMerger.exe" 0
  WriteRegStr HKCU "Software\PPT Merger" "InstallDir" "$INSTDIR"
  !define UNINST "Software\Microsoft\Windows\CurrentVersion\Uninstall\PPTMerger"
  WriteRegStr HKCU "${UNINST}" "DisplayName" "PPT Merger"
  WriteRegStr HKCU "${UNINST}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINST}" "Publisher" "PPT Merger"
  WriteRegStr HKCU "${UNINST}" "DisplayIcon" "$INSTDIR\PPTMerger.exe"
  WriteRegStr HKCU "${UNINST}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINST}" "UninstallString" '"$INSTDIR\Desinstal·la PPT Merger.exe"'
  WriteRegDWORD HKCU "${UNINST}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST}" "NoRepair" 1
SectionEnd

Section "Accés directe a l’escriptori" SecDesktop
  SetShellVarContext current
  CreateShortcut "$DESKTOP\PPT Merger.lnk" "$INSTDIR\PPTMerger.exe" "" "$INSTDIR\PPTMerger.exe" 0
SectionEnd

LangString DESC_Main ${LANG_CATALAN} "L’aplicació: fusió, PDF a PowerPoint, 16:9 i notes."
LangString DESC_Desktop ${LANG_CATALAN} "Afegeix una icona de PPT Merger a l’escriptori."
!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecMain} $(DESC_Main)
  !insertmacro MUI_DESCRIPTION_TEXT ${SecDesktop} $(DESC_Desktop)
!insertmacro MUI_FUNCTION_DESCRIPTION_END

Section "Uninstall"
  SetShellVarContext current
  Delete "$INSTDIR\PPTMerger.exe"
  Delete "$INSTDIR\PPTMerger.ico"
  Delete "$INSTDIR\Desinstal·la PPT Merger.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\PPT Merger.lnk"
  Delete "$DESKTOP\PPT Merger.lnk"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\PPTMerger"
  DeleteRegKey HKCU "Software\PPT Merger"
SectionEnd
