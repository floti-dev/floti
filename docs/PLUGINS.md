# Plugins

The host runs them (`crates/host/src/plugins`): their registry, settings,
secrets, programs, services and their pages' `fetch`. Main keeps the
`floti-plugin://` and `helm-plugin://` schemes (`main/plugins/protocol.ts`),
the hidden plugin host window (`main/plugins/background.ts`) and the tool
relay (`main/host/plugin-tools.ts`); the window draws their pages.
`packages/plugin-sdk` holds the manifest validator Helm loads with, so a
manifest change starts there. `apiVersion` is 1 or 2, and both load side by
side; anything else is an error in Settings, never a crash.

Writing a plugin is in
[packages/plugin-sdk/README.md](../packages/plugin-sdk/README.md). This is how
Floti runs one. A plugin's agent tools are served by the host's listener, in
[AGENT-TOOLS.md](AGENT-TOOLS.md#plugin-tools).

## API versions

- Version 2 is the current one. Its pages are served on
  `floti-plugin://<id>/` and get `window.floti`; its background is an ES
  module the host runs in an isolate of its own
  (`crates/host/src/plugins/runtime`), with `floti`, `BroadcastChannel` and
  timers and nothing else, importing only its plugin's own files by relative
  path. What needs a window (opening a tab or a link, asking for a secret,
  starting or listing sessions) rejects there. `examples/sample-plugin` is a
  version 2 plugin.
- Version 1 pages are served on `helm-plugin://<id>/` and get `window.helm`,
  with the same methods; a version 1 background is a page in the hidden
  plugin window. The host writes each page URL with its version's scheme
  (`plugins/manifest.rs` `page_scheme`), which is how a client tells the two
  apart.

## The SDK

The SDK is published to npm as `@coledtaylor/helm-plugin-sdk`, and authors
start with `npx @coledtaylor/helm-plugin-sdk create`. It ships what `files` in
its `package.json` lists, and nothing it ships may reach outside its own
folder: run through npx, that folder is a cache.

## Pages

- A plugin page is served on its version's scheme, one origin per plugin,
  framed out of process, with no preload and no Node. The bridge and the
  primitives stylesheet are injected into every page it serves. The same
  plugin on a remote host has an origin of its own,
  `floti-plugin://<id>.<label>/` or `helm-plugin://<id>.<label>/`
  (`shared/plugins/origin.ts`), served from that host's files only, so no
  storage or navigation is shared between hosts; This PC's stays
  `<scheme>://<id>/`.
- Its CSP has `connect-src 'none'`: `floti.fetch` (`helm.fetch` at version 1;
  the host's `plugins/net`) is its only way out, to origins the manifest lists
  and to its own service (`service:/path`), checked again on every redirect
  hop. A version 2 background's `floti.fetch` is held to the same rule.
- `guards.ts`: `will-frame-navigate` keeps a plugin frame on its own origin,
  and plugin origins get `clipboard-sanitized-write` and no other permission.
- The host answers a page's calls, `BroadcastChannel` posts included. The window and the hidden plugin window each ask it over their own
  connection (`app/host/plugins.ts`), and each draws or answers what concerns
  only its own pages: opening a tab and a link (`shared/plugins/client-calls.ts`),
  the theme. What a version 1 background page asks that only the window can
  draw goes between the two windows through main (`pluginWindow:*`). A remote host's pages call that host only, over
  its own connection (`bridgeFor(host)` in the window, a page link per host
  in the hidden window, whose background frames are keyed by host and
  plugin).
- Frames live outside React (`app/pluginFrames.ts`) and move between slots
  with `moveBefore`; `appendChild` would reload them. One relay
  (`renderer/src/plugins/relay.ts`) per page owns every frame's port.
- A plugin's failure shows in its own surface (`PluginFrame`), never in Helm's
  chrome, and never takes Helm down.
- A plugin with `pageStrip` has one `plugin-pages` tab whose ref holds its
  pages (`core/layout/pluginPages.ts`), never a tab per page. A page's frame
  key is its `pluginPageId`, never shaped like a plugin tab's, and a page's
  frame ends when the page leaves the layout, whatever removed it.

## Background

- Version 1 background pages are iframes in the hidden `plugin-host.html`
  window, not top-level pages: Chromium partitions an iframe's storage by its
  top-level site, and a top-level page would share nothing with the plugin's
  panels. A version 2 background module has no frame and no storage of its
  own; the host relays its `BroadcastChannel` posts to and from the plugin's
  pages on every client.

## Programs and services

- `helm.exec` runs only programs the manifest names, with an argument array
  and no shell. A service gets a loopback port and a token for the run. Both
  run with the user's rights, which is why Settings names them. Turning a
  plugin off or reloading it ends its calls in flight, programs and service.

## Tools

- A plugin's tools (`agent` in its manifest) are served by the host as one
  more family on its agent tools listener. A version 2 plugin's are answered
  by its background module (`floti.tools.handle`) in the host's isolate, with
  no client attached; a version 1 plugin's by its background page
  (`helm.tools.handle`) in the hidden window, through `main/plugins/tools.ts`.
  A plugin sends no notifications and
  never sends a running session anything: a session calls it, not the other
  way round.
- A tool its manifest marks `readOnly` is listed with `readOnlyHint` and
  passed as `--allowedTools mcp__helm-plugin-<id>__<tool>` to the sessions
  Helm starts, fixed at launch; Settings marks it. Every other tool asks, and
  a deny rule of the user's still wins.

## Sessions and links

- A plugin whose `sessions` lists `start` may ask to start one (`sessions.start`): only from
  a click in its page (the relay checks the window's user activation), and
  only once the user agrees in Helm's dialog, which shows the folder, name,
  first message and command. The host composes the launch from what it
  checked and holds; the window answers
  yes or no, on the connection it was asked on, and never says what to run.
- A plugin whose `sessions` lists `list` sees Helm's sessions since it opened
  (`sessions.list`, the `sessions` event), shaped by the host
  (`server/features/plugins/sessions.rs`), whose shape has no field for
  anything of a conversation: no argv, conversation id or `waitingFor`. A
  session's id there is the one its tool calls carry, minted by the host,
  never its row id.
- Any plugin may open an `https` link in the Browser tab (`open`), from a
  click only (the same relay check), one a second. The page that draws it
  checks it is an `https` address (`client-calls.ts`); the window opens it through `browser:open`, so it meets
  the reach rule a typed address does. The plugin never reads or drives it.

## Development

- `HELM_PLUGINS` registers folders in a dev build only.
