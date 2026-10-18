# 0005. Render HTML mail with Blitz

Date: 2026-10-18

## Status

Proposed

## Context

Mail shows HTML mail as text converted by html2text (ADR-0001), which loses layout. ADR-0004
requires any renderer to contain no script engine, and to load nothing except what Mail's own
resource loader hands it.

A spike (`spikes/blitz`, on the `spike/blitz` branch) tried Blitz `0.3.0-beta.4`: Stylo (Firefox's
CSS engine), Taffy layout, html5ever parsing, no JavaScript, painted on the CPU by vello_cpu into an
egui texture. Its resource loading (`NetProvider`) and navigation (`NavigationProvider`) are traits
the embedding app implements, and its `EventDriver` handles hover, selection and clicks.

Results: all 8 remote loads in a hostile test newsletter were refused through Mail's provider, and
scripts, meta refreshes and forms did nothing; layout fidelity was close to a browser's; a message
took 25–70 ms to lay out and 3–8 ms to paint. Problems: fixed-width mail overflows narrow panes; a
Blitz bug drops table cells restyled by mobile `@media` rules; blocked images show no alt text.

Considered and rejected: WebKitGTK (ruled out by ADR-0004); keeping text only (too poor for
newsletters and receipts); litehtml (no maintained Rust bindings found, and unspiked).

## Decision

We will render HTML mail with Blitz, pinned to an exact version:

- A `NetProvider` that serves only the message's `cid:` parts and `data:` URLs, answers everything
  else with empty bytes, and records what it refused; the base URL is `cid:/`.
- A `NavigationProvider` that hands link clicks to Mail, which shows the real target and warns
  when the link text looks like another domain; form submissions are dropped.
- Mail's own user-agent stylesheet: sans-serif body, forms/iframes/objects/embeds hidden.
- Mail laid out at the wider of the pane and its own width, zoomed down to fit.
- Painted on the CPU, only the visible part of the message, on a white page whatever the skin.
- A bar saying how many remote items were blocked, with a per-message "Load" that is a separate,
  later decision.
- The text view stays, as a View menu choice and the fallback for plain-text mail.

## Consequences

- HTML mail looks close to how its sender meant it, with ADR-0004's rules enforced by Mail's own
  code rather than an engine setting.
- About 100 more crates (mostly Stylo) and a ~15 MB bigger binary.
- Blitz is beta: upgrades will need work, and its bugs become ours to work around or report.
- When a second Patina app needs HTML, the renderer should move into Patina.
