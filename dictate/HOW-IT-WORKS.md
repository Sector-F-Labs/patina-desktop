# How dictation fits into the desktop

Hold a key, speak, let go, and the words are typed into whatever window has focus. This page
explains how that works on the Patina desktop, and why it turned out to be small. It is written
for this desktop's particular architecture, but the implementation is simple and kept apart
from the rest of the desktop, so it may be easy to bring to another one. The README next to it
covers running and building; the decisions are in ADR-0070 to ADR-0074.

## The idea in one paragraph

Typing into another program's window is the hard part of dictation on Wayland. A normal client
can't see global keys and can't press keys in someone else's window. On Patina the compositor is
the one process that owns the keyboard, the clipboard and the focus, so it does those two
jobs itself. It tells the world when the key goes down and up, and it types text when asked. The
rest (microphone, Whisper, the overlay) is an ordinary program that knows nothing about
windows or keyboards. The two sides talk over the session D-Bus with 3 message types.

## The picture

<img src="images/architecture.svg" alt="The compositor sees the dictate key and signals Started and Stopped over D-Bus. The dictate daemon records, runs Whisper and calls Commit back; the compositor types the text into the focused window through the clipboard. A separate overlay process shows the loudness.">

The compositor and the daemon only meet on the session D-Bus: two signals one way, and one
`Commit` call back.

## The sequence, in time

<img src="images/sequence.svg" alt="One dictation in time: hold the chord, the compositor signals the daemon, which records and transcribes and calls Commit; the compositor saves the clipboard, serves the text, presses Ctrl+V and gives the clipboard back.">

### Why it stops early and then waits

The two waits in the sequence look like they repeat each other, but they happen at different
moments, for different reasons:

- **Recording stops on the first key up.** The chord is three keys (Ctrl, Super, Alt), and
  letting go of any one of them ends it, whatever else is still down. A hand never lets go of
  all three at the same instant, so "all keys up" would make the recording run on.
- **Typing waits for the rest to come up.** By then the other keys may still be held. If the
  compositor pressed Ctrl+V straight away, the window would see Ctrl+Alt+Super+V, not a paste.
  So `Commit` waits, for up to two seconds, until no chord key is down, and only then presses
  Ctrl+V. In practice Whisper takes longer than your fingers do, so the wait rarely shows.

The modifiers still reach the window while they're held, so letting go of Super isn't taken as
a tap that opens the launcher.

## What the compositor does that a client can't

This is the "interesting architecture decision" part. Everything marked here is one process
acting on its own seat, so none of it needs a protocol between programs.

| Job | How it's done here | Why a normal client can't |
|-----|--------------------|---------------------------|
| See a held chord of only modifiers | `input.rs`: it is in the key path, before any window | clients only get keys when focused |
| Know the key came up, whatever else is down | same place | same |
| Press keys in another window | `KeyboardHandle::input` on the seat, with the key looked up in the layout in use, so V is V on Dvorak | needs a virtual-keyboard protocol, which not every compositor gives a client, and a keymap of its own |
| Know the window is a terminal | the compositor already tracks it (`cmd.rs`), so it presses Ctrl+Shift+V | a client doesn't know what the focused window is |
| Paste without losing your clipboard | it reads the clipboard out of the app that owns it, serves the text as its own selection, then serves the old contents back (`Served::Text`, `Served::Saved`) | a client would overwrite the clipboard and could not tell when the window had finished pasting |
| Know the paste has happened | it sees the window's request for the clipboard | invisible to a client |

The clipboard path was chosen over text-input-v3 and per-character key presses (ADR-0071). It
works in every Wayland app that pastes, terminals included. The cost is that clipboard managers
see the dictated text go past.

## What the daemon doesn't know

`patina-dictate --daemon` has no idea what a window is. Its whole contract is:

1. Listen for `DictationStarted` and `DictationStopped` on `com.sectorflabs.Patina.Input`.
2. Record between them, transcribe, and call `Commit(text)` on that same name.
3. Keep the model only as long as it's wanted. `FullscreenChanged` says a game or a video has the
   keyboard; ten seconds into one, and after ten idle minutes, the model is unloaded and loads
   again at the next press (ADR-0129). `preload` in the config loads it at start instead of at
   the first press. All three are on Settings' Dictation page, under Memory.
4. Exit when that name leaves the bus. It has no Wayland connection, so nothing else would end
   it, and a restarted desktop would have two daemons dictating everything twice.

That is why it could be written, tested and run on its own (`patina-dictate | wl-copy`) before
the compositor side existed, and why the overlay could be a third separate program fed over a
pipe (ADR-0073).

## The files

```
 patina-desktop/
 ├── compositor/src/
 │   ├── input.rs          key events; sees the chord and its release
 │   ├── keys.rs           [keys] dictate = "Ctrl+Super+Alt"; modifiers-only allowed for dictate only
 │   ├── typing.rs         announces started/stopped, runs Commit: clipboard in, Ctrl+V, clipboard back
 │   ├── typing/dbus.rs    the com.sectorflabs.Patina.Input interface
 │   └── main.rs           keep_client("patina-dictate --daemon")
 └── dictate/src/
     ├── daemon.rs         the D-Bus listener, the recording, the transcribe thread
     ├── capture.rs        microphone: cpal on Linux, /dev/dsp on FreeBSD
     ├── audio.rs          mixdown and resampling to Whisper's 16 kHz
     ├── transcribe.rs     whisper.cpp, and dropping what isn't speech
     ├── overlay.rs        the waveform pill, read from stdin
     └── config.rs         overlay edge in ~/.config/patina-dictate/config.toml
```

The diagrams in `images/` are hand-drawn SVG, and follow the viewer's light or dark theme.

## Bringing it to another desktop

Not something this project sets out to support, but the seams are where you'd cut. The work
comes down to three small parts, and the one that owns the keyboard does the typing:

1. **A trigger.** Something that sees the hotkey go down and up (a compositor, a window
   manager's key grab, or the desktop's own shortcut daemon) and emits "started" and "stopped".
2. **A recorder and transcriber.** Plain program, no windowing at all. Whisper through
   whisper.cpp does a few seconds of speech in well under a second on a modest GPU.
3. **A typer.** The one part that depends on the display server. It should live with whatever
   owns input on that desktop, and take a string. On Patina it saves the clipboard, serves the
   text, presses paste and restores. Other backends (text-input, synthetic keys, xdotool, wtype)
   would fit behind the same `Commit(text)` call.

In the first diagram, everything in the compositor box is specific to this desktop; the daemon
and the overlay would be reusable as they are. The D-Bus band between them is the seam.

## Limits, as they stand

- Wayland windows only. With an X11 window focused, the text is dropped and a line is logged.
  X11 support is deliberately minimal.
- A window that takes over a second to read the clipboard pastes the old contents.
- Apps that bind Ctrl+V to something other than paste do that instead.
- The overlay needs a layer-shell compositor.
