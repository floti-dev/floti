// @ts-check
/**
 * The page's bridge, `window.floti`, for the SDK's own helpers. Read when a
 * helper is used rather than when it is imported, so importing one outside
 * Floti - in a test, or in a build that renders on the server - does not
 * throw.
 *
 * @returns {import('./types').PluginBridge}
 */
export function bridge() {
  const page = /** @type {{ floti?: import('./types').FlotiBridge }} */ (/** @type {unknown} */ (globalThis))
  const found = page.floti
  if (found === undefined) {
    throw new Error('window.floti is missing: this page is not being served by Floti')
  }
  return found
}
