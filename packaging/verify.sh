#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
artifacts=(release/*)
if [[ ${#artifacts[@]} != 1 || ! -f ${artifacts[0]} ]]; then
  echo 'Expected exactly one fresh package in release/; rerun packaging/build.sh.' >&2
  exit 1
fi
tmp=$(mktemp -d)
cleanup() {
  if [[ -n ${pid:-} ]] && kill -0 "$pid" 2>/dev/null; then
    kill -TERM "$pid"
    wait "$pid" || [[ $? == 143 ]]
  fi
  if [[ -n ${mountpoint:-} ]]; then hdiutil detach "$mountpoint"; fi
  rm -rf -- "$tmp"
}
trap cleanup EXIT

# Fresh unpaired configuration must fail safely without network or serial I/O.
check_cli() {
  local status=0
  mkdir -p "$tmp/home" "$tmp/state" "$tmp/config"
  export HOME="$tmp/home" XDG_STATE_HOME="$tmp/state" XDG_CONFIG_HOME="$tmp/config"
  "$binary" --help
  "$binary" check --json > "$tmp/check.json" || status=$?
  [[ $status == 3 ]]
  jq -e '.ok == false and .configured == false and .appRunning == false and (.errors | length > 0)' "$tmp/check.json"
}

case "$(uname -s)" in
  Linux)
    packages=(release/*.deb)
    [[ ${#packages[@]} == 1 && -f ${packages[0]} ]]
    dpkg-deb -I "${packages[0]}"
    dpkg-deb -c "${packages[0]}"
    dpkg-deb -x "${packages[0]}" "$tmp/package"
    binary="$tmp/package/usr/bin/workshop"
    [[ $(dpkg-deb -f "${packages[0]}" Package) == workshop ]]
    desktop-file-validate "$tmp/package/usr/share/applications/workshop.desktop"
    python3 packaging/verify-elf.py "$binary" "${packages[0]}"
    if command -v apt-get >/dev/null; then
      sudo apt-get install -y -qq "$(pwd)/${packages[0]}"
      binary=/usr/bin/workshop
    else
      # Non-Debian developer hosts can verify/launch the extracted installed tree.
      echo 'No apt-get: launching the extracted package; system installation is only checked on Debian CI.' >&2
    fi
    if LC_ALL=C grep -aFq 'workshop-simulator' "$binary"; then
      echo 'Simulator leaked into package' >&2; exit 1
    fi
    check_cli
    dbus-run-session -- bash packaging/smoke-linux.sh "$binary" "$tmp"
    ;;
  Darwin)
    packages=(release/*.dmg)
    [[ ${#packages[@]} == 1 && -f ${packages[0]} ]]
    mountpoint="$tmp/mounted"
    hdiutil attach -readonly -nobrowse -mountpoint "$mountpoint" "${packages[0]}"
    app="$mountpoint/Holuto Workshop.app"
    plutil -lint "$app/Contents/Info.plist"
    [[ $(/usr/libexec/PlistBuddy -c 'Print CFBundleExecutable' "$app/Contents/Info.plist") == workshop ]]
    [[ $(/usr/libexec/PlistBuddy -c 'Print CFBundleIdentifier' "$app/Contents/Info.plist") == com.holuto.workshop ]]
    [[ -L "$mountpoint/Applications" && $(readlink "$mountpoint/Applications") == /Applications ]]
    mkdir -p "$tmp/Applications"
    ditto "$app" "$tmp/Applications/Holuto Workshop.app"
    app="$tmp/Applications/Holuto Workshop.app"
    binary="$app/Contents/MacOS/workshop"
    [[ -x $binary ]]
    [[ $(lipo -archs "$binary") == "$(uname -m)" ]]
    if LC_ALL=C grep -aFq 'workshop-simulator' "$binary"; then
      echo 'Simulator leaked into package' >&2; exit 1
    fi
    codesign --verify --strict "$app"
    check_cli
    "$binary" &
    pid=$!
    # A launch stability check, not test synchronization with fiscal operations.
    sleep 5
    kill -0 "$pid"
    title=$(osascript -e "tell application \"System Events\" to get name of every window of first application process whose unix id is $pid")
    [[ $title == 'Holuto Workshop' ]]
    kill -TERM "$pid"
    wait "$pid" || [[ $? == 143 ]]
    pid=
    ;;
  MINGW* | MSYS* | CYGWIN*)
    pwsh.exe -NoProfile -ExecutionPolicy Bypass -File packaging/verify-windows.ps1
    ;;
  *) echo 'Unsupported OS' >&2; exit 1 ;;
esac
