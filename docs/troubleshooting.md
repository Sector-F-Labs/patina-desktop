# Troubleshooting

Things that went wrong on the Patina desktop, how to tell, and how to put them right. Each entry
says what you see, why, how to check, and the fix; add new ones at the end.

- [Save and Open dialogs never appear](#save-and-open-dialogs-never-appear)
- [A minimised app ignores its controls](#a-minimised-app-ignores-its-controls)

## Save and Open dialogs never appear

### What you see

In Draw, Programmer, Teleprinter or any other app: Save, Open or "open a folder" does nothing.
No dialog, no error. Files itself opens and works.

### Why

Apps ask the desktop portal for a file chooser, and in a Patina session the portal hands the
request to Files (`files --portal`, ADR-0004). D-Bus starts that backend on demand, from a *service
file* that says which program to run. There can be two:

| File | Runs | Written by |
|---|---|---|
| `/usr/local/share/dbus-1/services/org.freedesktop.impl.portal.desktop.patina.service` | `/usr/local/bin/files --portal` | the repository's `make install` and `make deploy` |
| `~/.local/share/dbus-1/services/org.freedesktop.impl.portal.desktop.patina.service` | `~/.local/bin/files --portal` | Files' own per-user `make install`, from when Files had a repository of its own |

D-Bus looks in your home directory first. If the second file is there and its program has gone,
every chooser request tries to run something that doesn't exist and fails, and the correct file in
`/usr/local` is never reached. `make install` can't fix it: it never writes to a home directory.

This happened on 7 November 2026, on two machines at once. Files' per-user install (2 and 4
November) had left the service file; the binary it named was later gone. The decision to stop it
happening again is ADR-0122.

### How to check

The session log has the failure, with D-Bus's own words:

```sh
journalctl --user -b | grep desktop.patina
# ... Activated service 'org.freedesktop.impl.portal.desktop.patina' failed:
#     Failed to execute program org.freedesktop.impl.portal.desktop.patina: No such file or directory
```

Then look for the service files, and whether each one's program exists:

```sh
find ~/.local/share/dbus-1 /usr/local/share/dbus-1 /usr/share/dbus-1 \
     -name '*portal.desktop.patina*' | while read f; do echo "== $f"; cat "$f"; done
ls -l ~/.local/bin/files /usr/local/bin/files
```

A file in `~/.local/share/dbus-1/services` whose `Exec=` program is missing is the cause.

### The fix

Move the stale file out of the way (renaming is enough, and can be undone):

```sh
cd ~/.local/share/dbus-1/services
mv org.freedesktop.impl.portal.desktop.patina.service{,.disabled}
```

Nothing needs restarting: D-Bus reads service files each time it starts a service, and the portal
doesn't cache the failure. Check that the backend now starts on its own:

```sh
busctl --user call org.freedesktop.impl.portal.desktop.patina \
    /org/freedesktop/portal/desktop org.freedesktop.DBus.Peer Ping
pgrep -ax files   # lists a `files --portal` among the Files processes
```

Then try Save in an app.

### Checking the chooser without an app

`busctl` and `gdbus call` can't show a dialog: they exit as soon as the call returns, and the portal
closes a request whose caller has gone. A client has to stay connected until the answer arrives.
This one asks for an Open dialog and waits (Python's Gio, as in `files/AGENTS.md`):

```python
import sys, gi
gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib

loop = GLib.MainLoop()
bus = Gio.bus_get_sync(Gio.BusType.SESSION)
token = "probe"
sender = bus.get_unique_name()[1:].replace(".", "_")
handle = f"/org/freedesktop/portal/desktop/request/{sender}/{token}"

def on_response(conn, name, path, iface, signal, params, data):
    print("response", params.unpack())
    loop.quit()

bus.signal_subscribe("org.freedesktop.portal.Desktop", "org.freedesktop.portal.Request",
                     "Response", handle, None, Gio.DBusSignalFlags.NONE, on_response, None)
reply = bus.call_sync("org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop",
                      "org.freedesktop.portal.FileChooser", "OpenFile",
                      GLib.Variant("(ssa{sv})", ("", "Portal probe",
                                                 {"handle_token": GLib.Variant("s", token)})),
                      None, Gio.DBusCallFlags.NONE, -1)
print("request", reply.unpack())
GLib.timeout_add_seconds(int(sys.argv[1]) if len(sys.argv) > 1 else 15,
                         lambda: (print("still open after the wait"), loop.quit()))
loop.run()
```

A Files window titled "Portal probe" should appear (the portal starts `files --choose` for it), and
`response 0 …` prints when you pick something; `response 1 …` if you cancel.

### Other copies that shadow the installed ones

The same thing happens, more quietly, to anything installed per user. The root `make install` puts
everything in `/usr/local`, and `~/.local/bin` is usually first on the PATH, so an older per-user
build runs instead of the installed one, with nothing to say why. If an app behaves like an old
version, compare:

```sh
type -a notebook          # every `notebook` on the PATH, the one that runs first
ls -l ~/.local/bin/notebook /usr/local/bin/notebook
```

The per-user leftovers from Files', Reader's and Notebook's old installs are
`~/.local/bin/{files,reader,notebook}`, their
`~/.local/share/applications/com.sectorflabs.{Files,Reader,Notebook}.desktop` and icons under
`~/.local/share/icons/hicolor`, and the service file above. Remove them by hand; no `make uninstall`
reaches a home directory. Launcher's own `make install` is the one per-user install left (ADR-0122).

### Two traps when looking into it over ssh

- `pkill -f` and `pgrep -f` match the shell running them, since its command line contains the
  pattern: `pkill -f "files --portal"` over ssh kills the ssh session. Find processes by name with
  `pgrep -x files`, or by pid.
- zsh stops a command line at a glob that matches nothing (`ls ~/.local/share/dbus-1/services/*patina*`
  with no such file) before running any of it. Use `find`.

## A minimised app ignores its controls

### What you see

An app is minimised and keeps doing what it was doing (UnAmp keeps playing), but the bar's media
buttons do nothing. Bring the window back and the button you pressed takes effect at once, as if it
had been waiting. The same goes for anything else the app does from its window's loop: a track
ending and the next one starting, a download finishing.

### Why

Wayland has no "minimised" for a client. A minimised window is only a window the compositor stopped
drawing, and a compositor that doesn't draw it doesn't send it frame callbacks. winit (under egui,
eframe and most Rust GUI apps) asks for a callback every time it presents a frame and holds back
every redraw until it arrives. With none coming, the app's UI loop never runs again, and a message
waiting for it (a media command from D-Bus, handed over on a channel and read in the loop) waits too.
The app's other threads carry on, which is why the audio doesn't stop.

This is not the media code and not the app: UnAmp's D-Bus thread wakes its loop with
`request_repaint()` on every command, and its loop would act on it. It is the compositor withholding
the callbacks, and was fixed there (ADR-0123): minimised windows get one every 500 ms.

### How to check

With a build from before the fix, the main thread is idle, not stuck:

```sh
doas gdb -p $(pgrep -x unamp) -batch -ex "thread 1" -ex "bt 8"
# #0 polling::Poller::wait_impl  ...  calloop ... winit::...::EventLoop::pump_events
```

and a command sent while minimised is ignored until the window returns:

```sh
wlrctl toplevel minimize app_id:unamp
busctl --user call org.mpris.MediaPlayer2.unamp /org/mpris/MediaPlayer2 \
    org.mpris.MediaPlayer2.Player Pause
busctl --user get-property org.mpris.MediaPlayer2.unamp /org/mpris/MediaPlayer2 \
    org.mpris.MediaPlayer2.Player PlaybackStatus     # still "Playing"
```

With the fix it says `Paused` within about half a second. A reply that always takes close to 0.5 s
for a minimised app, and 0.02 s for a visible one, is the callback's tick.

### The fix, and what it costs

`State::start_minimized_frames` (compositor `minimize.rs`) sends each minimised window a frame
callback every 500 ms. An app that is minimised now wakes twice a second and draws a frame nobody
sees. If an app should do nothing at all while hidden, that is a thing for the app to decide, since a
compositor can't tell a media player that wants to hear its buttons from a game that doesn't.
