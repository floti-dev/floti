# The release dry run's test of floti.dev's install.sh, run for real in a CI
# runner or container and nowhere else: it installs floti in this user's own
# folder and runs `floti setup`, as a person's `curl | sh` does.
#
#   sh try-install.sh <served folder> <version> <target> [adds-path-line]
#
# <served folder> holds the run's floti files and an install.sh made with
# `floti-dev.mjs scripts --release-url http://127.0.0.1:8765`. A local server
# serves it there. First the script is cut off at ten places, each piped into
# sh with a HOME of its own, which must stay empty, and the server must have
# been asked for no floti file. Then the whole script is piped into sh as
# `curl -fsSL <url>/install.sh | sh`. floti must be at ~/.local/bin/floti and
# `floti version` must name <version> and <target>, and the script must have
# reached setup, whatever setup made of the runner: a host started, or a
# failure of setup's own, such as root being refused. Then floti must be on
# PATH: a new login shell of the user's own kind ($SHELL), started from a
# clean environment, must find it, through the line setup added to the
# startup file it named or, without `adds-path-line`, one there already. With
# `adds-path-line` setup must have added floti's line, so that new shell
# finds floti through that line alone. A shell floti knows no startup file
# of must have been told so; root, whom setup refuses, gets no line at all.
# Needs curl and python3.

set -eu

# It installs floti on this computer, registers its host and adds a line to
# a startup file, so it runs only on a CI runner.
if [ "${GITHUB_ACTIONS:-}" != true ]; then
  echo "try-install.sh installs Floti on this computer, so it runs only in GitHub Actions (GITHUB_ACTIONS=true); nothing was changed."
  exit 1
fi

served=$1
version=$2
target=$3
adds=false
case "${4:-}" in
  adds-path-line) adds=true ;;
  "") ;;
  *) echo "::error::try-install.sh knows no \"$4\"; it knows adds-path-line." && exit 1 ;;
esac
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

# floti on PATH. For a shell floti writes a startup file of, setup says which
# file it added floti's line to, or says nothing where that shell's startup
# files had the folder on PATH already, which `adds-path-line` refuses; either
# way a new login shell of the user's own kind ($SHELL), started from a clean
# environment whose PATH has only the system's folders, so that it has
# nothing of this job's PATH and only what its startup files add, must find
# this floti. For any other shell, or none, setup says it knows no startup
# file and changes none. Root is refused by setup before its PATH step.
shell=${SHELL:-}
path_said=$(grep -E '^(Put .* on your PATH|.* is not on your PATH, and Floti |Floti could not put )' "$out" | head -n 1 || true)
if [ "$(id -u)" = 0 ]; then
  shell_kind=root
else
  shell_kind=${shell##*/}
fi
case "$shell_kind" in
  root)
    [ -z "$path_said" ] || fail "setup, refused as root, still said: $path_said"
    [ "$adds" = false ] || fail "adds-path-line was asked of root, whom setup refuses."
    on_path="setup, refused as root, changed no startup file"
    ;;
  bash | zsh | fish | sh | dash | ash | ksh | mksh)
    case "$path_said" in
      "Put "*" on your PATH in "*)
        changed=$(printf '%s\n' "$path_said" | sed -E 's/^Put .* on your PATH in (.*), for (terminals|logins) .*$/\1/')
        [ -f "$changed" ] || fail "setup said it put floti on PATH in $changed, which is not there."
        grep -qF '# added by floti' "$changed" || fail "setup said it put floti on PATH in $changed, which has no line of floti's."
        on_path="setup said \"$path_said\""
        ;;
      "")
        [ "$adds" = false ] || fail "with SHELL=$shell, setup added no PATH line, which this job must show; a startup file of ${shell##*/} had floti's folder on PATH already."
        on_path="${shell##*/}'s startup files had floti's folder on PATH already, and setup changed none"
        ;;
      *) fail "with SHELL=$shell, setup said: $path_said" ;;
    esac
    user=$(id -un)
    found=$(cd / && env -i HOME="$HOME" USER="$user" LOGNAME="$user" SHELL="$shell" PATH=/usr/bin:/bin "$shell" -l -c 'command -v floti' </dev/null 2>"$work/shell.err" || true)
    [ "$found" = "$floti" ] || fail "a new ${shell##*/} login shell from a clean environment found floti at \"$found\", not $floti. $(tail -n 1 "$work/shell.err")"
    on_path="$on_path; a new ${shell##*/} login shell from a clean environment runs $found"
    ;;
  *)
    [ "$adds" = false ] || fail "adds-path-line was asked with SHELL=${shell:-not set}, a shell floti writes no startup file of."
    case "$path_said" in
      *" is not on your PATH, and Floti does not know which startup file your shell reads"*)
        on_path="with SHELL=${shell:-not set}, setup said it knows no startup file of that shell and changed none"
        ;;
      "") on_path="with SHELL=${shell:-not set}, floti's folder was on PATH already" ;;
      *) fail "with SHELL=${shell:-not set}, setup did not say it knows no startup file: \"$path_said\"" ;;
    esac
    ;;
esac

echo "install.sh on $target installed $said at $floti; setup said: $setup (exit $status); PATH: $on_path."
{
  echo "- \`install.sh\` on \`$target\` ($(uname -sm)): cut off at ten places it ran nothing; whole, it installed \`$said\` at \`$floti\`, and setup said \"$setup\" (exit $status); PATH: $on_path"
} >>"$summary"
