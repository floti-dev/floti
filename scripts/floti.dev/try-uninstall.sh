# The release workflow's test of `floti uninstall` on a macOS or Linux runner
# or container, after an install and check-setup.sh, run there and nowhere
# else: it removes what the install put there.
#
#   sh try-uninstall.sh <how> <by>
#
# <how> is what check-setup.sh checked setup left (launch-agent, user-unit,
# start-on-connect or refused). Three runs, each of which must exit 0:
#
# 1. `floti uninstall` from ~/.local/bin/floti. floti and its node folder,
#    the sign-in entry and the desktop app where it was installed must be
#    gone, and the data folder, where setup made one, must stay. Where a host
#    ran, it says it stopped it. floti's PATH line (`# added by floti`) must
#    be in no startup file of any shell floti knows, and where the install
#    added one, it says it removed it. Lingering is left on, and the desktop
#    app's settings are kept: neither is checked.
# 2. `floti uninstall --purge`, and the data folder must be gone.
# 3. `floti uninstall` again, which changes nothing: no line says anything
#    was removed, stopped or closed, and the sign-in entry, the desktop app
#    and the data each say "not found".
#
# floti removes itself in the first run, so the other two run from a copy of
# it made beforehand outside its install place. A floti from elsewhere
# removes what the install put there and leaves itself ("floti: left as it
# is"), which is what the third run's last line says. Each missing thing
# fails with one line naming it.

set -eu

# It uninstalls Floti from this computer, so it runs only on a CI runner.
if [ "${GITHUB_ACTIONS:-}" != true ]; then
  echo "try-uninstall.sh uninstalls Floti from this computer, so it runs only in GitHub Actions (GITHUB_ACTIONS=true); nothing was changed."
  exit 1
fi

how=$1
by=$2
summary=${GITHUB_STEP_SUMMARY:-/dev/null}

fail() {
  echo "::error::$by: $1"
  exit 1
}

floti="$HOME/.local/bin/floti"
case "$(uname -s)" in
  Darwin)
    data="$HOME/Library/Application Support/Floti"
    entry="$HOME/Library/LaunchAgents/dev.floti.host.plist"
    app="$HOME/Applications/Helm.app"
    ;;
  *)
    data="${XDG_DATA_HOME:-$HOME/.local/share}/floti"
    entry="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/floti.service"
    app="$HOME/Applications/Helm.AppImage"
    ;;
esac
marker="$data/start-on-connect"

[ -x "$floti" ] || fail "there is no $floti to uninstall."

# The startup files, of every shell floti knows, holding floti's PATH line.
with_path_line() {
  for file in .bash_profile .bash_login .profile .bashrc .zshenv .zprofile .zshrc .zlogin .config/fish/config.fish; do
    if [ -f "$HOME/$file" ] && grep -qF '# added by floti' "$HOME/$file"; then
      printf '%s ' "$HOME/$file"
    fi
  done
}
had_path_line=$(with_path_line)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
copy="$work/floti"
cp "$floti" "$copy"
had_app=false
[ -e "$app" ] && had_app=true
had_data=false
[ -d "$data" ] && had_data=true
case "$how" in
  refused) ;;
  *) [ "$had_data" = true ] || fail "setup made no data folder at $data." ;;
esac

# Runs `<floti> uninstall <args>`, shows what it said, and fails unless it
# exited 0. What it said is in $work/said.
uninstall() {
  program=$1
  shift
  status=0
  "$program" uninstall "$@" >"$work/said" 2>&1 || status=$?
  sed 's/^/  /' "$work/said"
  [ "$status" -eq 0 ] || fail "floti uninstall $* exited $status: $(tail -n 1 "$work/said")"
}

said() {
  grep -q "$1" "$work/said" || fail "floti uninstall $2 did not say \"$1\"."
}

echo "floti uninstall:"
uninstall "$floti"
[ ! -e "$floti" ] || fail "floti uninstall left $floti."
[ ! -e "$HOME/.local/bin/node" ] || fail "floti uninstall left $HOME/.local/bin/node."
case "$how" in
  launch-agent | user-unit)
    said '^host: stopped' ''
    said '^sign-in entry: removed' ''
    [ ! -e "$entry" ] || fail "floti uninstall left the sign-in entry $entry."
    ;;
  start-on-connect)
    said '^sign-in entry: removed' ''
    [ ! -e "$marker" ] || fail "floti uninstall left the start-on-connect marker $marker."
    ;;
  refused)
    said '^sign-in entry: not found' ''
    ;;
  *)
    fail "try-uninstall.sh knows no \"$how\"; it knows launch-agent, user-unit, start-on-connect and refused."
    ;;
esac
if [ "$how" = launch-agent ] && launchctl print "gui/$(id -u)/dev.floti.host" >/dev/null 2>&1; then
  fail "launchd still has gui/$(id -u)/dev.floti.host loaded."
fi
if [ "$how" = user-unit ] && [ "$(systemctl --user is-enabled floti.service 2>/dev/null || true)" = enabled ]; then
  fail "the user's systemd still has floti.service enabled."
fi
if [ "$how" = user-unit ] && [ "$(systemctl --user is-active floti.service 2>/dev/null || true)" = active ]; then
  fail "the user's systemd still has floti.service active."
fi
# The host it said it stopped, by its command line, given up to ten seconds
# to exit; pgrep is not in every container, where no host ran.
if command -v pgrep >/dev/null 2>&1; then
  tries=0
  while pgrep -f "^$floti host" >/dev/null 2>&1; do
    tries=$((tries + 1))
    [ "$tries" -lt 100 ] || fail "after floti uninstall, $floti host still runs (pid $(pgrep -f "^$floti host" | head -n 1))."
    sleep 0.1
  done
fi
left=$(with_path_line)
[ -z "$left" ] || fail "floti uninstall left floti's PATH line in $left"
if [ -n "$had_path_line" ]; then
  said '^PATH entry: removed' ''
else
  said '^PATH entry: not found' ''
fi
if [ "$had_app" = true ]; then
  said '^desktop app: removed' ''
  [ ! -e "$app" ] || fail "floti uninstall left the desktop app at $app."
fi
if [ "$had_data" = true ]; then
  said '^data: kept' ''
  [ -d "$data" ] || fail "floti uninstall removed the data folder $data, which only --purge removes."
else
  said '^data: not found' ''
fi

echo "floti uninstall --purge, from a copy:"
uninstall "$copy" --purge
[ ! -e "$data" ] || fail "floti uninstall --purge left the data folder $data."
if [ "$had_data" = true ]; then said '^data: removed' '--purge'; else said '^data: not found' '--purge'; fi

echo "floti uninstall again, from a copy:"
uninstall "$copy"
if grep -E '^[^:]+: (removed|stopped|closed)' "$work/said" >"$work/changed"; then
  fail "a second floti uninstall changed something: $(head -n 1 "$work/changed")"
fi
for line in 'sign-in entry: not found' 'desktop app: not found' 'data: not found'; do
  said "^$line\$" 'run again'
done
said '^floti: left as it is' 'run again'

removed="floti and the sign-in entry"
[ "$had_app" = true ] && removed="floti, the sign-in entry and the desktop app"
[ "$how" = refused ] && removed="floti, with no sign-in entry there"
[ -n "$had_path_line" ] && removed="$removed, and floti's PATH line from ${had_path_line% }"
kept="kept the data"
[ "$had_data" = true ] || kept="found no data"
echo "$by: floti uninstall removed $removed and $kept; --purge left no data; run again it found nothing."
echo "- $by: \`floti uninstall\` removed $removed and $kept; \`--purge\` left no data folder; a second run exited 0 and found nothing" >>"$summary"
