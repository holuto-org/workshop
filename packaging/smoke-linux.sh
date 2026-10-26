#!/usr/bin/env bash
# Runs under xvfb-run; always owns fresh unpaired state, even when called directly.
set -euo pipefail
binary=$1
state=$(mktemp -d "$2/smoke.XXXXXX")
mkdir -p "$state/home" "$state/state" "$state/config"
export HOME="$state/home" XDG_STATE_HOME="$state/state" XDG_CONFIG_HOME="$state/config"
export WINIT_UNIX_BACKEND=x11
unset WAYLAND_DISPLAY
cleanup() {
  if [[ -n ${pid:-} ]] && kill -0 "$pid" 2>/dev/null; then
    kill -TERM "$pid"
    wait "$pid" || [[ $? == 143 ]]
  fi
  rm -rf -- "$state"
}
trap cleanup EXIT
# Production packages embed a live DSN: opt out before any GUI initialization.
"$binary" config set --diagnostics off
"$binary" >"$state/app.log" 2>&1 &
pid=$!
if ! window=$(timeout 20 xdotool search --all --sync --onlyvisible --pid "$pid" --name '^Holuto Workshop$'); then
  cat "$state/app.log" >&2
  echo 'Main application window did not appear' >&2
  exit 1
fi
kill -0 "$pid"
xprop -id "$window" WM_CLASS | grep -F '"workshop"'
if xdotool search --all --onlyvisible --pid "$pid" --name 'błąd'; then
  echo 'Startup error window appeared' >&2; exit 1
fi
# --shell prints WINDOW/X/Y/WIDTH/HEIGHT/SCREEN assignments.
eval "$(xdotool getwindowgeometry --shell "$window")"
[[ $WIDTH -ge 1040 && $HEIGHT -ge 720 ]]
printf 'Verified main window: %s (%s × %s)\n' "$window" "$WIDTH" "$HEIGHT"
