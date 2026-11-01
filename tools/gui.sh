#!/bin/sh
# Run a shell snippet against the app on a virtual display: `tools/gui.sh 'shot examples'`.
# Helpers inside: shot NAME (saves target/shots/NAME.png), click X Y, key KEYS, type TEXT.
# IMAGE=cadence-gui-arch runs it on current Arch GTK/libadwaita instead of Ubuntu 24.04.
# HOME is target/gui-home (wiped first unless KEEP=1), so real jobs are never touched.
set -eu
cd "$(dirname "$0")/.."
mkdir -p target/shots
[ "${KEEP:-0}" = 1 ] || rm -rf target/gui-home
mkdir -p target/gui-home
exec docker run --rm -u "$(id -u):$(id -g)" -v "$PWD:/w" -w /w -e HOME=/w/target/gui-home -e SNIPPET="$1" \
  -e GSK_RENDERER=cairo -e GTK_A11Y=none "${IMAGE:-cadence-gui}" sh -c '
  Xvfb :99 -screen 0 1280x900x24 >/dev/null 2>&1 & export DISPLAY=:99; sleep 1
  dbus-run-session -- sh -c "
    cp /bin/sleep /tmp/crond; /tmp/crond 600 &
    target/release/cadence 2>/tmp/err.log & sleep 3
    shot() { import -window root -crop 1050x760+0+0 +repage target/shots/\$1.png; }
    click() { xdotool mousemove \$1 \$2 click 1; sleep 1; }
    key() { xdotool key \$@; sleep 0.6; }
    type() { xdotool type --delay 20 \"\$1\"; sleep 0.5; }
    eval \"\$SNIPPET\"
    grep -v prefer-dark /tmp/err.log | head -5 || true
  "'
