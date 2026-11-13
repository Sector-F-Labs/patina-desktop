#!/usr/bin/env python3
"""patina-keyring against the real gnome-keyring-daemon, on a bus and in a
data directory of their own (your keyring isn't touched, and no screen is
needed: a stub answers where the dialog would).

The daemon asks the prompter for a password twice: to create a keyring (a
new password) and to unlock it again once locked. Run from the repo root,
after `cargo build -p patina-keyring`:

    python3 -I keyring/tests/daemon-e2e.py
"""
import os, subprocess, sys, tempfile, time

if "DBUS_SESSION_BUS_ADDRESS" not in os.environ or os.environ.get("E2E_INNER") != "1":
    # Re-run inside a private session bus.
    os.environ["E2E_INNER"] = "1"
    sys.exit(subprocess.run(["dbus-run-session", "--", sys.executable, "-I", __file__]).returncode)

import gi
gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib

root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
binary = os.path.join(root, "..", "target", "debug", "patina-keyring")
tmp = tempfile.mkdtemp(prefix="patina-keyring-e2e-")
asked = os.path.join(tmp, "asked")
stub = os.path.join(tmp, "dialog")
with open(stub, "w") as f:
    # Logs the options it was given and answers with a password: "pw", or
    # "bad" when asked by a file named wrong-password (a negative test).
    f.write(f'#!/bin/sh\necho "$@" >> {asked}\ncase "$*" in *"--kind confirm"*) printf "yes\\n";; *"Unlock Keyring"*) if [ -e {tmp}/wrong ]; then if [ -e {tmp}/retried ]; then printf "no\\n"; else touch {tmp}/retried; printf "yes\\nchoice 0\\npassword bad\\n"; fi; else printf "yes\\nchoice 0\\npassword pw\\n"; fi;; *) printf "yes\\nchoice 0\\nstrength 2\\npassword pw\\n";; esac\n')
os.chmod(stub, 0o755)

env = dict(os.environ, XDG_DATA_HOME=tmp + "/data", PATINA_KEYRING_DIALOG=stub)
os.makedirs(env["XDG_DATA_HOME"])
procs = []
try:
    procs.append(subprocess.Popen([binary], env=env))
    time.sleep(1)
    procs.append(subprocess.Popen(
        ["gnome-keyring-daemon", "--foreground", "--components=secrets"], env=env,
        stdout=subprocess.DEVNULL))
    time.sleep(1)

    bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    SECRETS = "org.freedesktop.secrets"

    def call(path, iface, method, args, sig):
        return bus.call_sync(SECRETS, path, iface, method, GLib.Variant(sig, args),
                             None, Gio.DBusCallFlags.NONE, 30000, None)

    def prompt(path):
        """Runs a Secret Service prompt to its end; whether it was dismissed."""
        if path == "/":
            return None
        loop = GLib.MainLoop()
        out = {}
        def completed(conn, sender, obj, iface, signal, params):
            out["dismissed"], out["result"] = params.unpack()
            loop.quit()
        sub = bus.signal_subscribe(SECRETS, "org.freedesktop.Secret.Prompt", "Completed",
                                   path, None, Gio.DBusSignalFlags.NONE, completed)
        call(path, "org.freedesktop.Secret.Prompt", "Prompt", ("",), "(s)")
        GLib.timeout_add_seconds(30, loop.quit)
        loop.run()
        bus.signal_unsubscribe(sub)
        return out

    # 1. Create a keyring: the daemon asks for a new password.
    props = {"org.freedesktop.Secret.Collection.Label": GLib.Variant("s", "e2e")}
    r = call("/org/freedesktop/secrets", "org.freedesktop.Secret.Service", "CreateCollection",
             (props, ""), "(a{sv}s)").unpack()
    collection, prompt_path = r
    if collection == "/":
        done = prompt(prompt_path)
        assert done and not done["dismissed"], f"create was dismissed: {done}"
        collection = done["result"]
    print("created", collection)
    with open(tmp + "/data/keyrings/e2e.keyring", "rb") as f:
        assert f.read(12) == b"GnomeKeyring", "the keyring should be encrypted"

    # 2. Lock it, then unlock it: the daemon asks for the password.
    call("/org/freedesktop/secrets", "org.freedesktop.Secret.Service", "Lock",
         ([collection],), "(ao)")
    locked = bus.call_sync(SECRETS, collection, "org.freedesktop.DBus.Properties", "Get",
                           GLib.Variant("(ss)", ("org.freedesktop.Secret.Collection", "Locked")),
                           None, Gio.DBusCallFlags.NONE, 5000, None).unpack()[0]
    assert locked, "it should be locked"
    # A wrong password doesn't unlock it: the daemon asks again (we cancel).
    open(tmp + "/wrong", "w").close()
    _, prompt_path = call("/org/freedesktop/secrets", "org.freedesktop.Secret.Service",
                          "Unlock", ([collection],), "(ao)").unpack()
    done = prompt(prompt_path)
    assert done and done["dismissed"], f"a wrong password unlocked it: {done}"
    os.remove(tmp + "/wrong")
    # ...the right one does.
    unlocked, prompt_path = call("/org/freedesktop/secrets", "org.freedesktop.Secret.Service",
                                 "Unlock", ([collection],), "(ao)").unpack()
    done = prompt(prompt_path)
    assert done and not done["dismissed"], f"unlock was dismissed: {done}"
    locked = bus.call_sync(SECRETS, collection, "org.freedesktop.DBus.Properties", "Get",
                           GLib.Variant("(ss)", ("org.freedesktop.Secret.Collection", "Locked")),
                           None, Gio.DBusCallFlags.NONE, 5000, None).unpack()[0]
    assert not locked, "it should be unlocked now"
    print("unlocked again")
    shown = open(asked).read()
    print("the dialog was asked:")
    print(shown)
    # With a password strength reported, the daemon doesn't ask whether to
    # go on without a password.
    assert "--kind confirm" not in shown, "it asked about unencrypted storage"
    print("ok")
finally:
    for p in procs:
        p.terminate()
    import shutil
    shutil.rmtree(tmp, ignore_errors=True)
