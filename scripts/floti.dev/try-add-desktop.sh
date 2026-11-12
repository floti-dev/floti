# The release workflow's test of `floti add desktop` against a release that
# is published, run for real in a CI runner after `publish` and nowhere else.
# It cannot run in a dry run: floti fetches from the releases address built
# into it and checks the desktop files' list with the key built into it, and
# a dry run's files are neither there nor signed with that key.
#
#   bash try-add-desktop.sh <version> <tag> <target> <SHA-256 list>
#
# It downloads the release's floti for <target> as anybody does, with no
# token, checks it against <SHA-256 list> (the run's own, which `sign`
# checked and `publish` uploaded), runs `floti add desktop` with it, and
# checks that it exited 0 and the app is where add puts it, with its launcher
# entry:
#
#   windows  %LOCALAPPDATA%\Programs\Helm\Helm.exe and its uninstaller, and
#            the Start menu's Helm.lnk
#   macos    ~/Applications/Helm.app, whose folder is the Applications entry
#   linux    ~/Applications/Helm.AppImage, executable, its menu entry
#            applications/helm.desktop running it and its icon under
#            $XDG_DATA_HOME (~/.local/share)
#
# Each missing thing fails with one line naming it. Publishes nothing.

set -eu

version=$1
tag=$2
target=$3
sums=$4
summary=${GITHUB_STEP_SUMMARY:-/dev/null}
by="floti add desktop on $target"

fail() {
  echo "::error::$by: $1"
  exit 1
}

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d ' ' -f 1
}

case "$target" in
  windows-*) name="floti-$version-$target.exe" ;;
  *) name="floti-$version-$target" ;;
esac
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
floti="$work/$name"
url="https://github.com/$GITHUB_REPOSITORY/releases/download/$tag/$name"
env -u GH_TOKEN -u GITHUB_TOKEN curl --fail --silent --show-error --location --retry 3 --retry-all-errors \
  --output "$floti" "$url" || fail "$url did not download without a sign-in."
want=$(awk -v name="$name" '$2 == name || $2 == "*" name { print $1 }' "$sums")
[ -n "$want" ] || fail "the run's SHA-256 list names no $name."
[ "$(sha256 "$floti")" = "$want" ] || fail "$url is not the $name this run built."
chmod +x "$floti"

if [ "${target%%-*}" = windows ]; then
  echo "This job runs in Windows session $(powershell -NoProfile -Command '(Get-Process -Id $PID).SessionId'); add refuses session 0."
fi

status=0
"$floti" add desktop >"$work/add.out" 2>&1 || status=$?
cat "$work/add.out"
[ "$status" -eq 0 ] || fail "it exited $status: $(tail -n 1 "$work/add.out")"

case "$target" in
  windows-*)
    programs=$(cygpath -u "$LOCALAPPDATA")/Programs/Helm
    menu="$(cygpath -u "$APPDATA")/Microsoft/Windows/Start Menu/Programs/Helm.lnk"
    [ -f "$programs/Helm.exe" ] || fail "the app is not at $programs/Helm.exe."
    [ -f "$programs/Uninstall Helm.exe" ] || fail "the app has no uninstaller at $programs/Uninstall Helm.exe."
    [ -f "$menu" ] || fail "the Start menu has no entry at $menu."
    found="the app at \`$programs\`, its uninstaller, and the Start menu's \`Helm.lnk\`"
    ;;
  macos-*)
    app="$HOME/Applications/Helm.app"
    [ -f "$app/Contents/Info.plist" ] || fail "the app is not at $app (no Contents/Info.plist)."
    [ -x "$app/Contents/MacOS/Helm" ] || fail "$app has no executable Contents/MacOS/Helm."
    found="\`$app\`, in the Applications entry \`~/Applications\`"
    ;;
  linux-*)
    image="$HOME/Applications/Helm.AppImage"
    share="${XDG_DATA_HOME:-$HOME/.local/share}"
    entry="$share/applications/helm.desktop"
    icon="$share/icons/hicolor/256x256/apps/helm.png"
    [ -f "$image" ] && [ -x "$image" ] || fail "the app is not at $image, or not executable."
    [ -f "$entry" ] || fail "there is no menu entry at $entry."
    grep -qxF "Exec=\"$image\"" "$entry" || fail "the menu entry at $entry does not run $image."
    [ -f "$icon" ] || fail "the menu entry's icon is not at $icon."
    found="\`$image\`, its menu entry \`$entry\` and its icon"
    ;;
  *)
    fail "try-add-desktop.sh knows no target \"$target\"."
    ;;
esac

echo "$by: the release's $name installed $found."
echo "- $by: the published \`$name\` installed $found" >>"$summary"
