import type { AddHostActions, HostInstallActions } from '@helm/ui'
import { helm } from './bridge'

/**
 * What a window does on another computer through main: Settings > Hosts'
 * check, install and cancel, and the add-a-host dialog's reads, checks and
 * adds. Shared by the shell and the setup window, whose step 3 opens the same
 * dialog; main runs each through that host's own transport.
 */

/** Settings > Hosts' check, install and cancel on a host's computer. */
export const hostInstall: HostInstallActions = {
  check: (target) => helm.invoke('remoteHosts:check', target),
  install: (id, host) => helm.invoke('remoteHosts:install', { id, host }),
  cancel: (id) => void helm.invoke('remoteHosts:cancel', { id }),
  progress: (id, listener) =>
    helm.on('remoteHosts:installProgress', (said) => {
      if (said.id === id) listener(said.line)
    })
}

/**
 * The add-a-host dialog's: main reads this computer's ssh config Host names,
 * checks and installs through the user's own ssh or command, and adds a host
 * only once it connected.
 */
export const addHostActions: AddHostActions = {
  sshHosts: () => helm.invoke('remoteHosts:sshHosts'),
  wslDistros: () => helm.invoke('remoteHosts:wslDistros'),
  check: hostInstall.check,
  install: hostInstall.install,
  cancelInstall: hostInstall.cancel,
  add: (request) => helm.invoke('remoteHosts:add', request),
  cancelAdd: (attempt) => void helm.invoke('remoteHosts:cancelAdd', { attempt })
}
