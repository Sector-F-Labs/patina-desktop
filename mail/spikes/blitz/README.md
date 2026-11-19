# Blitz spike

Can [Blitz](https://github.com/DioxusLabs/blitz) (`0.3.0-beta.4`) render HTML mail inside Mail's
egui window under [ADR-0004](../../docs/adr/0004-no-code-no-network-reader.md)'s rules: no code,
nothing loaded from outside the message?

```
cargo run --release --bin render -- corpus/newsletter.html out/newsletter.png 700
cargo run --release --bin viewer            # the corpus in an egui window
cargo test --release                        # links, selection, scripts/forms
```

`corpus/` is made-up mail written to be hostile: a table-layout newsletter with a remote
stylesheet, `@import`, web font, CSS background image, remote images, a tracking pixel, an iframe,
a form, a script, a meta refresh and a link whose text names a different site; a receipt; a
Gmail-style reply. No real mail was used.

## How it fits together

- `Guard` (a Blitz `NetProvider`) is the only way the engine loads anything. It serves `cid:` parts
  and `data:` URLs and refuses the rest, answering with empty bytes. Dropped requests stay
  "pending critical resources" and hold back layout, so a refused stylesheet must still be answered.
  The base URL is `cid:/`, so relative URLs can't reach the network either.
- `Links` (a `NavigationProvider`) receives link clicks and form submissions. Nothing is followed;
  the app decides.
- A user-agent stylesheet of Mail's own goes after Blitz's default one: a sans-serif body, and
  forms, iframes, objects and embeds `display: none`.
- Painting goes through `anyrender_vello_cpu` into an RGBA buffer, uploaded as an egui texture.
- egui pointer events go to Blitz's `EventDriver`, which does hover, text selection and link clicks;
  `get_selected_text()` serves Ctrl+C; `get_cursor()` gives the pointer shape.

## Findings

**It works, and it meets ADR-0004.**

- Every one of the newsletter's 8 remote loads went through the guard and was refused (stylesheet,
  `@import`, font, background, logo, hero image, iframe, tracking pixel); the `cid:` image was
  served. The script and meta refresh made no request and no navigation: there is no script engine
  to run them.
- Fidelity is good: nested tables, inline styles, `border-radius`, the button, blockquote bars,
  lists and the receipt's right-aligned columns all match what a browser shows (`out/*.png`).
- Speed, release build, one CPU-painted 700 px wide message: parse + style + layout 25–70 ms, paint
  3–8 ms. Fine for opening a message; repainting the whole message on every pointer move is
  wasteful for long mail, so the real thing should paint only the visible part (`paint_scene` takes
  offsets).
- Link clicks arrive with the real target, so the "text says proton.me, link goes elsewhere"
  warning is easy. Drag-selection and copying work.

**Problems found**

- Fixed-width mail (`width="600"`) overflows a narrower pane and is cut off. Fix: lay out at the
  wider of the pane and the mail's own width, and zoom it down to fit, as Apple Mail does (Blitz's
  `Viewport` has a zoom).
- Blitz bug: the newsletter's own mobile CSS (`@media (max-width:600px) { .col { display:block
  !important } }` on table cells) makes that row disappear entirely below 600 px. Zoom-to-fit
  avoids it by never laying out below 600 px; worth reporting upstream with `corpus/newsletter.html`.
- Blocked images show nothing, not even `alt` text. Showing a "N images blocked — Load" bar is
  Mail's job anyway, but alt text would help.
- CSS floats are behind Blitz's `floats` feature (off by default); turned on here.
- `hover_node_id` is private (there's `get_hover_node_id()`) and `Node::is_element_with_tag_name`
  isn't public: the API is beta and has rough edges.

**Costs**

- About 100 more crates than Mail already has (519 against 413), most of them Stylo, Firefox's CSS
  engine. The viewer binary is ~15 MB bigger than Mail's (unstripped).
- Blitz is a beta (`0.3.0-beta.4`): expect API changes between releases. Pin the exact version.
- Dark mode: Blitz has a colour-scheme setting, but most mail assumes a white page. Like most
  clients, Mail should render mail on white regardless of skin, at least to start.

## Recommendation

Adopt Blitz for HTML mail ([ADR-0005](../../docs/adr/0005-render-html-mail-with-blitz.md),
Proposed), with zoom-to-fit, visible-area painting, a "remote content blocked" bar with a per-message
Load button, and the plain-text view kept as a fallback. Once a second Patina app needs to show
HTML, move the renderer into Patina.
