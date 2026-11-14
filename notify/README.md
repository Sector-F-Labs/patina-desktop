# patina-notify

The notification daemon (ADR-0141): apps' notifications, and Ubuntu's update prompts, shown as a
column of cards down the right edge of the screen.

## On the desktop

A notification becomes a card at the top of the column: the app's name, a ✕ in its top right, the
summary, the body and a button for each action. Cards stay until they're dismissed (✕ closes one,
the others stay where they are) unless the app gave the notification a timeout; critical ones never
expire. Clicking a card sends the app its default action, if it gave one, and brings its window forward (the
compositor has no xdg-activation, so the column does it, ADR-0142); the card closes only if a window
came forward, so a click that did nothing leaves it. More cards than fit scroll.

The column keeps its space, as the bar does: while there are cards, tiled and maximised windows
make room for it, and they come back when the last one goes. That's the experiment (ADR-0141);
`enabled = false` in `~/.config/patina-notify/config.toml` turns notifications off, and no space
is kept. While a fullscreen window has the keyboard (a game, a video) they wait, and show when
it's left: no cards and no space. `hide_in_fullscreen = false` shows them anyway (the column is on
the Top layer, so the window still covers it). The compositor's `Fullscreen` property on
`com.sectorflabs.Patina.Input` says when (ADR-0129); nothing needs Night Light on.

Try it with `notify-send -u critical -A open=Open "Updates" "15 updates can be applied"`.

## How it fits in

- **Started by** the compositor, kept running like the polkit agent, on the hardware only. Nested,
  the parent desktop's daemon has the name.
- **Takes** `org.freedesktop.Notifications` on the session bus (`Notify`, `CloseNotification`,
  `GetCapabilities`, `GetServerInformation`, and the signals `NotificationClosed` and
  `ActionInvoked`). While notifications are off the calls still succeed and nothing is shown.
- **The column** is one patina-shell layer surface on the Top layer, anchored top, right and bottom,
  with an exclusive zone as wide as a card plus its margin while it has cards. The compositor works
  the work area out from exclusive zones and tells the tiler when it changes, so it needed no
  change. The part under the last card takes no pointer input.
- **Switched** from the HUD's bell in the tray (struck through while off; there while patina-notify
  is the daemon) and from Settings' Notifications page, which both write the config file
  (`patina_system::notifications`, where the daemon's reading of it is too).
- **Screenshots** arrive as cards (ADR-0143): the compositor announces each saved file with the
  hints `image-path` and `x-patina-file`. A card with the latter shows the picture and can be pulled
  out of the column into an app, which gets the file (`text/uri-list`, and its own type for a PNG or
  JPEG). The card goes once an app takes the drop. Other cards can't be dragged. `screenshots = false` in the config (a checkbox in Settings) turns
  these cards off.
- **Not yet**: icons and images, history, and do-not-disturb.

## Code

`daemon.rs` (the bus), `store.rs` (the cards on show, with their tests), `column.rs` (the surface
and the cards), `config.rs`.
