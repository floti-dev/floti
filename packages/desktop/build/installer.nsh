; Included by electron-builder's NSIS script (`nsis.include` in
; electron-builder.yml), ahead of its own includes, in both the installer and
; the uninstaller.
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
; Then electron-builder's own check runs, which asks the user to close Helm if
; it is still running. A silent install (/S) answers OK, as that check does.
;
; Defining customCheckAppRunning replaces the default check, and drops the
; include and variable allowOnlyOneInstallerInstance.nsh brings for it; the
; default check is still the one that runs, so both are brought here.
;
; The client folder is `host-client` in Helm's data directory, which for the
; installed app is %APPDATA%\<productName> (`clientFolder` in
; src/main/host/launcher.ts, `appMode` in src/main/paths.ts). It and the
; floti.exe carried are defines, which a script including this one may set
; first to point them elsewhere.
;
; What the install adds outside $INSTDIR, which this uninstaller leaves and
; `floti uninstall` removes (docs/PACKAGING.md lists it):
; %LOCALAPPDATA%\Programs\Floti\bin, the entry for it in the user's Path, and
; setup's logon task "Floti host". `floti uninstall` runs this uninstaller
; itself, silently and in place (`/S _?=$INSTDIR`).

!include "LogicLib.nsh"
!include "getProcessInfo.nsh"
Var pid

!macro customCheckAppRunning
  !ifndef BUILD_UNINSTALLER
    !insertmacro HELM_STOP_OLD_LOCAL_HOST
  !endif
  !insertmacro IS_POWERSHELL_AVAILABLE
  !insertmacro _CHECK_APP_RUNNING
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
  !define HELM_CLIENT_FOLDER "$APPDATA\${PRODUCT_NAME}\host-client"
!endif

!macro HELM_STOP_OLD_LOCAL_HOST
  ${If} ${FileExists} "$INSTDIR\resources\floti.exe"
    ClearErrors
    FileOpen $3 "$INSTDIR\resources\floti.exe" a
    ${If} ${Errors}
      ; In use: a host, keeper or bridge of this Helm runs from inside it.
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
