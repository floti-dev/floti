# The release workflow's check of what `floti setup` left on a macOS or Linux
# runner or container after floti was installed in this user's own folder,
# run there and nowhere else. It reads the computer and changes nothing.
#
#   sh check-setup.sh <how> <version> <what installed it>
#
# floti must be at ~/.local/bin/floti and `floti version` there must name
# <version>. <how> is what setup must have left:
#
#   launch-agent      ~/Library/LaunchAgents/dev.floti.host.plist runs that
#                     floti, and launchd runs it in this user's GUI domain.
#   user-unit         ~/.config/systemd/user/floti.service runs that floti,
#                     and the user's systemd has it enabled and active.
#   start-on-connect  the data folder holds the start-on-connect marker, and
#                     no unit was written: a WSL distro with no systemd.
#   refused           neither a unit nor the marker: setup refused (root).
#
# Each missing thing fails with one line naming it.

set -eu

how=$1
version=$2
by=$3
summary=${GITHUB_STEP_SUMMARY:-/dev/null}

fail() {
  echo "::error::$by: $1"
  exit 1
}

floti="$HOME/.local/bin/floti"
[ -x "$floti" ] || fail "$floti is not there, or not executable."
said=$("$floti" version) || fail "$floti version did not answer."
case "$said" in
  "floti $version "*) ;;
  *) fail "$floti version said \"$said\", not floti $version." ;;
esac

unit="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/floti.service"
marker="${XDG_DATA_HOME:-$HOME/.local/share}/floti/start-on-connect"

case "$how" in
  launch-agent)
    plist="$HOME/Library/LaunchAgents/dev.floti.host.plist"
    [ -f "$plist" ] || fail "setup wrote no LaunchAgent at $plist."
    grep -qF "<string>$floti</string>" "$plist" || fail "the LaunchAgent at $plist does not run $floti."
    service="gui/$(id -u)/dev.floti.host"
    loaded=$(launchctl print "$service" 2>/dev/null) || fail "launchd has no $service loaded."
    printf '%s\n' "$loaded" | grep -q 'state = running' || fail "launchd has $service loaded but not running."
    found="the LaunchAgent dev.floti.host, running $floti in $service"
    ;;
  user-unit)
    [ -f "$unit" ] || fail "setup wrote no systemd user unit at $unit."
    # Setup quotes the path as one systemd word, escaping \ and " with a
    # backslash and % and $ by doubling them; an unquoted path is the same
    # command to systemd when it needs no quoting.
    quoted=\"$(printf '%s' "$floti" | sed -e 's/[\\"]/\\&/g' -e 's/%/%%/g' -e 's/\$/$$/g')\"
    exec=$(grep '^ExecStart=' "$unit" || true)
    case "$exec" in
      "ExecStart=$quoted host" | "ExecStart=$floti host") ;;
      *) fail "the user unit at $unit does not run $floti host: it has \"${exec:-no ExecStart line}\"." ;;
    esac
    enabled=$(systemctl --user is-enabled floti.service 2>/dev/null || true)
    [ "$enabled" = enabled ] || fail "the user unit floti.service is not enabled (systemctl says \"$enabled\")."
    active=$(systemctl --user is-active floti.service 2>/dev/null || true)
    [ "$active" = active ] || fail "the user unit floti.service is not active (systemctl says \"$active\")."
    found="the user unit floti.service, enabled and active, running $floti host"
    ;;
  start-on-connect)
    [ -f "$marker" ] || fail "setup left no start-on-connect marker at $marker."
    [ ! -e "$unit" ] || fail "setup wrote a user unit at $unit where systemd does not run."
    found="the start-on-connect marker at $marker, and no unit"
    ;;
  refused)
    [ ! -e "$unit" ] || fail "setup was refused but wrote a user unit at $unit."
    [ ! -e "$marker" ] || fail "setup was refused but left a start-on-connect marker at $marker."
    found="no unit and no marker, as a refused setup leaves"
    ;;
  *)
    fail "check-setup.sh knows no \"$how\"; it knows launch-agent, user-unit, start-on-connect and refused."
    ;;
esac

echo "$by: $said answers from $floti; setup left $found."
echo "- $by: \`$said\` answers from \`$floti\`; setup left $found" >>"$summary"
