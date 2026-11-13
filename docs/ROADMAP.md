# Roadmap

What the Patina desktop doesn't do yet, and what we've decided to leave alone. What it does today is in
[FEATURES.md](FEATURES.md).

This is a list of known gaps, each taken from a README, a "Limits" section or the code, and checked on
2026-11-09. It is not a schedule and promises nothing: an item moves when someone needs it. When one is
done, move it to FEATURES.md and delete it here. A decision worth a record gets an
[ADR](adr/README.md).

## Around the session

The things a desktop usually brings that the session still leaves to the system.

- **A notification daemon.** Nothing shows `org.freedesktop.Notifications`, so apps' notifications go
  nowhere. The tray (StatusNotifierItem) is there; notifications are not.
- **Autostart.** There is no list of apps to open at login.
- **A keyring manager.** gnome-keyring stores the passwords and logging in unlocks it (Ubuntu's PAM
  setup), and the password prompt is Patina's ([ADR-0139](adr/0139-a-native-keyring-prompter-in-front-of-gnome-keyring.md)),
  but nothing lists or deletes what's stored or changes the keyring's password: a Passwords page in
  Settings would.
- **Flatpak and portals.** Files is the file chooser and xdg-desktop-portal-wlr shares the screen
  ([ADR-0092](adr/0092-screen-capture-for-apps-and-recording.md)), and `xdg-desktop-portal-gtk` covers the
  rest. There is no portal backend of the desktop's own. We ruled that out for now, since it gives
  nothing the existing ones don't; revisit it if Flatpak apps need something they lack.
- **The lid.** The compositor has no handling of its own: logind suspends and the screen locks first
  ([ADR-0025](adr/0025-lock-screen.md)). Closing the lid with a monitor attached (a docked laptop) isn't
  handled.

## Displays

From [displays/HOW-IT-WORKS.md](../displays/HOW-IT-WORKS.md).

- A scale and a resolution **per monitor**, and a **primary you choose**.
- **Mirror and Primary for one monitor at a time**, not only all together.
- **Mirroring a monitor of another shape** letterboxes it instead of cropping.
- **The lock screen on every monitor** (the others stay black), and screenshots, capture and touch beyond
  the primary.
- **Monitors on a second GPU**, which aren't found.

## Windows

- **X11 windows in the tiler.** They aren't listed or placed; tiling goes through xdg-toplevel
  ([tiler/HOW-IT-WORKS.md](../tiler/HOW-IT-WORKS.md)).
- **More than halves and fill**, such as thirds or a grid. Another tiler can do it (ADR-0053); none exists yet.
- **The tiling preview drawn under the dragged window**, not over it.
- **Keeping windows across a desktop restart.** Today every window closes. It would take reconnecting
  clients, which only some toolkits can do ([session/HOW-IT-WORKS.md](../session/HOW-IT-WORKS.md)).

## Screenshots and recording

From [screenshot/HOW-IT-WORKS.md](../screenshot/HOW-IT-WORKS.md).

- A **selection across several outputs**, and one that's frozen while it's made.
- **Sharing through dmabufs**, not copies through memory.
- **Recording without `ffmpeg`**, which it needs on the path.

## Passwords and privileges

- **Other PAM prompts in the polkit dialog.** Every prompt is a password box, so there's no fingerprint
  reader or one-time code ([polkit/HOW-IT-WORKS.md](../polkit/HOW-IT-WORKS.md)).
- **The app's icon in that dialog**, which polkit sends.

## Dictation and typing

- **X11 windows.** Text can't be typed into one; it's dropped and a line is logged. X11 support is kept
  minimal on purpose.
- **Windows that are slow to hand over the clipboard** paste the old contents, and apps that bind Ctrl+V
  to something else do that instead ([dictate/HOW-IT-WORKS.md](../dictate/HOW-IT-WORKS.md)).
- **Loading the model when the first press arrives** is the default; `preload` removes the wait, and
  the model is put away after a game or video, and when idle
  ([ADR-0129](adr/0129-the-compositor-says-when-a-fullscreen-window-has-the-keyboard.md)). What stays
  is the GPU runtime, about 70 MB on nifl, until the daemon exits.
- **Dropped words on bigger models** were reported but not reproduced on recordings, in files or on
  nifl, so nothing is changed. If it comes back, add an option to keep the audio of each recording, so
  the exact input can be replayed.

## Platforms

- **Debian 13 as a session.** It builds and installs in a container; nobody has logged in to it.
- **Other Linux distributions**, which are untried (`make deps` names Ubuntu, Debian and FreeBSD packages only).
- **FreeBSD** runs in a VM on dwl. The battery and Wi-Fi work there; the rest hasn't been brought over.

## Housekeeping

- **ADR statuses.** About a dozen and a half records still say *Proposed* for things that shipped, such
  as the hardware backend (0016), nested rendering (0008, 0015), the process manager (0088–0090), tiling
  (0049–0052) and multi-monitor (0121). A status is allowed to change; the text isn't. Go through them and
  mark each Accepted, Superseded or Rejected.
- **Man pages and READMEs.** `make check` lints the man pages. Nothing checks that a README's "not yet"
  list is still true; this file is where such lists end up, so when you finish something, search the
  READMEs for the phrase before closing it.

## Decided against, for now

- **Another speech engine (Parakeet, Voxtral).** Evaluated on 2026-11-09 against Whisper on four of the
  user's recordings: Parakeet kept the stumbles Whisper cleans up, but made its own errors and dropped the
  last sentence of a 47-second take. Whisper's `small.en` was as good overall, so the work is in tuning
  Whisper. Adding an engine would be a trait around `transcribe.rs` and a different runtime (ONNX Runtime
  or sherpa-onnx), with no Vulkan.
- **An X11 clipboard bridge.** New features target Wayland only.
- **A portal backend of our own.** See above.
