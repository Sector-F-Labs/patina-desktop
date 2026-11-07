#!/bin/sh
# The multi-monitor test VM's virtual monitors (ADR-0125), run in the guest as
# root by `vm.sh display ...`: vkms, the kernel's virtual display driver, with
# several connectors that can be plugged and unplugged.
#
#   vkms.sh up [N]            make the device with N connectors (default 3), the first
#                             connected as the machine's own panel, the rest not
#   vkms.sh plug N [PRESET]   connect connector N (0 is the first), as the monitor
#                             PRESET of edid.py says it is
#   vkms.sh unplug N          disconnect it
#   vkms.sh status            each connector, whether it's connected, and its monitor
#   vkms.sh install           a systemd unit that does `up` before the session starts
#
# vkms on this kernel reads no EDID, so the compositor is told one through
# PATINA_FAKE_EDID_DIR (the files in $EDIDS, one `Virtual-N.bin` for each
# connector); `plug` writes it before the connector is connected.
set -eu

CONFIGFS=/sys/kernel/config
DEV=$CONFIGFS/vkms/patina
EDIDS=/run/patina-fake-edid
HERE=$(cd "$(dirname "$0")" && pwd)

# The Nth connector's name, counting from 0: Virtual-1, Virtual-2, ...
connector() {
    ls /sys/class/drm | grep -E '^card[0-9]+-Virtual-[0-9]+$' | sed 's/^card[0-9]*-//' \
        | sort -t- -k2 -n | sed -n "$(($1 + 1))p"
}

sysfs() { ls -d /sys/class/drm/card*-"$1" | head -1; }

up() {
    n=${1:-3}
    modprobe vkms create_default_dev=0
    mkdir -p "$EDIDS"
    chmod 755 "$EDIDS"
    if [ -d "$DEV" ] && [ "$(cat "$DEV/enabled")" = 1 ]; then
        return 0
    fi
    mkdir -p "$DEV"
    i=0
    while [ "$i" -lt "$n" ]; do
        mkdir -p "$DEV/planes/p$i" "$DEV/crtcs/c$i" "$DEV/encoders/e$i" "$DEV/connectors/k$i"
        echo 1 > "$DEV/planes/p$i/type"  # primary
        ln -sf "$DEV/crtcs/c$i" "$DEV/planes/p$i/possible_crtcs/c$i"
        ln -sf "$DEV/crtcs/c$i" "$DEV/encoders/e$i/possible_crtcs/c$i"
        ln -sf "$DEV/encoders/e$i" "$DEV/connectors/k$i/possible_encoders/e$i"
        # Status 1 is connected, 2 disconnected: only the first starts connected.
        [ "$i" -eq 0 ] || echo 2 > "$DEV/connectors/k$i/status"
        i=$((i + 1))
    done
    echo 1 > "$DEV/enabled"
    udevadm settle
    first=$(connector 0)
    python3 "$HERE/edid.py" builtin > "$EDIDS/$first.bin"
    chmod 644 "$EDIDS/$first.bin"
}

plug() {
    n=$1
    name=$(connector "$n")
    [ -n "$name" ] || { echo "no connector $n" >&2; exit 1; }
    if [ $# -ge 2 ]; then
        python3 "$HERE/edid.py" "$2" > "$EDIDS/$name.bin"
        chmod 644 "$EDIDS/$name.bin"
    fi
    echo 1 > "$DEV/connectors/k$n/status"
}

unplug() {
    echo 2 > "$DEV/connectors/k$1/status"
}

status() {
    i=0
    while name=$(connector "$i"); [ -n "$name" ]; do
        state=$(cat "$(sysfs "$name")/status")
        edid=-
        [ -f "$EDIDS/$name.bin" ] && edid=$(python3 - "$EDIDS/$name.bin" <<'P'
import sys
d = open(sys.argv[1], "rb").read()
m = ((d[8] << 8) | d[9])
maker = "".join(chr(((m >> s) & 31) + 64) for s in (10, 5, 0))
print(f"{maker}-{d[11] << 8 | d[10]:04X}")
P
)
        echo "$i $name $state $edid"
        i=$((i + 1))
    done
}

install() {
    cat > /etc/systemd/system/patina-vkms.service <<'U'
[Unit]
Description=Virtual monitors for the Patina test VM (ADR-0125)
RequiresMountsFor=/patina
Before=getty@tty1.service

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=/bin/sh /patina/vm-linux/vkms.sh up

[Install]
WantedBy=multi-user.target
U
    systemctl daemon-reload
    systemctl enable patina-vkms.service >/dev/null 2>&1
    up "${1:-3}"
}

cmd=${1:-}
[ $# -eq 0 ] || shift
case "$cmd" in
    up) up "$@" ;;
    plug) plug "$@" ;;
    unplug) unplug "$@" ;;
    status) status ;;
    install) install "$@" ;;
    *) sed -n '2,16p' "$0" | sed 's/^# \{0,1\}//'; exit 1 ;;
esac
