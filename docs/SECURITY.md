# Security

What Floti treats as a secret, how a device reaches a host, and what the
browser pane may do. The network posture is in
[PACKAGING.md](PACKAGING.md#network-posture), stated identically in README,
SPEC 5 and the `update:check` comment in `shared/ipc.ts`; change all four
together. The agent tools listener's token and origin rules are in
[AGENT-TOOLS.md](AGENT-TOOLS.md#the-listener).

## Agent and GitHub credentials

- Never handle or store any agent's credential, or GitHub's. Each agent
  backend lists its credential paths (`credential_paths` in the host's
  `AgentBackend`), and nothing opens them. A sign-in is checked by the agent's
  own status command where it has one, by an artefact existing where it does
  not, and is Unknown otherwise. For Claude Code that is an artefact existing
  (`.credentials.json`, `ANTHROPIC_API_KEY`, the onboarding record in
  `.claude.json`). Never open those, `hosts.yml`, an agent's or GitHub's
  keyring entry or `GH_TOKEN`,
  and never run a command that prints a credential. The remedy for "not signed
  in" is a sentence naming the agent's login command (`claude` for Claude
  Code).
- `~/.claude/sessions` keeps a `<pid>.<sha256>.key` credential beside every
  `<pid>.json`. The host's Claude Code status reader
  (`agent/claude_code/status.rs`) reads `.json` files only; never widen it.
- `CLAUDE_CONFIG_DIR` moves credentials too, so a session pointed at a fixture
  home cannot sign in.

## Browser pane

- The `persist:helm-browser` partition holds the user's cookies and logins.
  Nothing reads it: main sets its permission, download and certificate
  handlers and opens pages in it, and the only call on what it stores is
  `clearStorageData`.
- Every navigation goes through `browserReachAllows` in `@helm/core`; an
  agent's restrictions are composed by `agentReach`. `file:` and custom schemes
  are refused there.
- Self-signed certificates are accepted for loopback only, and there is no
  `certificate-error` handler. Downloads are refused and handed to the system
  browser. Every permission is denied but clipboard writing and fullscreen,
  and those only to a page in the Browser tab that has the keyboard. Only the
  address bar searches, on Enter, with the engine in `browserSearch` (`off`
  refuses); nothing is sent while typing.

## The plugin secret store

- The plugin secret store (`crates/host/src/plugins/secrets`) is the one
  place Helm keeps a secret of the user's, and only one the user typed into
  it; Floti's own keys are below. Each value
  is sealed by this OS's store for the user (`platform/secret_seal`) and
  refused when it cannot be; a value is filled in by the host, for an origin
  and a plugin the user allowed, and never sent to any client. Helm never reads a Claude or GitHub credential to put there.

## Access, keys and devices

- The user's login is the access to Floti. The host serves whatever reaches
  its local socket, which only the user's OS account can open, once its Noise
  handshake completes: a client on the host's computer, or one that comes
  through `floti connect` over the user's own ssh or command. Every such
  device is served every channel, as the user could at the host; Floti adds
  no approval, code, role or prompt of its own, and keeps no list of allowed
  devices. The host keeps its host key (`host.key`) in its data folder and
  no device list; at start it deletes any `devices.json` and `enrolment.json`
  in its data folder (`RETIRED_FILES` in `server/mod.rs`). Each client
  keeps its device key (`device.key`) and the host key it pinned
  (`host.pub`) at its first connection, and refuses a host that proves
  another, naming the host, until the user removes it and adds it again.
  Each is an owner-only file (`platform/private_files`): `0600` on macOS and
  Linux, an access list naming only the user on Windows. The device key
  names the device in the attached list (`devices:attached`, Settings >
  Devices) and records which sessions it started, so quitting a client ends
  only its own. Disconnect there (`devices:disconnect`) closes every
  connection of a device's key to that host at once and leaves its sessions
  running; taking its SSH key away is what keeps it out. A private key is
  never logged, printed, sent to another client or put in a session's
  environment; logs name a key by its fingerprint, and a client that fails
  the handshake is closed without a byte. Keys and the Noise sessions they
  prove come from snow, through `floti_protocol::noise`; never hand-rolled
  crypto.
