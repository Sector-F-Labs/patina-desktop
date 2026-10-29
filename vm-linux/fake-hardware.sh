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
#   fake-hardware.sh                       set it up (idempotent)
#   fake-hardware.sh battery PERCENT [discharging|charging|full|not-charging]
#   fake-hardware.sh ac on|off
set -eu

PARAMS=/sys/module/test_power/parameters
AP_DIR=/etc/patina-vm

install_all() {
    # NetworkManager isn't in the server image, and hostapd is the access point.
    missing=
    for p in network-manager wpasupplicant hostapd dnsmasq-base; do
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
    cat > /etc/systemd/system/patina-fake-ap.service <<EOF
[Unit]
Description=A fake Wi-Fi access point on wlan1 (mac80211_hwsim), for the test VM
After=sys-subsystem-net-devices-wlan1.device
BindsTo=sys-subsystem-net-devices-wlan1.device

[Service]
ExecStart=/usr/sbin/hostapd $AP_DIR/hostapd.conf
ExecStartPost=/usr/sbin/ip addr replace 10.42.0.1/24 dev wlan1
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
ExecStart=/usr/sbin/dnsmasq --keep-in-foreground --conf-file=/dev/null --interface=wlan1 --bind-interfaces --port=0 --dhcp-range=10.42.0.10,10.42.0.50,12h --dhcp-leasefile=/run/patina-fake-dhcp.leases
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
    systemctl enable --now patina-fake-ap.service patina-fake-dhcp.service >/dev/null 2>&1
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
    *)
        echo "usage: fake-hardware.sh [battery PERCENT [status] | ac on|off]" >&2
        exit 2
        ;;
esac
