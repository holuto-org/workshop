#!/usr/bin/env bash
# Run by verify.sh under xvfb-run with fresh state, no configured printer and no Start action: never touches a serial device.
set -euo pipefail
binary=$1
state=$2
export WINIT_UNIX_BACKEND=x11
unset WAYLAND_DISPLAY
"$binary" >"$state/app.log" 2>&1 &
pid=$!
cleanup() {
  if kill -0 "$pid" 2>/dev/null; then
    kill -TERM "$pid"
    wait "$pid" || [[ $? == 143 ]]
  fi
}
trap cleanup EXIT
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
