---
name: dev
description: Running Helm in development, looking at a change, and the diagnostic drivers. Use before launching the app to look at something, driving the window you have open, or diagnosing a problem a test cannot reach.
---

## The dev build

```bash
pnpm dev            # its own data directory; the database is a copy of the real one
pnpm dev --fresh    # no database: the first-run state
pnpm dev --drive    # a remote debugging port for drive-dev.mjs, and only the fake agents
```

- The data directory is `%LOCALAPPDATA%\Helm\dev\helm-data`, seeded each launch
  from a `VACUUM INTO` copy of the real database. Anything changed in dev,
  settings included, is gone at the next launch. A second `pnpm dev` gets
  `dev-2`.
- With `--drive`, every agent is a fake from `test/fake-agents`: each launch
  writes them under the dev folder's `fake-agents/` and chooses them in the
  database, and Claude Code's config folder is `fake-agents/claude`. A new
  session, a restore, a History resume and an agent-started child all run a
  fake, and the console names each program. `--fresh` starts that folder over.
- Without `--drive`, `~/.claude` and `claude` are the real ones, because
  `CLAUDE_CONFIG_DIR` moves credentials and a dev app that cannot sign in
  cannot host a session. Those dev sessions land in the real
  `~/.claude/history.jsonl`.
- Templates are copied once and then left alone; `--fresh` re-copies them.

`pnpm dev:live` runs against the installed app's `%APPDATA%\Helm`. Do not use it
unless the owner asks.

## Looking at the window

With `pnpm dev --drive` running:

```bash
node packages/desktop/scripts/drive-dev.mjs text             # what the window says
node packages/desktop/scripts/drive-dev.mjs controls         # every button, by label
node packages/desktop/scripts/drive-dev.mjs click "Session history"
node packages/desktop/scripts/drive-dev.mjs eval "window.helm.invoke('settings:write', {...})"
node packages/desktop/scripts/drive-dev.mjs shot history.png
```

`shot` captures the renderer's own pixels, so it works with the window covered.
The port is off unless `--drive` asks for it.

## Diagnostic drivers

These drive the real window and are tools for diagnosis, not tests. Nothing has
to pass them before a change is done. Each runs in its own data directory under
`%LOCALAPPDATA%\Helm\checks\<name>`.

| command | answers |
|---|---|
| `pnpm fidelity` | does xterm still render a TUI correctly (spike page, no sessions) |
| `pnpm claude-check` | does the real `claude` CLI behave inside Helm's terminal |
| `pnpm packaging-check` | first run, the built artefacts and the personal-path audit; only when the owner asks |

Most take `--only=<group>`; the `GROUPS` constant in each driver is the list.
