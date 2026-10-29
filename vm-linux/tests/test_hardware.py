"""What the bar reads from the machine: the battery (ADR-0024) and Wi-Fi (ADR-0022),
from the VM's fake hardware (ADR-0096). Both are polled every few seconds (the
battery's every 5, Wi-Fi's every 3), so the screen changes soon after the hardware.

The bar's layout moves (flat, or floating in parts), so these don't look for an icon at
a place: they look at the strip where the tray's icons are, left of the clock, which
changes every minute and so is left out, and ask whether it changed."""

import pytest

from conftest import changed, wait_for

# The tray's icons at 1280x800: Wi-Fi, volume and battery, in either layout, ending
# before the clock.
ICONS = (900, 745, 1085, 800)
PARAMS = "/sys/module/test_power/parameters"


def icons_change_from(vm, before, timeout=30, what="the bar's icons to change"):
    wait_for(
        lambda: changed(before, vm.shot("now"), ICONS) > 0.01,
        timeout=timeout,
        every=2,
        what=what,
    )


def test_the_fake_battery_is_what_the_kernel_reports(vm):
    vm.fake("battery", "55", "discharging")
    assert vm.ssh("cat /sys/class/power_supply/test_battery/capacity") == "55"
    assert vm.ssh("cat /sys/class/power_supply/test_battery/status") == "Discharging"


def test_the_bar_follows_the_battery(vm):
    vm.fake("ac", "on")
    vm.fake("battery", "90", "charging")
    wait_for(lambda: True, timeout=1)
    settled = vm.shot("battery-90")
    vm.fake("ac", "off")
    vm.fake("battery", "9", "discharging")
    # 9% and discharging is the low-battery look: a different icon and number.
    icons_change_from(vm, settled, what="the bar's battery to change")


def test_the_bar_shows_a_battery_only_where_the_kernel_has_one(vm):
    vm.fake("battery", "60", "discharging")
    try:
        with_battery = vm.shot("with-battery")
        vm.ssh(f"echo 0 | sudo tee {PARAMS}/battery_present")
        icons_change_from(vm, with_battery, what="the battery to leave the bar")
    finally:
        vm.ssh(f"echo 1 | sudo tee {PARAMS}/battery_present", check=False)


@pytest.fixture
def forgotten(vm):
    """Starts and ends not joined, and forgets the network, or it would rejoin."""
    forget = "sudo nmcli connection delete patina-test; sudo nmcli device disconnect wlan0"
    vm.ssh(forget, check=False)
    yield
    vm.ssh(forget, check=False)


def test_the_fake_access_point_is_in_range(vm):
    # As root: polkit lets the desktop's session scan, not an ssh shell.
    vm.ssh("sudo nmcli device wifi rescan", check=False)
    wait_for(
        lambda: "patina-test" in vm.ssh("sudo nmcli -t -f SSID device wifi list", check=False),
        timeout=30,
        what="patina-test in range",
    )


def test_the_bar_shows_joining_a_network(vm, forgotten):
    not_joined = vm.shot("wifi-off")
    vm.ssh(
        "sudo nmcli device wifi rescan; sleep 3; "
        "sudo nmcli device wifi connect patina-test password patina-test-pass",
        timeout=90,
    )
    icons_change_from(vm, not_joined, what="the bar's Wi-Fi icon to change once joined")
    assert "connected" in vm.ssh("nmcli -t -f DEVICE,STATE device status | grep wlan0")
