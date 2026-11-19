# 0004. Mail never runs code or fetches anything it wasn't asked to

Date: 2026-10-18

## Status

Accepted

## Context

Most mail is HTML, and Mail shows it as flattened text (ADR-0001), which is poor for newsletters
and receipts. Showing it properly needs an HTML renderer, and which one is acceptable depends on
what a message is allowed to do.

Mainstream clients (Thunderbird, Apple Mail, Gmail) turn JavaScript off for mail, but run it in
full browser engines where the script engine and network stack are still built in: a setting or a
bug is all that stands between a message and running code or calling home. Beyond scripts, mail
tracks and attacks readers through remote images and CSS (`url()`, `@import`, web fonts), links
whose text names one site and whose target is another, forms, meta refreshes, iframes and
embedded objects.

Options considered for rendering: richer text conversion only (safe, but loses layout); embedding
WebKitGTK through `wry` (full fidelity, but a whole browser with scripting, and a poor fit with
eframe's window on Wayland); a script-less Rust HTML/CSS engine such as Blitz, or litehtml.

## Decision

Mail will never run code from a message, and never contact anything other than Bridge unless the
user asks it to, for that message:

- No JavaScript: the renderer must not contain a script engine at all, not merely have it off.
  This rules out WebKit and other full browser engines.
- No network by default: the renderer may only load resources from inside the message (`cid:`
  parts and `data:` URLs). Remote images, stylesheets and fonts load only when the user asks, per
  message, and Mail says what was blocked.
- Forms, meta refreshes, iframes, objects and embeds are removed or ignored.
- Links open only on a click, with the real address shown, and a warning when the link's text
  looks like a different domain from its target.
- Attachments are never opened or rendered automatically; saving one stays an explicit action.

## Consequences

- An HTML renderer is only acceptable if its resource loading can be handed to Mail, so Mail can
  refuse everything that isn't in the message. That is the test the Blitz spike must pass.
- Messages that rely on remote images look broken until the user loads them, as in Proton's own
  web app.
- Some fidelity is given up for good: anything that needs scripts, and whatever a script-less
  engine lays out differently from a browser.
- The plain-text view stays available as a fallback for whatever the renderer can't handle.
