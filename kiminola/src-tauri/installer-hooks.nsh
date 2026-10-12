; The product name changes the default NSIS uninstall key. Remove the previous
; app through its own updater-mode uninstaller before installing OatWave.
; User data and the WebView profile keep their original namespaces.
!include StrFunc.nsh
${StrRep}
Var OatWaveLegacyInstallDir

!macro NSIS_HOOK_PREINSTALL
  Push $0
  Push $1
  Push $2
  ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Kimi Nola" "InstallLocation"
  ${If} $0 != ""
    ${StrRep} $0 $0 '$\"' ""
    StrCpy $OatWaveLegacyInstallDir $0
    ReadRegStr $1 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Kimi Nola" "DisplayVersion"
    nsis_tauri_utils::SemverCompare "${VERSION}" $1
    Pop $2
    ${If} $2 = -1
      MessageBox MB_OK|MB_ICONSTOP "A newer Kimi Nola version is installed. Install a newer OatWave release to upgrade." /SD IDOK
      SetErrorLevel 1
      Abort
    ${EndIf}
    IfFileExists "$0\uninstall.exe" oatwave_legacy_uninstaller_present 0
      MessageBox MB_OK|MB_ICONSTOP "The previous Kimi Nola uninstaller is missing. Repair that installation before installing OatWave." /SD IDOK
      SetErrorLevel 1
      Abort
    oatwave_legacy_uninstaller_present:
    ClearErrors
    ExecWait '$\"$0\uninstall.exe$\" /S /UPDATE _?=$0' $1
    ${If} ${Errors}
    ${OrIf} $1 != 0
    ${OrIf} ${FileExists} "$0\kiminola.exe"
      MessageBox MB_OK|MB_ICONSTOP "The previous Kimi Nola installation could not be removed. Close it and retry installing OatWave." /SD IDOK
      SetErrorLevel 1
      Abort
    ${EndIf}
    Delete "$0\uninstall.exe"
    RMDir "$0"
  ${EndIf}
  Pop $2
  Pop $1
  Pop $0
!macroend

!macro NSIS_HOOK_POSTINSTALL
  Push $0
  Push $1
  Push $2
  Push $3
  Push $4
  Push $5
  ; Updater-mode uninstall preserves shortcuts. Replace only shortcuts that
  ; point to the previous executable, including when /UPDATE skips new ones.
  ${If} $OatWaveLegacyInstallDir != ""
    !insertmacro IsShortcutTarget "$SMPROGRAMS\Kimi Nola.lnk" "$OatWaveLegacyInstallDir\kiminola.exe"
    Pop $0
    ${If} $0 = 1
      !insertmacro UnpinShortcut "$SMPROGRAMS\Kimi Nola.lnk"
      Delete "$SMPROGRAMS\Kimi Nola.lnk"
      CreateShortcut "$SMPROGRAMS\OatWave.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
      !insertmacro SetLnkAppUserModelId "$SMPROGRAMS\OatWave.lnk"
    ${EndIf}
    !insertmacro IsShortcutTarget "$DESKTOP\Kimi Nola.lnk" "$OatWaveLegacyInstallDir\kiminola.exe"
    Pop $0
    ${If} $0 = 1
      !insertmacro UnpinShortcut "$DESKTOP\Kimi Nola.lnk"
      Delete "$DESKTOP\Kimi Nola.lnk"
      CreateShortcut "$DESKTOP\OatWave.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
      !insertmacro SetLnkAppUserModelId "$DESKTOP\OatWave.lnk"
    ${EndIf}
  ${EndIf}
  ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "KimiNola"
  ${If} $0 != ""
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "OatWave" '$\"$INSTDIR\${MAINBINARYNAME}.exe$\" --background'
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "KimiNola"
  ${EndIf}
  Pop $5
  Pop $4
  Pop $3
  Pop $2
  Pop $1
  Pop $0
!macroend
