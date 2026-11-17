import { installBridge } from './bridge'

/**
 * The script a plugin page loads: `bridge.ts`, run against the page it is in,
 * with the start-up facts main put on its own tag (`data-floti-boot`).
 */
const script = document.currentScript
installBridge(window, script === null ? null : script.getAttribute('data-floti-boot'))
