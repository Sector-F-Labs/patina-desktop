"""The Linux test VM as a fixture: boots it if it isn't up, makes sure the Patina
session is running, and gives tests its screen, its shell and its fake hardware.

Run with `make test-vm` (from patina-desktop), or `uv run pytest` in this folder.
"""

import os
import subprocess
import time
from pathlib import Path

import pytest
from PIL import Image, ImageChops

REPO = Path(__file__).resolve().parents[2]
VM_SH = REPO / "vm-linux" / "vm.sh"
INSTANCE = os.environ.get("PATINA_VM_INSTANCE", "dev")
SCREEN = (1280, 800)
# The bar along the bottom; windows are above it.
BAR_TOP = 760


def pattern(path: str) -> str:
    """A pgrep/pkill -f pattern for `path` that doesn't match the shell running it:
    the first letter of the last name in brackets."""
    name = path.rsplit("/", 1)[-1]
    return f"[{name[0]}]{name[1:]}"


def wait_for(predicate, timeout=30.0, every=1.0, what="the condition"):
    """The predicate's first truthy result, polled until `timeout` seconds."""
    deadline = time.monotonic() + timeout
    while True:
        result = predicate()
        if result:
            return result
        if time.monotonic() > deadline:
            raise AssertionError(f"timed out after {timeout:.0f}s waiting for {what}")
        time.sleep(every)


def changed(a: Image.Image, b: Image.Image, box=None, threshold=24) -> float:
    """The fraction of pixels in `box` (all, if None) that differ between two
    screenshots by more than `threshold` in any channel."""
    a, b = a.convert("RGB"), b.convert("RGB")
    if box:
        a, b = a.crop(box), b.crop(box)
    diff = ImageChops.difference(a, b)
    # A pixel counts if its largest channel difference passes the threshold.
    peak = ImageChops.lighter(ImageChops.lighter(*diff.split()[:2]), diff.split()[2])
    hist = peak.histogram()
    return sum(hist[threshold + 1 :]) / (a.width * a.height)


class Vm:
    def run(self, *args, check=True, timeout=300) -> str:
        done = subprocess.run(
            [str(VM_SH), *args],
            capture_output=True,
            text=True,
            timeout=timeout,
            env={**os.environ, "PATINA_VM_INSTANCE": INSTANCE},
        )
        if check and done.returncode:
            raise RuntimeError(f"vm.sh {' '.join(args)}: {done.stderr or done.stdout}")
        return done.stdout.strip()

    def ssh(self, command: str, check=True, timeout=120) -> str:
        return self.run("ssh", command, check=check, timeout=timeout)

    def running(self) -> bool:
        pid = REPO / "target" / f"vm-{INSTANCE}" / "qemu.pid"
        try:
            return pid.exists() and Path(f"/proc/{pid.read_text().strip()}").exists()
        except OSError:
            return False

    def shot(self, name="screen") -> Image.Image:
        path = REPO / "target" / f"vm-{INSTANCE}" / f"test-{name}.png"
        self.run("shot", str(path))
        with Image.open(path) as image:
            return image.convert("RGB")

    def fake(self, *args: str) -> None:
        self.run("fake", *args)

    def display(self, *args: str) -> str:
        """Plugs or unplugs a virtual monitor (`display plug 1 lg-ultrawide`); the
        multi-monitor guest only (PATINA_VM_INSTANCE=displays)."""
        return self.run("display", *args)

    def session(self, command: str, check=True, timeout=60) -> str:
        """`command` run in the Patina session's environment: its session bus,
        runtime folder and Wayland display."""
        return self.run("session", command, check=check, timeout=timeout)

    def session_up(self) -> bool:
        return self.alive("release/patina-compositor")

    def alive(self, path: str) -> bool:
        return self.ssh(f"pgrep -f '{pattern(path)}'", check=False) != ""

    def launch(self, command: str) -> None:
        """Starts `command` in the Patina session, as the desktop would."""
        self.ssh(
            f"pid=$(pgrep -f '{pattern('release/patina-compositor')}' | head -1); "
            "export $(tr '\\0' '\\n' < /proc/$pid/environ "
            "| grep -E '^(DBUS_SESSION_BUS_ADDRESS|XDG_RUNTIME_DIR)='); "
            "export WAYLAND_DISPLAY=$(ls $XDG_RUNTIME_DIR | grep -E '^wayland-[0-9]+$' | head -1); "
            f"nohup {command} >/tmp/test-app.log 2>&1 </dev/null &"
        )

    def kill(self, path: str) -> None:
        self.ssh(f"pkill -f '{pattern(path)}'", check=False)


@pytest.fixture(scope="session")
def vm() -> Vm:
    vm = Vm()
    if not vm.running():
        vm.run("start", timeout=900)
    wait_for(vm.session_up, timeout=90, what="the Patina session")
    # Let the bar draw once.
    time.sleep(5)
    return vm
