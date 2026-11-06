# 0003. Set note text in the bundled Ubuntu variable fonts

Date: 2026-10-11

## Status

Proposed. Bold and italic text were required by the WYSIWYG decision. The font choice is mine.

## Context

egui ships one proportional font, Ubuntu Light, with no bold or italic face. Its `italics` flag
slants glyphs, and there is no synthetic bold. A notes editor showing bold as nothing at all isn't
WYSIWYG.

Patina's design language says "stock egui first", meaning no bundled fonts unless the app needs
them.

egui 0.35 supports variable-font axes (`TextFormat::coords`). Ubuntu ships as variable fonts with
a `wght` axis, plus a separate italic face, under the Ubuntu Font Licence.

## Decision

- **Bundle** `Ubuntu-Variable.ttf` and `Ubuntu-Italic-Variable.ttf` (about 1.7 MB) in
  `assets/fonts/`, with the licence in `UFL.txt`.
- **Register them** as two font families of their own.
- **Use them for note text only:** body text at weight 300, which matches egui's stock Ubuntu
  Light; bold at 700; italics from the italic face. Emoji and symbols fall back to egui's fonts.
- **Leave everything else stock:** the rest of the UI keeps egui's fonts, and inline code keeps
  egui's monospace.

## Consequences

- Real bold and italics, in the same family as the rest of the UI, so note text doesn't look
  foreign.
- The binary is about 1.7 MB bigger.
- It depends on egui's variable-font support, which is new in 0.35. If it regresses, bold
  disappears.
- Skins can't change the note font. A `[app.notebook]` skin section could add that later.
