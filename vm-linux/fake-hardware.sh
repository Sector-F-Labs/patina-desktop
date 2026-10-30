#!/bin/sh
# Run in the Linux test VM by vm.sh, as root, on every start and restart: the
# hardware a VM lacks and the desktop's bar and menus read, made by the kernel
# itself, so the code that reads it runs for real (ADR-0017).
#
# - A battery and a charger, from test_power: /sys/class/power_supply/test_battery,
#   which the bar's battery and Settings' Power page read. Changed with
#   `fake-hardware.sh battery` and `ac` below.
# - Wi-Fi, from mac80211_hwsim: wlan0, the one NetworkManager manages and the
#   Wi-Fi menu uses, and wlan1, an access point (hostapd) called patina-test with
#   the password patina-test-pass, and a DHCP server (dnsmasq), so the menu has a
#   network to list and join.
#
# - A captive portal on that network, off until asked for (fake-portal.py): NetworkManager's
#   connectivity check goes to it, so with `portal on` joining patina-test leaves you at a
#   sign-in page, as at a café; its Accept button, or `portal off`, lets you through.
#
#   fake-hardware.sh                       set it up (idempotent)
#   fake-hardware.sh battery PERCENT [discharging|charging|full|not-charging]
#   fake-hardware.sh ac on|off
#   fake-hardware.sh portal on|off
set -eu

PORTAL=${PORTAL:-/patina/vm-linux/fake-portal.py}

PARAMS=/sys/module/test_power/parameters
AP_DIR=/etc/patina-vm

install_all() {
    # NetworkManager isn't in the server image, and hostapd is the access point.
    missing=
    for p in network-manager wpasupplicant hostapd dnsmasq-base iw; do
        dpkg -s "$p" >/dev/null 2>&1 || missing="$missing $p"
    done
    if [ -n "$missing" ]; then
        apt-get update -qq
        DEBIAN_FRONTEND=noninteractive apt-get install -y -qq $missing
    fi

    # Loaded at every boot, wlan0 and wlan1 being the two radios.
    printf 'test_power\nmac80211_hwsim\n' > /etc/modules-load.d/patina-fake-hardware.conf
    echo 'options mac80211_hwsim radios=2' > /etc/modprobe.d/patina-fake-hardware.conf

    # wlan1 is the access point's, not NetworkManager's.
    mkdir -p /etc/NetworkManager/conf.d
    cat > /etc/NetworkManager/conf.d/patina-vm.conf <<EOF
[keyfile]
unmanaged-devices=interface-name:wlan1

[connectivity]
enabled=true
uri=http://10.42.0.1/
response=NetworkManager is online
interval=10
EOF

    mkdir -p "$AP_DIR"
    cat > "$AP_DIR/hostapd.conf" <<EOF
interface=wlan1
driver=nl80211
ssid=patina-test
hw_mode=g
channel=6
wpa=2
wpa_key_mgmt=WPA-PSK
rsn_pairwise=CCMP
wpa_passphrase=patina-test-pass
EOF
    # The access point lives in a network namespace of its own: in the same one as
    # wlan0, its address 10.42.0.1 would be the host's own, and NetworkManager's
    # traffic to it (bound to wlan0) would never reach it, as it can't to a real
    # router. `patina-ap` is the router.
    MOUNT_REQ=
    if [ "${PORTAL#/patina}" != "$PORTAL" ]; then
        # The portal's script is the checkout's, on the /patina share: wait for it.
        MOUNT_REQ="RequiresMountsFor=/patina"
    fi
    cat > /etc/systemd/system/patina-fake-ap.service <<EOF
[Unit]
Description=A fake Wi-Fi access point on wlan1 (mac80211_hwsim), in its own namespace, for the test VM
After=systemd-modules-load.service

[Service]
ExecStartPre=/bin/sh -c 'ip netns add patina-ap 2>/dev/null; for i in 1 2 3 4 5 6 7 8 9 10; do [ -e /sys/class/net/wlan1 ] || ip netns exec patina-ap ip link show wlan1 >/dev/null 2>&1 && break; sleep 1; done; [ ! -e /sys/class/net/wlan1 ] || iw phy "\$\$(cat /sys/class/net/wlan1/phy80211/name)" set netns name patina-ap'
ExecStartPre=/usr/sbin/ip netns exec patina-ap ip link set lo up
ExecStart=/usr/sbin/ip netns exec patina-ap /usr/sbin/hostapd $AP_DIR/hostapd.conf
ExecStartPost=/usr/sbin/ip netns exec patina-ap /usr/sbin/ip addr replace 10.42.0.1/24 dev wlan1
Restart=on-failure

[Install]
WantedBy=multi-user.target
EOF
    # Addresses for whoever joins it, or NetworkManager never finishes joining.
    cat > /etc/systemd/system/patina-fake-dhcp.service <<EOF
[Unit]
Description=DHCP for the fake Wi-Fi access point, for the test VM
After=patina-fake-ap.service
Requires=patina-fake-ap.service

[Service]
ExecStart=/usr/sbin/ip netns exec patina-ap /usr/sbin/dnsmasq --keep-in-foreground --conf-file=/dev/null --interface=wlan1 --bind-interfaces --port=0 --dhcp-range=10.42.0.10,10.42.0.50,12h --dhcp-leasefile=/run/patina-fake-dhcp.leases
Restart=on-failure

[Install]
WantedBy=multi-user.target
EOF
    cat > /etc/systemd/system/patina-fake-portal.service <<EOF
[Unit]
Description=A fake captive portal on the fake access point, for the test VM
After=patina-fake-ap.service
Requires=patina-fake-ap.service
${MOUNT_REQ}

[Service]
ExecStart=/usr/sbin/ip netns exec patina-ap /usr/bin/python3 $PORTAL
Restart=on-failure

[Install]
WantedBy=multi-user.target
EOF
    systemctl daemon-reload

    # Now, as well as at the next boot.
    modprobe test_power
    modprobe mac80211_hwsim radios=2 2>/dev/null || true
    systemctl enable --now NetworkManager >/dev/null 2>&1
    systemctl restart NetworkManager
    systemctl enable --now patina-fake-ap.service patina-fake-dhcp.service patina-fake-portal.service >/dev/null 2>&1
}

case "${1:-}" in
    "")
        install_all
        ;;
    battery)
        echo "${2:?usage: fake-hardware.sh battery PERCENT [status]}" > "$PARAMS/battery_capacity"
        [ -z "${3:-}" ] || echo "$3" > "$PARAMS/battery_status"
        ;;
    ac)
        case "${2:-}" in
            on) echo 1 > "$PARAMS/ac_online" ;;
            off) echo 0 > "$PARAMS/ac_online" ;;
            *) echo "usage: fake-hardware.sh ac on|off" >&2; exit 2 ;;
        esac
        ;;
    portal)
        case "${2:-}" in
            on) touch /run/patina-fake-portal ;;
            off) rm -f /run/patina-fake-portal ;;
            *) echo "usage: fake-hardware.sh portal on|off" >&2; exit 2 ;;
        esac
        ;;
    *)
        echo "usage: fake-hardware.sh [battery PERCENT [status] | ac on|off | portal on|off]" >&2
        exit 2
        ;;
esac
