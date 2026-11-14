# 0141. Notifications are a column at the right edge that keeps its space

Date: 2026-11-14

## Status

Accepted. The user chose the shape below and asked for it to be experimental: notifications are
meant to be really there, and Settings turns them off for anyone who finds that too much.

## Context

Nothing owned `org.freedesktop.Notifications` in a Patina session, so every app's notification,
Ubuntu's update prompts included, went nowhere (`docs/ROADMAP.md`). The updates come from
`update-notifier`, which `/etc/xdg/autostart` starts and which raises its prompts through libnotify,
as do the Ubuntu Pro (ESM) notice, the reboot-required notice and the disk notifier. A daemon that
implements the interface properly, with actions, is all they need.

The usual design is a pop-up that floats over the windows for a few seconds. The user wants the
opposite: notifications as something close to a HUD element, there until dismissed, one card under
another down the right edge, each with its own close button, and tiled windows making room for them
the way they do for the bar. "Notifications should be annoying or not there at all", hence a switch.
They may later hold other things, clipboard entries to drag out, which is why the column is its own
surface and not a list in a menu.

## Decision

We will add `patina-notify`, a program kept running by the compositor, like `patina-polkit`.

- **It owns `org.freedesktop.Notifications`** on the session bus: `Notify`, `CloseNotification`,
  `GetCapabilities` (body, body-markup, actions), `GetServerInformation`, and the signals
  `NotificationClosed` and `ActionInvoked`. Markup in a body is shown as plain text.
- **The column is one layer-shell surface**, anchored top, right and bottom, as wide as a card plus
  its margin, with an exclusive zone of that width while there is something to show. The compositor
  already derives the work area from exclusive zones and tells the tiler when it changes, so tiled
  and maximised windows move over and come back without a change to the compositor. Floating
  windows are left alone, as they are for the bar. The surface is on the Top layer, so a fullscreen
  window still covers it. The part of the surface below the last card takes no pointer input.
- **A card** has the app's name, a close button (the ✕ in its top right), the summary, the body, and
  a button for each action. Clicking the card runs its `default` action. Cards stack from the top
  in the order they came; a notification that replaces another (`replaces_id`) keeps its place. More
  than fit scroll.
- **A notification stays until it is dismissed**, unless the app gave it a timeout. The server
  default is "no timeout", which the specification allows. Critical ones never expire.
- **It can be switched off.** `~/.config/patina-notify/config.toml` has `enabled` (default true),
  checked each second. While it is off nothing is shown and no space is kept, but the name stays
  owned and the calls succeed, so apps don't fail or fall back to something else.
- **One process** both serves the bus and draws. The polkit split (an agent and a dialog it starts)
  is for a dialog that lives for one request; the column is one surface for the whole session, and
  the bus thread wakes the host's loop through a channel when something changes.
- **Not nested**, like polkit: the parent desktop's daemon has the name there.

## Consequences

- Update prompts, Ubuntu Pro, `notify-send` and every app's notifications appear. The prompts will
  only come from `update-notifier` once something runs `/etc/xdg/autostart`, which Patina doesn't
  yet (a separate item in the roadmap).
- Notifications take space, and tiled windows jump when one arrives or is dismissed. That is the
  point; Settings' switch is the answer to anyone who disagrees. If it proves too much, the zone
  could be kept only while the pointer is over the column, or only for critical ones.
- A first version has no icons or images, and no do-not-disturb or history. The tray's toggle and
  Settings' page come next and write the same config file.
- Dragging things out of the column is not built; the surface being its own is what leaves it open.
