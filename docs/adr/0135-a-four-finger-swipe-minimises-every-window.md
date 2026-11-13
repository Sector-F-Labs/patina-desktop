# 0135. A four-finger swipe toward the window list's edge minimises every window, and one away from it brings them back

Date: 2026-11-13

## Status

Accepted. The user asked for it.

## Context

Minimising is one window at a time (Super+M, the window's button, [0084](0084-minimise-to-the-bars-tabs.md)).
To get at the desktop, or to hide everything for a moment, that is a keypress per window. macOS and
GNOME both give the touchpad a swipe for it. libinput reports four-finger swipes as gestures, as it does
three-finger ones, which three-finger drag already takes ([0031](0031-three-finger-drag.md)); four
fingers weren't used.

Ways to do it:

- **Act when the fingers lift**, as the swipe ends. The window list would change only after the
  fingers are up, which feels late, and a swipe that wandered would count by where it ended.
- **Follow the fingers** with an animation, as GNOME's overview does. The compositor had no
  animations for windows; minimising and restoring got one in [0136](0136-minimising-and-restoring-are-animated.md),
  but it doesn't follow the fingers.
- **Act once the swipe has gone far enough.** Chosen.

## Decision

- Four fingers moving 60 logical pixels toward the edge the HUD's window list is attached to, more
  along that way than across it (twice as much), minimise every window. The same distance away from
  the edge brings back the windows that swipe minimised. With the list on the right, in any of its
  spots (right, top right, bottom right), that's a swipe right to minimise and left to bring back;
  on the bottom, down and up. It acts once for each swipe, so the fingers can stay down, and a swipe
  toward and then away in one go does the first only. The user's reason: the windows go toward the
  list, so the gesture goes with them. (The first version was down and up, whatever the list.)
- The edge is read from the window list's layer surface on the monitor the pointer is on, when the
  fingers go down: a surface taller than it is wide is on the left or right, whichever it's nearer,
  and one that's wider on the top or bottom. With no window list there, the bottom.
- The windows are remembered in the order they were stacked, and put back in that order, so the one
  that was on top is on top again and has the keyboard. A window brought back by hand, or closed, in
  between is left as it is. A swipe up with nothing remembered does nothing: it doesn't bring back
  windows minimised some other way. Windows opened after the swipe stay.
- X11 override-redirect windows (menus and tooltips) aren't minimised.
- `four_finger_swipe` in `[touchpad]` turns it off; on by default.
- Directions are the fingers moving on the pad, whatever natural scrolling is set to, as three-finger
  drag follows the fingers.

## Consequences

- The tabs in the HUD show the minimised windows, as for any other minimise, so a single one can be
  brought back from its tab.
- Fullscreen windows are minimised too: a game under a four-finger swipe is suspended
  as for [0044](0044-smithay-from-a-fork-for-x11-focus.md)'s minimise.
- The swipe is a one-off at a distance, not a gesture that follows the fingers; a short one does nothing.
- The distance and the 2:1 ratio are guesses, to be tried on the touchpad; I haven't tried them on one.
- Four fingers are no longer free for another gesture (workspaces, say); that would have to take
  another number of fingers or a direction.
