# Security policy

## Reporting a vulnerability

Please report it privately, through
[GitHub's private vulnerability reporting](https://github.com/floti-dev/floti/security/advisories/new)
(**Security → Report a vulnerability** on the repository). Don't post it
anywhere public first.

A useful report says:

- the version (`floti --version`, or **Settings → Updates** in the app) and
  the OS and CPU
- what an attacker can do, and from where: another user on the same computer,
  another computer, a web page in the browser pane, a plugin, etc.
- the steps to reproduce it

## What to expect

Floti is a personal project that I work on in my own time. I read every report,
but I don't promise a response time. A fix ships in a new release, and only the
latest release gets fixes; older versions aren't patched. Once a fix is out, I
may publish an advisory describing the problem, and I'll credit you there if
you'd like.

## Scope

In scope is everything this repository builds and publishes: floti (the host,
the session keeper and the command line), the desktop app, the plugin SDK, the
install scripts and update manifest on floti.dev, and the release signing.
The boundaries Floti means to hold are a good place to look, for example:

- anything outside your OS account reaching the host, its local socket, or the
  agent tools it serves on `127.0.0.1`
- a page in the browser pane escaping its view, or reaching what the pane's
  settings refuse
- a plugin reaching hosts its manifest doesn't name, or reading a secret back
- an update or install going through without a valid signature or SHA-256
- Floti reading or storing an agent's credentials or an SSH key, or letting a
  browser cookie out of the browser pane

Out of scope:

- vulnerabilities in Claude Code itself, which go to Anthropic
- third-party plugins, which their authors maintain
- anything that needs an attacker who already runs code as your OS user. Your
  OS login is the access to Floti, and a plugin's programs run with your rights
  by design ([docs/SECURITY.md](../docs/SECURITY.md) has the reasoning).

## Verifying a release

Every file a release carries has a minisign signature beside it, checked
against the public key at
[`crates/host/src/update/minisign.pub`](../crates/host/src/update/minisign.pub),
and a GitHub build attestation tying it to the run of
[`release.yml`](workflows/release.yml) that built it from the tagged commit.
`floti update` and `floti add desktop` check both the signature and the
SHA-256 before anything runs.
