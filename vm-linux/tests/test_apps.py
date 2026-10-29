"""Each app the session starts opens a window that draws something. A window that
maps but never draws (Settings on FreeBSD, before WGPU_BACKEND=gl) leaves the screen
as it was apart from a tab in the bar, so the test is that the area above the bar
changes, by even a little: a terminal's dark background on the dark wallpaper is a few
levels apart, and a blank window is none."""

import time

import pytest

from conftest import BAR_TOP, SCREEN, changed, wait_for

ABOVE_BAR = (0, 0, SCREEN[0], BAR_TOP)

APPS = {
    "settings": "/patina/target/release/patina-settings",
    "processes": "/patina/target/release/patina-processes",
    "teleprinter": "/teleprinter/target/release/teleprinter",
    "foot": "/usr/bin/foot",
}


@pytest.mark.parametrize("name", APPS)
def test_the_app_opens_and_draws(vm, name):
    command = APPS[name]
    vm.kill(command)
    time.sleep(2)
    before = vm.shot(f"{name}-before")
    vm.launch(command)
    try:
        wait_for(
            lambda: changed(before, vm.shot(f"{name}-after"), ABOVE_BAR, threshold=4) > 0.02,
            timeout=40,
            every=2,
            what=f"{name} to draw a window",
        )
    finally:
        vm.kill(command)
