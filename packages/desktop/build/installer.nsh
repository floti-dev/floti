; Included by electron-builder's NSIS script (`nsis.include` in
; electron-builder.yml), ahead of its own includes, in both the installer and
; the uninstaller.
;
; Where the app goes: %LOCALAPPDATA%\Programs\Floti\desktop, beside floti's
; own Programs\Floti\bin (customInit), unless an earlier install of Floti
; was put elsewhere or /D names a folder.
;
; Once, an install of the app from when it was called Helm moves there: the
; installer finds it by the uninstall entry its app ID (dev.coletaylor.helm)
; gave it, or in %LOCALAPPDATA%\Programs\Helm, and runs its own uninstaller
; silently before this version's files go in (FLOTI_MOVE_HELM_INSTALL). That
; uninstaller removes its files, its Start menu shortcut, its app ID's
; registration and its uninstall entry, and keeps the user's data, which Floti
; moves itself at its first start (src/main/dataFolder.ts). Windows'
; notification settings for the old app ID go too. The uninstall entry this
; installer writes is the new one, under the key electron-builder derives from
; dev.floti.desktop.
;
; After the app's files are in place (customInstall), the installer runs
; `floti install-self` from the floti.exe it just put at
; `$INSTDIR\resources\floti.exe`. That copies floti.exe, and the Node runtime
; beside it, to %LOCALAPPDATA%\Programs\Floti\bin, where the install script
; puts floti, renaming aside a floti.exe or node.exe there that is running
; rather than replacing it; puts that folder on the user's PATH (their `Path`
; value, kept an expandable string); and runs `floti setup` from there, which
; registers that floti to start the host at sign-in and starts it. Helm runs
; every floti it needs from there. What it prints goes to the details log; a
; failure does not fail the install, and Helm tries the same step again when
; it starts (`src/main/host/local-floti.ts`).
;
; So an install ends no session. The host and its session keeper run from
; %LOCALAPPDATA%\Programs\Floti\bin, outside the install folder, which
; electron-builder's check below and the old version's uninstaller both
; leave alone (they stop every process run from inside $INSTDIR). The host
; keeps running from the copy renamed aside until Helm's Restart host to
; update hands its sessions to the new host through the keeper.
;
; Except once: a Helm from before this ran its host from
; `$INSTDIR\resources\floti.exe`, and the old version's uninstaller, which
; runs before any of this version's files are written, force-stops every
; process under $INSTDIR, its host and session keeper included, whatever
; this installer does. So when that floti.exe is in use (it cannot be opened
; for writing while any process runs it), the installer asks the host first,
; as it always did: `floti update-stop <client folder> --ask`, with Helm's
; client folder.
;
; - one or more sessions: it prints the question, which says how many end and
;   that Helm offers them back; OK stops the host for an update (`--stop`,
;   which returns once it has exited) and goes on, and Cancel quits the
;   installer with every session still running;
; - none (status 7): the host is stopped the same way, with no question;
; - anything else - no host answering, a client folder Helm never paired, a
;   host it cannot talk to - asks nothing and leaves the host to the check
;   below.
;
; The floti.exe that asks is the one this installer carries, not the one
; installed: an installed Helm from before `update-stop` existed has a
; floti.exe that does not know the subcommand. The installer carries the copy
; electron-builder packs at `resources\floti.exe` (`dist-host\floti.exe`, an
; extraResource) and writes it to its own temporary folder ($PLUGINSDIR),
; which it removes when it ends.
;
; Then the check that the app is not running (FLOTI_CHECK_APP_RUNNING), which
; asks the user to close Floti if it is still running; a silent install (/S)
; answers OK. It is electron-builder's check with one difference: it finds the
; app only by its path, a process whose program is inside $INSTDIR, and never
; by its image name. Floti.exe and the host's floti.exe differ only by case,
; and electron-builder's fallback for a computer without PowerShell
; (`tasklist` and `taskkill /IM Floti.exe`) would match the host, its keeper
; and its bridges, and end every session. Without PowerShell it looks whether
; the app's program can be opened for writing, which a running program
; cannot, and asks the user to close the app rather than ending anything.
;
; The client folder is `host-client` in the data directory of the Helm whose
; host that is, %APPDATA%\Helm (`clientFolder` in src/main/host/launcher.ts):
; only a Helm from before update-stop ran its host from inside its install.
; It and the floti.exe carried are defines, which a script including this one
; may set first to point them elsewhere.
;
; What the install adds outside $INSTDIR, which this uninstaller leaves and
; `floti uninstall` removes (docs/PACKAGING.md lists it):
; %LOCALAPPDATA%\Programs\Floti\bin, the entry for it in the user's Path, and
; setup's logon task "Floti host". `floti uninstall` runs this uninstaller
; itself, silently and in place (`/S _?=$INSTDIR`).

!include "LogicLib.nsh"
; The installer alone uses it, and makensis is run with warnings as errors.
!ifndef BUILD_UNINSTALLER
  Var helmInstall
!endif

; The folder under the per-user Programs folder the app is installed in
; (`INSTALL_FOLDER` in the host's platform/installs/windows.rs).
!define FLOTI_INSTALL_FOLDER "Floti\desktop"

; What electron-builder derives from the app ID Helm had, dev.coletaylor.helm:
; the key of its uninstall entry and of the install's own registry key.
; `installer.test.ts` holds this to the UUID v5 electron-builder computes.
!define HELM_APP_GUID "3faefc2b-fc42-5861-891b-1eda657b27a9"
!define HELM_UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${HELM_APP_GUID}"
!define HELM_INSTALL_KEY "Software\${HELM_APP_GUID}"

!macro customInit
  ; Beside floti's own folder, unless this app was installed elsewhere before
  ; (electron-builder has read that from its registry key) or /D says where.
  ReadRegStr $0 HKCU "${INSTALL_REGISTRY_KEY}" InstallLocation
  !insertmacro GetDParameter $1
  ${If} $0 == ""
  ${AndIf} $1 == ""
    StrCpy $INSTDIR "$LOCALAPPDATA\Programs\${FLOTI_INSTALL_FOLDER}"
  ${EndIf}
!macroend

!macro customCheckAppRunning
  !ifndef BUILD_UNINSTALLER
    !insertmacro FLOTI_FIND_HELM_INSTALL
    !insertmacro HELM_STOP_OLD_LOCAL_HOST "$INSTDIR"
    ${If} $helmInstall != ""
      !insertmacro HELM_STOP_OLD_LOCAL_HOST "$helmInstall"
    ${EndIf}
  !endif
  !insertmacro IS_POWERSHELL_AVAILABLE
  !insertmacro FLOTI_CHECK_APP_RUNNING
  !ifndef BUILD_UNINSTALLER
    !insertmacro FLOTI_MOVE_HELM_INSTALL
  !endif
!macroend

; Sets _RETURN to 0 when the app runs from $INSTDIR, else 1: by the path of
; each process through PowerShell, else by whether the app's program can be
; opened for writing. Never by image name (see the top of this file).
!macro FLOTI_APP_RUNNING _RETURN
  ${If} $IsPowerShellAvailable == 0
    nsExec::Exec `"$PowerShellPath" -C "if ((Get-CimInstance -ClassName Win32_Process | ? {$$_.Path -and $$_.Path.StartsWith('$INSTDIR\', 'CurrentCultureIgnoreCase')}).Count -gt 0) { exit 0 } else { exit 1 }"`
    Pop ${_RETURN}
  ${Else}
    StrCpy ${_RETURN} 1
    ${If} ${FileExists} "$INSTDIR\${APP_EXECUTABLE_FILENAME}"
      ClearErrors
      FileOpen $R9 "$INSTDIR\${APP_EXECUTABLE_FILENAME}" a
      ${If} ${Errors}
        StrCpy ${_RETURN} 0
      ${Else}
        FileClose $R9
      ${EndIf}
    ${EndIf}
  ${EndIf}
!macroend

; Ends every process whose program is inside $INSTDIR, by path, through
; PowerShell; FORCE is "-Force" or "". Without PowerShell it ends nothing.
!macro FLOTI_STOP_APP FORCE
  ${If} $IsPowerShellAvailable == 0
    nsExec::Exec `"$PowerShellPath" -C "Get-CimInstance -ClassName Win32_Process | ? {$$_.Path -and $$_.Path.StartsWith('$INSTDIR\', 'CurrentCultureIgnoreCase')} | % { Stop-Process -Id $$_.ProcessId ${FORCE} }"`
    Pop $R8
  ${EndIf}
!macroend

; electron-builder's _CHECK_APP_RUNNING, finding the app by path alone: asks
; to close the app (a silent run answers OK), ends it by path, and asks to
; retry or cancel while it is still running.
!macro FLOTI_CHECK_APP_RUNNING
  ; electron-builder declares $CmdPath for its own check, which this replaces;
  ; reading it keeps makensis, run with warnings as errors, from refusing an
  ; unused variable.
  StrCpy $R7 $CmdPath
  !insertmacro FLOTI_APP_RUNNING $R0
  ${If} $R0 == 0
    ${If} ${Cmd} `MessageBox MB_OKCANCEL|MB_ICONEXCLAMATION "$(appRunning)" /SD IDOK IDCANCEL`
      Quit
    ${EndIf}
    DetailPrint "$(appClosing)"
    StrCpy $R1 0
    ${Do}
      ${If} $R1 < 2
        !insertmacro FLOTI_STOP_APP ""
      ${Else}
        !insertmacro FLOTI_STOP_APP "-Force"
      ${EndIf}
      Sleep 1000
      !insertmacro FLOTI_APP_RUNNING $R0
      ${IfThen} $R0 != 0 ${|} ${Break} ${|}
      IntOp $R1 $R1 + 1
      ${If} $R1 > 4
        ${If} ${Cmd} `MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "$(appCannotBeClosed)" /SD IDCANCEL IDCANCEL`
          Quit
        ${EndIf}
        StrCpy $R1 0
      ${EndIf}
    ${Loop}
  ${EndIf}
!macroend

; Sets $helmInstall to the folder of an install from when the app was called
; Helm, or to "" when there is none: the folder its registry key names, else
; %LOCALAPPDATA%\Programs\Helm, holding its uninstaller.
!macro FLOTI_FIND_HELM_INSTALL
  ReadRegStr $helmInstall HKCU "${HELM_INSTALL_KEY}" InstallLocation
  ${If} $helmInstall == ""
    StrCpy $helmInstall "$LOCALAPPDATA\Programs\Helm"
  ${EndIf}
  ${IfNot} ${FileExists} "$helmInstall\Uninstall Helm.exe"
    StrCpy $helmInstall ""
  ${EndIf}
!macroend

; Removes the install $helmInstall names by its own uninstaller, run silently
; in place from a copy, which takes its uninstall entry, its Start menu
; shortcut and its app ID's registration and keeps the user's data. A
; failure is in the details log and stops nothing: this version installs in
; its own folder either way.
!macro FLOTI_MOVE_HELM_INSTALL
  ${If} $helmInstall != ""
    DetailPrint "Moving Helm from $helmInstall to $INSTDIR."
    InitPluginsDir
    CopyFiles /SILENT "$helmInstall\Uninstall Helm.exe" "$PLUGINSDIR\helm-uninstaller.exe"
    ExecWait '"$PLUGINSDIR\helm-uninstaller.exe" /S /currentuser _?=$helmInstall' $0
    Delete "$PLUGINSDIR\helm-uninstaller.exe"
    ${If} $0 == 0
      Delete "$helmInstall\Uninstall Helm.exe"
      RMDir "$helmInstall"
    ${Else}
      DetailPrint "Helm's uninstaller ended with $0; $helmInstall is left as it is."
    ${EndIf}
  ${EndIf}
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Notifications\Settings\dev.coletaylor.helm"
!macroend

!macro customInstall
  DetailPrint "Putting floti in %LOCALAPPDATA%\Programs\Floti\bin and setting up the host."
  nsExec::ExecToLog `"$INSTDIR\resources\floti.exe" install-self`
  Pop $0 ; the exit status, or "error" when it could not run
!macroend

!ifndef HELM_UPDATE_STOP_FLOTI
  !define HELM_UPDATE_STOP_FLOTI "${PROJECT_DIR}\dist-host\floti.exe"
!endif
!ifndef HELM_CLIENT_FOLDER
  !define HELM_CLIENT_FOLDER "$APPDATA\Helm\host-client"
!endif

; Asks a host still running from inside the install FOLDER, from a Helm before
; update-stop, to stop before that install's files are removed.
!macro HELM_STOP_OLD_LOCAL_HOST FOLDER
  ${If} ${FileExists} "${FOLDER}\resources\floti.exe"
    ClearErrors
    FileOpen $3 "${FOLDER}\resources\floti.exe" a
    ${If} ${Errors}
      ; In use: a host, keeper or bridge of that Helm runs from inside it.
      !insertmacro HELM_UPDATE_STOP
    ${Else}
      FileClose $3
    ${EndIf}
  ${EndIf}
!macroend

!macro HELM_UPDATE_STOP
  InitPluginsDir
  File "/oname=$PLUGINSDIR\floti-update-stop.exe" "${HELM_UPDATE_STOP_FLOTI}"
  nsExec::ExecToStack `"$PLUGINSDIR\floti-update-stop.exe" update-stop "${HELM_CLIENT_FOLDER}" --ask`
  Pop $0 ; the exit status, or "error" when it could not run
  Pop $1 ; the question, when there is one
  StrCpy $2 ""
  ${If} $0 == 7
    StrCpy $2 "stop"
  ${ElseIf} $0 == 0
    ${If} ${Cmd} `MessageBox MB_OKCANCEL|MB_ICONEXCLAMATION "$1" /SD IDOK IDCANCEL`
      Quit
    ${EndIf}
    StrCpy $2 "stop"
  ${EndIf}
  ${If} $2 == "stop"
    DetailPrint "Stopping the local host on this PC."
    nsExec::Exec `"$PLUGINSDIR\floti-update-stop.exe" update-stop "${HELM_CLIENT_FOLDER}" --stop`
    Pop $0
  ${EndIf}
  Delete "$PLUGINSDIR\floti-update-stop.exe"
!macroend
