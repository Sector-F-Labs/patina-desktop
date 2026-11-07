"""Several monitors (ADR-0121, ADR-0124), against the multi-monitor guest: vkms virtual
monitors that are plugged and unplugged, with fake EDIDs so each is a particular model.

Run with `make test-vm-displays` (PATINA_VM_INSTANCE=displays). The compositor's
`com.sectorflabs.Patina.Displays` is what these ask and what they check; the popup
(`patina-displays`) opens by itself when a monitor is plugged in, and is closed here so
it never waits for a choice."""

import json

import pytest

from conftest import INSTANCE, pattern, wait_for

pytestmark = pytest.mark.skipif(
    INSTANCE != "displays", reason="needs the multi-monitor guest (PATINA_VM_INSTANCE=displays)"
)

BUS = (
    "busctl --user call com.sectorflabs.Patina.Displays "
    "/com/sectorflabs/Patina/Displays com.sectorflabs.Patina.Displays"
)


def call(vm, member, *args, json_out=False):
    flags = "--json=short " if json_out else ""
    cmd = BUS.replace("busctl --user call", f"busctl --user {flags}call") + f" {member} {' '.join(args)}"
    out = vm.session(cmd)
    return json.loads(out) if json_out else out


def dictionaries(vm, member):
    """The dictionaries `member` returns (List or Saved), each as plain values."""
    reply = call(vm, member, json_out=True)
    return [{k: v["data"] for k, v in row.items()} for row in reply["data"][0]]


def monitors(vm):
    return dictionaries(vm, "List")


def saved(vm):
    return dictionaries(vm, "Saved")


def the_popup_closed(vm):
    vm.ssh(f"pkill -f '{pattern('patina-displays')}'", check=False)


def settle(vm, count, what):
    """Waits until `count` monitors are listed; returns them."""
    return wait_for(
        lambda: (m := monitors(vm)) if len(monitors(vm)) == count else None,
        timeout=30,
        every=1,
        what=what,
    )


@pytest.fixture(autouse=True)
def alone(vm):
    """Each test starts with the first monitor only, extending, and nothing remembered."""
    for n in (1, 2):
        vm.display("unplug", str(n))
    the_popup_closed(vm)
    settle(vm, 1, "the first monitor alone")
    call(vm, "SetMode", "s", "extend")
    for entry in saved(vm):
        call(vm, "Forget", "s", entry["id"])
    yield
    the_popup_closed(vm)


def primary(vm):
    return next(m for m in monitors(vm) if m["primary"])


def test_the_desktop_starts_on_one_monitor(vm):
    [only] = monitors(vm)
    assert only["primary"] and only["active"]
    # Its EDID says it is a panel with a product code and no serial, so its port is part of its id.
    assert only["id"].startswith("IVO-8C44-00000000-")


def test_a_monitor_plugged_in_stays_off_until_chosen(vm):
    vm.display("plug", "1", "lg-ultrawide")
    first, second = settle(vm, 2, "the plugged monitor to be listed")
    assert first["active"] and not second["active"]
    assert second["id"] == "GSM-772E-812NTAB"
    # The popup comes by itself, a moment after: it's what asks.
    wait_for(
        lambda: vm.ssh(f"pgrep -f '{pattern('patina-displays')}'", check=False),
        timeout=20,
        what="the popup to open",
    )


def test_extending_drives_it_to_the_right_of_the_first(vm):
    vm.display("plug", "1", "lg-ultrawide")
    settle(vm, 2, "the monitor to be listed")
    the_popup_closed(vm)
    call(vm, "SetMode", "s", "extend")
    first = primary(vm)
    [other] = [m for m in monitors(vm) if not m["primary"]]
    assert other["active"]
    # Not placed yet: after the one before, side by side.
    assert (other["x"], other["y"]) == (first["x"] + first["logical-width"], first["y"])


def test_unplugging_drops_it_and_the_first_stays(vm):
    vm.display("plug", "1", "lg-ultrawide")
    settle(vm, 2, "the monitor to be listed")
    call(vm, "SetMode", "s", "extend")
    vm.display("unplug", "1")
    [only] = settle(vm, 1, "the unplugged monitor to go")
    assert only["primary"] and (only["x"], only["y"]) == (0, 0)


def test_a_placement_is_remembered_and_listed(vm):
    vm.display("plug", "1", "lg-ultrawide")
    settle(vm, 2, "the monitor to be listed")
    call(vm, "SetMode", "s", "extend")
    first = primary(vm)
    call(vm, "SetPlace", "sssi", "GSM-772E-812NTAB", first["id"], "above", "0")
    [entry] = saved(vm)
    assert entry["id"] == "GSM-772E-812NTAB" and entry["in-use"]
    assert entry["side"] == "above" and entry["name"] == "LG HDR WQHD"


def test_a_hot_desks_monitor_goes_where_one_of_its_model_was(vm):
    # At home: a Dell, placed above the first monitor.
    vm.display("plug", "1", "dell-home")
    settle(vm, 2, "the Dell to be listed")
    call(vm, "SetMode", "s", "extend")
    first = primary(vm)
    dell = next(m for m in monitors(vm) if not m["primary"])
    call(vm, "SetPlace", "sssi", dell["id"], first["id"], "above", "0")
    vm.display("unplug", "1")
    settle(vm, 1, "the Dell to go")

    # At the hot desk: another Dell, a serial never seen.
    vm.display("plug", "1", "dell-desk")
    _, desk = settle(vm, 2, "the desk's Dell to be listed")
    assert desk["id"] != dell["id"]
    assert desk["known"], "a monitor of a model that was placed is known"
    the_popup_closed(vm)
    call(vm, "SetMode", "s", "extend")
    first = primary(vm)
    [desk] = [m for m in monitors(vm) if not m["primary"]]
    assert desk["active"]
    assert desk["y"] < first["y"], "the desk's Dell is above, as the home one was"
    # Nothing was saved for it: only placing a monitor saves an entry.
    assert [e["id"] for e in saved(vm)] == [dell["id"]]


def test_forgetting_makes_the_model_new_again(vm):
    vm.display("plug", "1", "dell-home")
    settle(vm, 2, "the Dell to be listed")
    call(vm, "SetMode", "s", "extend")
    first = primary(vm)
    dell = next(m for m in monitors(vm) if not m["primary"])
    call(vm, "SetPlace", "sssi", dell["id"], first["id"], "left", "0")
    vm.display("unplug", "1")
    settle(vm, 1, "the Dell to go")
    [entry] = saved(vm)
    assert not entry["in-use"], "nothing connected goes by it"
    call(vm, "Forget", "s", entry["id"])
    assert saved(vm) == []
    vm.display("plug", "1", "dell-desk")
    _, desk = settle(vm, 2, "the desk's Dell to be listed")
    assert not desk["known"]


def test_the_primary_alone_turns_the_others_off(vm):
    vm.display("plug", "1", "full-hd")
    settle(vm, 2, "the monitor to be listed")
    call(vm, "SetMode", "s", "extend")
    assert sum(m["active"] for m in monitors(vm)) == 2
    call(vm, "SetMode", "s", "off")
    wait_for(
        lambda: sum(m["active"] for m in monitors(vm)) == 1,
        timeout=20,
        what="the second monitor to go off",
    )
    # Still connected, so still listed, and extending lights it again.
    assert len(monitors(vm)) == 2
    call(vm, "SetMode", "s", "extend")
    assert sum(m["active"] for m in monitors(vm)) == 2


def test_the_desktop_can_be_seen(vm):
    # The capture of the primary: not blank, and the size of its mode.
    vm.display("plug", "1", "lg-ultrawide")
    settle(vm, 2, "the monitor to be listed")
    the_popup_closed(vm)
    call(vm, "SetMode", "s", "extend")
    image = vm.shot("displays")
    assert image.width > 0 and image.height > 0
    assert len(image.resize((16, 16)).getcolors(256) or []) > 1, "the screen is one flat colour"
