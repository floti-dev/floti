import type { AddHostActions, HostInstallActions } from '@floti/ui'
import { desktop } from './bridge'

/**
 * What a window does on another computer through main: Settings > Hosts'
 * check, install and cancel, and the add-a-host dialog's reads, checks and
 * adds. Shared by the shell and the setup window, whose step 3 opens the same
 * dialog; main runs each through that host's own transport.
 */

/** Settings > Hosts' check, install and cancel on a host's computer. */
export const hostInstall: HostInstallActions = {
  check: (target) => desktop.invoke('remoteHosts:check', target),
  install: (id, host) => desktop.invoke('remoteHosts:install', { id, host }),
  cancel: (id) => void desktop.invoke('remoteHosts:cancel', { id }),
  progress: (id, listener) =>
    desktop.on('remoteHosts:installProgress', (said) => {
      if (said.id === id) listener(said.line)
    })
}

/**
 * The add-a-host dialog's: main reads this computer's ssh config Host names,
 * checks and installs through the user's own ssh or command, and adds a host
 * only once it connected.
 */
export const addHostActions: AddHostActions = {
  sshHosts: () => desktop.invoke('remoteHosts:sshHosts'),
  wslDistros: () => desktop.invoke('remoteHosts:wslDistros'),
  check: hostInstall.check,
  install: hostInstall.install,
  cancelInstall: hostInstall.cancel,
  add: (request) => desktop.invoke('remoteHosts:add', request),
  cancelAdd: (attempt) => void desktop.invoke('remoteHosts:cancelAdd', { attempt })
}
