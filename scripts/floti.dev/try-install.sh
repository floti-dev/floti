# The release dry run's test of floti.dev's install.sh, run for real in a CI
# runner or container and nowhere else: it installs floti in this user's own
# folder and runs `floti setup`, as a person's `curl | sh` does.
#
#   sh try-install.sh <served folder> <version> <target>
#
# <served folder> holds the run's floti files and an install.sh made with
# `floti-dev.mjs scripts --release-url http://127.0.0.1:8765`. A local server
# serves it there. First the script is cut off at ten places, each piped into
# sh with a HOME of its own, which must stay empty, and the server must have
# been asked for no floti file. Then the whole script is piped into sh as
# `curl -fsSL <url>/install.sh | sh`. floti must be at ~/.local/bin/floti and
# `floti version` must name <version> and <target>, and the script must have
# reached setup, whatever setup made of the runner: a host started, or a
# failure of setup's own, such as root being refused. Needs curl and python3.

set -eu

served=$1
version=$2
target=$3
port=8765
url="http://127.0.0.1:$port"
summary=${GITHUB_STEP_SUMMARY:-/dev/null}

fail() {
  echo "::error::install.sh on $target: $1"
  exit 1
}

work=$(mktemp -d)
python3 -m http.server "$port" --bind 127.0.0.1 --directory "$served" >"$work/server.log" 2>&1 &
server=$!
trap 'kill "$server" 2>/dev/null || true; rm -rf "$work"' EXIT

tries=0
until curl -fsS -o /dev/null "$url/install.sh" 2>/dev/null; do
  tries=$((tries + 1))
  [ "$tries" -lt 200 ] || fail "the local server never answered."
  sleep 0.1
done

script="$served/install.sh"
size=$(wc -c <"$script" | tr -d ' ')
last=$(tail -n 1 "$script" | wc -c | tr -d ' ')
for cut in $((size / 10)) $((size * 2 / 10)) $((size * 3 / 10)) $((size * 4 / 10)) $((size * 5 / 10)) \
  $((size * 6 / 10)) $((size * 7 / 10)) $((size * 8 / 10)) $((size * 9 / 10)) $((size - last)); do
  home="$work/cut-$cut"
  mkdir "$home"
  head -c "$cut" "$script" | HOME="$home" sh >"$work/cut.out" 2>&1 || true
  if [ -n "$(ls -A "$home")" ]; then
    fail "cut off after $cut of $size bytes, it ran: it left $(ls -A "$home" | tr '\n' ' ')in HOME."
  fi
done
if grep -q "GET /floti-" "$work/server.log"; then
  fail "a script cut off asked the server for a floti file."
fi
echo "install.sh cut off at ten places, the last just before its last line: each ran nothing."

out="$work/install.out"
status=0
curl -fsSL "$url/install.sh" | sh >"$out" 2>&1 || status=$?
cat "$out"

floti="$HOME/.local/bin/floti"
if grep -q '^floti install: ' "$out"; then
  fail "it did not install floti: $(grep '^floti install: ' "$out" | head -n 1)"
fi
grep -q "^floti $version is at $floti\.$" "$out" || fail "it did not say floti is at $floti."
[ -x "$floti" ] || fail "$floti is not there, or not executable."
said=$("$floti" version)
case "$said" in
  "floti $version protocol "*" ${target%-*} ${target#*-}") ;;
  *) fail "$floti version said \"$said\", not floti $version for ${target%-*} ${target#*-}." ;;
esac
setup=$(grep -E '^(The Floti host |floti setup: )' "$out" | tail -n 1 || true)
[ -n "$setup" ] || fail "it never reached floti setup (exit $status)."

echo "install.sh on $target installed $said at $floti; setup said: $setup (exit $status)"
{
  echo "- \`install.sh\` on \`$target\` ($(uname -sm)): cut off at ten places it ran nothing; whole, it installed \`$said\` at \`$floti\`, and setup said \"$setup\" (exit $status)"
} >>"$summary"
