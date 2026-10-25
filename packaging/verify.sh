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
    package=${artifacts[0]}
    [[ $package == *.deb ]]
    dpkg-deb -I "$package"
    dpkg-deb -c "$package"
    dpkg-deb -x "$package" "$tmp/package"
    binary="$tmp/package/usr/bin/workshop"
    desktop-file-validate "$tmp/package/usr/share/applications/workshop.desktop"
    python3 packaging/verify-elf.py "$binary" "$package"
    if [[ ${CI:-} == true ]] && command -v apt-get >/dev/null; then
      sudo apt-get install -y -qq "$(pwd)/$package"
      binary=/usr/bin/workshop
    else
      # Developer hosts verify/launch the extracted installed tree without a system install.
      echo 'Not Debian CI: launching the extracted package; system installation is only checked on Debian CI.' >&2
    fi
    check_cli
    dbus-run-session -- xvfb-run -a -s '-screen 0 1440x1000x24' bash packaging/smoke-linux.sh "$binary" "$tmp"
    ;;
  Darwin)
    package=${artifacts[0]}
    [[ $package == *.dmg ]]
    mountpoint="$tmp/mounted"
    hdiutil attach -readonly -nobrowse -mountpoint "$mountpoint" "$package"
    app="$mountpoint/Holuto Workshop.app"
    plutil -lint "$app/Contents/Info.plist"
    mkdir -p "$tmp/Applications"
    ditto "$app" "$tmp/Applications/Holuto Workshop.app"
    app="$tmp/Applications/Holuto Workshop.app"
    binary="$app/Contents/MacOS/workshop"
    [[ $(lipo -archs "$binary") == "$(uname -m)" ]]
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
