# 0006. Remote images load one at a time, by choice, with tracking pixels flagged

Date: 2026-10-18

## Status

Accepted

## Context

ADR-0004 lets Mail fetch remote content only when the user asks, per message, and ADR-0005 left
how to ask for later. Most newsletters lean on remote images, so they look broken until loaded.

Loading any remote image tells its server that the message was opened, when, and from which IP
address (Bridge doesn't proxy images the way Proton's web app does). A 1×1 tracking pixel is the
well-known case, but a normal-looking logo with a per-recipient URL tells the sender just as much.
An image's real size can't be known without fetching it, and the fetch is what tracks, so any
judgement has to come from what the HTML declares before anything is fetched.

Considered: one "Load images" button per message (simple, but loads trackers along with
everything else); loading everything automatically except images that look tiny (pleasant, but
undoes ADR-0004 for every tracker that doesn't declare itself tiny); per-image choice (chosen).

## Decision

- Before Blitz sees a message, Mail lists its remote `<img>`s (http, https and `//` URLs) with what
  each declares: width and height from attributes or inline style, and whether it's hidden
  (`hidden`, `display:none`, `visibility:hidden`, `opacity:0`).
- An image declaring a width or height of 4 px or less, or hidden, is a suspected tracking pixel.
  It is replaced by a small "Suspected tracking pixel" box linking to EFF's explanation
  (<https://www.eff.org/deeplinks/2019/01/stop-tracking-my-emails>), and isn't fetched unless the
  user picks it in the list.
- Every other remote image shows in place as a placeholder box naming its server. Clicking the box
  loads that image.
- A bar says how many remote images are blocked, and Review… lists them: server, full address,
  declared size, and flags ("tiny", "hidden", "looks like a tracker" for addresses with long
  unique-looking IDs or words such as open, track, pixel, beacon). The user picks which to load.
  Unflagged images start ticked, flagged ones unticked. The list says plainly that no flags doesn't
  mean an image can't track.
- Approved images are fetched off the UI thread: images only (by content type), no cookies, no
  referrer, a 10 MB limit, 15 s timeout. Mail's `Guard` then serves exactly those URLs for that
  message; everything else stays refused, including CSS backgrounds, stylesheets and fonts.
- Choices last only while the message is open. Remembering them per sender is a later decision.

## Consequences

- Newsletters can be made whole a picture at a time, and the user sees which server each one
  would tell.
- Tracking pixels that declare themselves are caught before they're fetched; ones that don't
  still need the user to recognise them, which the flags only help with.
- Mail makes outbound requests for the first time, from a background thread, and only to
  addresses the user approved for the message in front of them.
- Approved images are fetched again each time the message is opened.
- Remote CSS backgrounds can't be approved yet; they stay blocked.
