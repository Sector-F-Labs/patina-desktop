# 0039. Night Light: the compositor warms the screen through the CRTC's gamma ramp, on the hardware only

Date: 2026-10-16

## Status

Accepted

## Context

The user wanted a Night Light like GNOME's: the screen warms in the evening. Three ways to tint it
were considered:

- **The CRTC's gamma ramp** in the DRM backend, as GNOME and wlroots do. The display controller
  applies it at scanout, so it costs no rendering, keeps direct scanout and the cursor plane, and
  covers everything, the lock screen included. Screenshots stay neutral.
- **A colour-matrix shader pass** in the GL backends. This would show nested and in the VM, but it
  costs a full-screen pass every frame, blocks direct scanout, and wouldn't reach the pixman
  fallback.
- **Both:** gamma on the hardware, and a shader when nested.

The user ruled out the nested cases. Nested on another desktop, or in a VM, the machine underneath
has its own night light, so tinting there is pointless. That leaves the gamma ramp on its own.

Schedules: the user asked for manual times first, built so that sunset to sunrise (from a location
typed in, or a location service) can come later.

The user also wanted outside apps to be able to turn it off, for example a photo editor that needs
true colour. GNOME offers a single "disable until tomorrow" switch. KWin offers D-Bus
`Inhibit`/`Uninhibit` with a cookie. wlr-gamma-control lets a client take over the ramp entirely. A
switch that apps flip leaves the screen stuck if the app crashes, and two apps asking at once undo
each other. A held inhibitor, dropped when its holder goes away, has neither problem.

## Decision

- **Where:** the compositor owns Night Light (`compositor/src/night_light.rs`). The udev backend
  sets the CRTC's gamma ramp through the legacy `set_gamma` call, which the kernel turns into the
  atomic `GAMMA_LUT` beside Smithay's atomic commits. It restores the original ramp on exit. When
  the session comes back from another console it applies the ramp again, because gamma doesn't
  survive the switch. A CRTC with no gamma (`gamma_size` 0, for example virtio-gpu) makes Night
  Light unavailable. The nested backends ignore Night Light.
- **The tint:** a colour temperature from 6500 K (neutral, the ramp left alone) down to 1700 K. The
  colour comes from a blackbody approximation (Tanner Helland's fit, as redshift and gammastep use),
  scaled so the brightest channel stays at full. Each channel of the ramp the CRTC had is scaled,
  so a calibrated ramp stays calibrated. It fades over a minute at a schedule's edge, and over a
  second when it's asked for (a setting, an override, an app inhibiting it).
- **The setting:** `[night_light]` in the compositor's config: `mode` (`"off"`, `"always"` or
  `"schedule"`), `from` and `to` (`"HH:MM"`, local time, which can wrap past midnight) and
  `temperature`. The compositor checks the file's modification time every second, as the bar does
  its own, and reloads the whole config when it changes, as Super+Shift+R does. So Settings
  (ADR-0038) only writes the file, and every other setting in it now applies on saving too.
- **Schedules** are a `Schedule` trait: "given now, is it warm, and when does that next change?"
  `Manual(from, to)` is the only one so far. A sun-times schedule is a second implementation, and
  the fade and the gamma code don't change.
- **Outside control: inhibitors.** Night Light is on when the mode and schedule say so, **and**
  nothing inhibits it, **and** no override says otherwise. It's served on the session bus as
  `com.sectorflabs.Patina.NightLight` at `/com/sectorflabs/Patina/NightLight`:
  - `Inhibit(reason) → cookie` and `Uninhibit(cookie)`, shaped like KWin's. An inhibitor held by a
    caller that leaves the bus is dropped, so a crash can't leave the screen neutral.
  - `SetOverride(state)`: `"auto"`, `"on"` or `"off"` until the schedule next changes (GNOME's
    "Disable until tomorrow", and its opposite).
  - Properties `Active`, `Available`, `Inhibitors` (the reasons given), `Temperature` and
    `Override`, with change signals, for the bar and Settings.

  The `patina-night-light` command (`on`, `off`, `auto`, `status`, and `inhibit -- CMD`, which
  holds an inhibitor while a command runs) is a client of it, built with Settings. The client
  side, for any Rust piece or app, is `patina_system::night_light`. Its D-Bus side runs on its own thread
  and talks to the event loop over a calloop channel, as the sleep inhibitor in `lock.rs` does.
- **Next:** a Wayland protocol of Patina's own, tied to a surface and modelled on idle-inhibit:
  "while this surface is in front, no Night Light". Patina apps would get it through patina-ui, with
  Photograph first. wlr-gamma-control, so that gammastep and wlsunset can take the ramp over while
  ours steps aside, may come later.

## Consequences

- No rendering cost, and it works the same on every frame and plane.
- It can't be seen in either test VM: the FreeBSD one has no KMS, and virtio-gpu has no gamma. The
  maths is unit-tested here, but checking it on screen needs real hardware, the laptop or a text
  console.
- One monitor only, like the rest of the udev backend. More monitors means one ramp per CRTC.
- The legacy gamma call is used beside atomic commits. Should a driver reject it, the backend would
  have to set `GAMMA_LUT` itself as part of a commit, which Smithay's `DrmCompositor` doesn't expose
  today.
- Apps can only ask for "off", not for a different warmth. That's on purpose: the warmth belongs to
  the user.
- The compositor now owns a D-Bus name on the session bus. Without a session bus (some FreeBSD
  setups), Night Light still runs, but nothing outside the compositor can control it.
