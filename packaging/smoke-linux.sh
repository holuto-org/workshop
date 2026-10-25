#!/usr/bin/env bash
# verify.sh's fresh state, no configured printer, no Start action: never touches a serial device.
set -euo pipefail
binary=$1
state=$2
export WINIT_UNIX_BACKEND=x11
unset WAYLAND_DISPLAY
mkfifo "$state/display"
exec 3<>"$state/display"
Xvfb -displayfd 3 -screen 0 1440x1000x24 -nolisten tcp &
xvfb=$!
pid=
cleanup() {
  if [[ -n $pid ]] && kill -0 "$pid" 2>/dev/null; then
    kill -TERM "$pid"
    if wait "$pid"; then :; else [[ $? == 143 ]]; fi
  fi
  kill -TERM "$xvfb"
  if wait "$xvfb"; then :; else [[ $? == 143 ]]; fi
}
trap cleanup EXIT
# displayfd reports a ready server, not a guessed startup delay.
read -r -t 20 display <&3
exec 3>&-
DISPLAY=":$display"
export DISPLAY
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
geometry=$(xdotool getwindowgeometry --shell "$window")
# --shell prints WINDOW/X/Y/WIDTH/HEIGHT/SCREEN assignments.
eval "$geometry"
[[ $WIDTH -ge 1040 && $HEIGHT -ge 720 ]]
printf 'Verified main window: %s (%s × %s)\n' "$window" "$WIDTH" "$HEIGHT"
