# 0142. Clicking a notification raises its app, and a card goes only if something happened

Date: 2026-11-14

## Status

Accepted. Refines [0141](0141-notifications-are-a-column-at-the-right-edge-that-keeps-its-space.md),
which had a click run the card's `default` action and then close the card.

## Context

Clicking a Telegram card closed it and brought nothing forward. The `default` action is a signal to
the app (`ActionInvoked`), and a Wayland app can only raise its own window with an xdg-activation
token, which the specification has the notification server pass along (`ActivationToken`). The
compositor does not implement xdg-activation, so Telegram was told to open and could not. A card
that disappears with nothing visibly happening is also wrong in itself: the user expects a click
that does nothing to leave things as they were.

## Decision

- **The column raises the app's window itself**, through foreign-toplevel as the bar's tabs do. The
  window is the one whose app id contains the notification's `desktop-entry` hint or its app name,
  compared as lowercase letters and digits ("Telegram Desktop" finds
  `telegram-desktop_telegram-desktop`), or the other way round; names under three letters are
  ignored.
- **A click on the card** sends `default` if the app offered it, raises the window if one is found,
  and **closes the card only if a window was raised.** Without one it stays, and ✕ is how it goes.
- **A button** runs its action and closes the card: the app does something with it.

## Consequences

- Cards from apps with no window (a background service) stay when clicked, which is what the user
  asked for, but never close by clicking.
- The match is by name, so an app whose window and notification names differ entirely is not found
  until its desktop entry says so.
- xdg-activation in the compositor would let apps raise themselves, and the `ActivationToken`
  signal could be sent; this does not replace that.
