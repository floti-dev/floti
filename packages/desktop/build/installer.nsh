; Included by electron-builder's NSIS script (`nsis.include` in
; electron-builder.yml), ahead of its own includes, in both the installer and
; the uninstaller.
;
; Where the app goes: %LOCALAPPDATA%\Programs\Floti\desktop, beside floti's
; own Programs\Floti\bin (customInit), unless an earlier install of Floti
; was put elsewhere or /D names a folder. The uninstall entry this installer
; writes is under the key electron-builder derives from dev.floti.desktop.
;
; After the app's files are in place (customInstall), the installer runs
; `floti install-self` from the floti.exe it just put at
; `$INSTDIR\resources\floti.exe`. That copies floti.exe, and the Node runtime
; beside it, to %LOCALAPPDATA%\Programs\Floti\bin, where the install script
; puts floti, renaming aside a floti.exe or node.exe there that is running
; rather than replacing it; puts that folder on the user's PATH (their `Path`
; value, kept an expandable string); and runs `floti setup` from there, which
; on Windows registers nothing and starts no host (the app starts the host
; when it needs one, with no console) and deletes the logon task "Floti host"
; an earlier floti registered. The desktop runs every floti it needs from
; there. What it prints goes to the details log; a failure does not fail the
; install, and the desktop tries the same step again when it starts
; (`src/main/host/local-floti.ts`).
;
; So an install ends no session. The host and its session keeper run from
; %LOCALAPPDATA%\Programs\Floti\bin, outside the install folder, which
; electron-builder's check below and the old version's uninstaller both
; leave alone (they stop every process run from inside $INSTDIR). The host
; keeps running from the copy renamed aside until the new Floti reaches it:
; a host older than Floti is restarted onto the new floti by Floti itself,
; with no question (`connectUpdatingLocalHost` in
; `src/main/host/launcher.ts`), and the keeper hands its sessions over.
;
; Except while that step has not yet worked: until then the desktop runs its
; host from `$INSTDIR\resources\floti.exe`, and the old version's
; uninstaller, which runs before any of this version's files are written,
; force-stops every process under $INSTDIR, its host and session keeper
; included, whatever this installer does. So when that floti.exe is in use (it
; cannot be opened for writing while any process runs it), the installer asks
; the host first (FLOTI_STOP_INSTALLED_HOST): `floti update-stop <client
; folder> --ask`, with the desktop's client folder.
;
; - one or more sessions: it prints the question, which says how many end and
;   that the desktop offers them back; OK stops the host for an update
;   (`--stop`, which returns once it has exited) and goes on, and Cancel quits
;   the installer with every session still running;
; - none (status 7): the host is stopped the same way, with no question;
; - anything else - no host answering, a client folder the desktop never
;   paired, a host it cannot talk to - asks nothing and leaves the host to the
;   check below.
;
; The floti.exe that asks is the one this installer carries, not the one in
; use: electron-builder packs it at `resources\floti.exe` (`dist-host\floti.exe`,
; an extraResource), and the installer writes it to its own temporary folder
; ($PLUGINSDIR), which it removes when it ends.
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
; The client folder is `host-client` in the desktop's data folder,
; %LOCALAPPDATA%\Floti\desktop (`clientFolder` in src/main/host/launcher.ts).
; It and the floti.exe carried are defines, which a script including this one
; may set first to point them elsewhere.
;
; What the install adds outside $INSTDIR, which this uninstaller leaves and
; `floti uninstall` removes (docs/PACKAGING.md lists it):
; %LOCALAPPDATA%\Programs\Floti\bin and the entry for it in the user's Path;
; it also deletes the logon task "Floti host" an earlier floti registered,
; when it finds one. `floti uninstall` runs this uninstaller itself, silently
; and in place (`/S _?=$INSTDIR`).

!include "LogicLib.nsh"

; The folder under the per-user Programs folder the app is installed in
; (`INSTALL_FOLDER` in the host's platform/installs/windows.rs).
!define FLOTI_INSTALL_FOLDER "Floti\desktop"

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
    !insertmacro FLOTI_STOP_INSTALLED_HOST
  !endif
  !insertmacro IS_POWERSHELL_AVAILABLE
  !insertmacro FLOTI_CHECK_APP_RUNNING
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

!macro customInstall
  DetailPrint "Putting floti in %LOCALAPPDATA%\Programs\Floti\bin and setting up the host."
  nsExec::ExecToLog `"$INSTDIR\resources\floti.exe" install-self`
  Pop $0 ; the exit status, or "error" when it could not run
!macroend

!ifndef FLOTI_UPDATE_STOP_EXE
  !define FLOTI_UPDATE_STOP_EXE "${PROJECT_DIR}\dist-host\floti.exe"
!endif
!ifndef FLOTI_CLIENT_FOLDER
  !define FLOTI_CLIENT_FOLDER "$LOCALAPPDATA\Floti\desktop\host-client"
!endif

; Asks a host still running from inside $INSTDIR to stop before the install's
; files are removed.
!macro FLOTI_STOP_INSTALLED_HOST
  ${If} ${FileExists} "$INSTDIR\resources\floti.exe"
    ClearErrors
    FileOpen $3 "$INSTDIR\resources\floti.exe" a
    ${If} ${Errors}
      ; In use: a host, keeper or bridge of this install runs from inside it.
      !insertmacro FLOTI_UPDATE_STOP
    ${Else}
      FileClose $3
    ${EndIf}
  ${EndIf}
!macroend

!macro FLOTI_UPDATE_STOP
  InitPluginsDir
  File "/oname=$PLUGINSDIR\floti-update-stop.exe" "${FLOTI_UPDATE_STOP_EXE}"
  nsExec::ExecToStack `"$PLUGINSDIR\floti-update-stop.exe" update-stop "${FLOTI_CLIENT_FOLDER}" --ask`
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
    nsExec::Exec `"$PLUGINSDIR\floti-update-stop.exe" update-stop "${FLOTI_CLIENT_FOLDER}" --stop`
    Pop $0
  ${EndIf}
  Delete "$PLUGINSDIR\floti-update-stop.exe"
!macroend
