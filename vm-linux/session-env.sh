#!/bin/sh
# Runs its arguments in the Patina session's environment, as the user
# running it: its session bus (the compositor's services are on it), runtime
# folder and Wayland display. `vm.sh session CMD` is this, in the guest.
set -eu
pid=$(pgrep -u "$(id -u)" -x patina-composit | head -1)
[ -n "$pid" ] || { echo "no Patina session is running" >&2; exit 1; }
for v in DBUS_SESSION_BUS_ADDRESS XDG_RUNTIME_DIR; do
    value=$(tr '\0' '\n' < "/proc/$pid/environ" | sed -n "s/^$v=//p")
    export "$v=$value"
done
WAYLAND_DISPLAY=$(ls "$XDG_RUNTIME_DIR" | grep -E '^wayland-[0-9]+$' | head -1)
export WAYLAND_DISPLAY
exec "$@"
