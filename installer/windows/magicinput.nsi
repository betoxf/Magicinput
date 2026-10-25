; MagicInput setup for Windows. Built by scripts/release.sh:
;   makensis -DVERSION=X.Y.Z -DDIST=dist/windows -DOUTFILE=dist/MagicInput-Setup.exe installer/windows/magicinput.nsi
; The setup runs `magicinput.exe install`, the same installer the app uses, so the result is the
; same as installing from the zip: %LOCALAPPDATA%\Programs\magicinput, a startup task, a firewall
; rule for private networks and a Start menu entry. Silent install: MagicInput-Setup.exe /S

Unicode true
!include "MUI2.nsh"

!define APP "MagicInput"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\MagicInput"

Name "${APP}"
OutFile "${OUTFILE}"
RequestExecutionLevel admin
SetCompressor /SOLID lzma
InstallDir "$LOCALAPPDATA\Programs\magicinput"
BrandingText "Bullfighter Studio"
ShowInstDetails nevershow
ShowUninstDetails nevershow

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${APP}"
VIAddVersionKey "CompanyName" "Bullfighter Studio"
VIAddVersionKey "FileDescription" "MagicInput Setup"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "Copyright 2026 Bullfighter Studio. PolyForm Noncommercial 1.0.0."

!define MUI_ICON "..\..\assets\MagicInput.ico"
!define MUI_UNICON "..\..\assets\MagicInput.ico"
!define MUI_WELCOMEFINISHPAGE_BITMAP "welcome.bmp"
!define MUI_UNWELCOMEFINISHPAGE_BITMAP "welcome.bmp"
!define MUI_ABORTWARNING

!define MUI_WELCOMEPAGE_TITLE "Welcome to MagicInput ${VERSION}"
!define MUI_WELCOMEPAGE_TEXT "One keyboard and mouse for your Mac and your PC.$\r$\n$\r$\nSetup installs MagicInput for you, starts it now and each time you sign in, and allows it through the Windows firewall on private networks.$\r$\n$\r$\nIf MagicInput is already installed, it's updated and your settings are kept."
!define MUI_FINISHPAGE_TITLE "MagicInput is ready"
!define MUI_FINISHPAGE_TEXT "MagicInput finds your other computer by itself. Open it on both computers and pair them with the code under the link icon."
!define MUI_FINISHPAGE_RUN
!define MUI_FINISHPAGE_RUN_TEXT "Open MagicInput"
!define MUI_FINISHPAGE_RUN_FUNCTION OpenApp
!define MUI_FINISHPAGE_LINK "bullfighterstudio.com/magicinput"
!define MUI_FINISHPAGE_LINK_LOCATION "https://bullfighterstudio.com/magicinput"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Function .onInit
  SetShellVarContext current
FunctionEnd

Function un.onInit
  SetShellVarContext current
FunctionEnd

Section "MagicInput"
  InitPluginsDir
  SetOutPath "$PLUGINSDIR\app"
  File "${DIST}\magicinput.exe"
  File "${DIST}\magicinputw.exe"
  File "${DIST}\magicinput-app.exe"
  File "${DIST}\WebView2Loader.dll"

  DetailPrint "Installing MagicInput..."
  nsExec::ExecToLog '"$PLUGINSDIR\app\magicinput.exe" install'
  Pop $0
  StrCmp $0 "0" installed
    SetOutPath "$TEMP"
    MessageBox MB_ICONSTOP "MagicInput could not be installed (error $0). Please try again, or contact bullfighterstudio.com/contact." /SD IDOK
    Abort
  installed:
  SetOutPath "$INSTDIR"

  WriteUninstaller "$INSTDIR\Uninstall MagicInput.exe"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${APP}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "Bullfighter Studio"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\magicinput-app.exe"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "URLInfoAbout" "https://bullfighterstudio.com/magicinput"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\Uninstall MagicInput.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\Uninstall MagicInput.exe" /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "EstimatedSize" 12000
SectionEnd

; Open the app as the signed-in user, not as administrator.
Function OpenApp
  Exec '"$WINDIR\explorer.exe" "$INSTDIR\magicinput-app.exe"'
FunctionEnd

Section "Uninstall"
  nsExec::ExecToLog '"$INSTDIR\magicinput.exe" uninstall'
  nsExec::ExecToLog 'taskkill /f /im magicinput-app.exe'
  Sleep 500
  Delete "$INSTDIR\magicinput.exe"
  Delete "$INSTDIR\magicinputw.exe"
  Delete "$INSTDIR\magicinput-app.exe"
  Delete "$INSTDIR\WebView2Loader.dll"
  Delete "$INSTDIR\Uninstall MagicInput.exe"
  RMDir /r "$INSTDIR\magicinput-app.exe.WebView2"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\MagicInput.lnk"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
SectionEnd
