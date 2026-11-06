#!/bin/bash
# The README's headline screenshot, in one go: boots the Linux test VM (1920x1200) if it isn't
# up, sets its session up the way the screenshot shows (the Everforest Dark skin in the bar and
# in Teleprinter, a wallpaper, gaps on, the bar's default layout), opens Teleprinter floating
# at its own 1280x800, which fits fastfetch's output with the Sector F logo, and saves the screen.
#
#   vm-linux/screenshot.sh [out.png]      default docs/screenshot.png
#
# WALLPAPER=path.jpg  what goes behind it (default: the forest the README has used)
# NO_RESTART=1  don't set the session up again: only take the picture
set -euo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(dirname "$HERE")
OUT=${1:-$REPO/docs/screenshot.png}
WALLPAPER=${WALLPAPER:-$HOME/Pictures/Wallpapers/ben-den-engelsen-q0THLEGsq5I-unsplash.jpg}
VM=$HERE/vm.sh
ASSETS=$HERE/screenshot

if ! "$VM" ssh true 2>/dev/null; then
    "$VM" start
fi

if [ -z "${NO_RESTART:-}" ]; then
    "$VM" ssh 'command -v fastfetch >/dev/null || sudo apt-get install -y fastfetch >/dev/null 2>&1'

    # Everything below is the guest's config, written fresh, so it's the same every time.
    "$VM" ssh 'mkdir -p ~/.config/patina-hud ~/.config/teleprinter ~/.config/fastfetch ~/Pictures/Wallpapers'
    if [ -f "$WALLPAPER" ]; then
        "$VM" ssh 'cat > ~/Pictures/Wallpapers/screenshot.jpg' < "$WALLPAPER"
        wallpaper='wallpaper = "/home/patina/Pictures/Wallpapers/screenshot.jpg"'
    else
        wallpaper=''
    fi
    "$VM" ssh "cat > ~/.config/patina-hud/config.toml" <<EOF
skin = "Everforest Dark"
$wallpaper
wallpaper_fit = "centre"
EOF
    "$VM" ssh 'cat > ~/.config/teleprinter/config.toml' < "$ASSETS/teleprinter.toml"
    "$VM" ssh 'cat > ~/.config/fastfetch/config.jsonc' < "$ASSETS/fastfetch/config.jsonc"
    "$VM" ssh 'cat > ~/.config/fastfetch/logo.txt' < "$ASSETS/fastfetch/logo.txt"
    # Gaps on, whatever else the file has (it's reloaded live).
    "$VM" ssh 'f=~/.config/patina-compositor/config.toml
        grep -q "^\[gaps\]" "$f" || printf "\n[gaps]\nenabled = true\nsize = 8\nradius = 8\n" >> "$f"'

    "$VM" restart
fi

# The session takes a moment to make its socket after a restart.
for _ in $(seq 1 60); do
    "$VM" ssh 'pid=$(pgrep -f release/patina-compositor | head -1); [ -n "$pid" ] &&
        d=$(tr "\0" "\n" < /proc/$pid/environ | sed -n "s/^XDG_RUNTIME_DIR=//p") &&
        ls "$d" | grep -qE "^wayland-[0-9]+$" && pgrep -f release/patina-hud >/dev/null' && break
    sleep 1
done
sleep 3

# The tiler reopens an app's window where it last was, filled if it closed filled (ADR-0065), so
# a Teleprinter left over from an earlier run is closed and forgotten: it floats at its own size.
"$VM" ssh 'pkill -x teleprinter; sleep 1; pkill -x patina-tiler; rm -f ~/.local/state/patina-tiler/windows.toml'
sleep 3

# Teleprinter, started as the desktop would start it. The sleep lets the window settle at its
# size before fastfetch asks the terminal how wide it is.
"$VM" ssh "pid=\$(pgrep -f 'release/patina-compositor' | head -1)
    export \$(tr '\\0' '\\n' < /proc/\$pid/environ | grep -E '^(DBUS_SESSION_BUS_ADDRESS|XDG_RUNTIME_DIR)=')
    export WAYLAND_DISPLAY=\$(ls \$XDG_RUNTIME_DIR | grep -E '^wayland-[0-9]+\$' | head -1)
    nohup /teleprinter/target/release/teleprinter -e sh -c 'sleep 4; clear; fastfetch; exec bash' \
        >/tmp/screenshot-teleprinter.log 2>&1 </dev/null &"
sleep 12
"$VM" shot "$OUT"
echo "saved $OUT"
