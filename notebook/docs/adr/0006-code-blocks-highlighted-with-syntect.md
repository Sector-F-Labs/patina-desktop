# 0006. Embed code blocks with a language picker, highlighted by syntect through egui_extras

Date: 2026-10-11

## Status

Proposed. The embedded code block carries over from the GTK repo's ADR-0006. Replacing
GtkSourceView (its ADR-0007) with syntect is my choice.

## Context

The GTK version embedded code blocks with a language dropdown and a copy button, highlighted by
GtkSourceView. GtkSourceView doesn't exist in egui. `egui_extras` has a highlighter with a small
built-in tokenizer, or syntect behind its `syntect` feature.

## Decision

- **The paragraph:** consecutive code lines of one language are gathered into one `Para::Code`.
- **The widget:** `embeds::code` draws a framed block in the code background colour, with:
  - a language picker listing common fence tags (an unlisted tag is kept and shown as it is);
  - Copy and Delete buttons;
  - a multiline code `TextEdit`.
- **Highlighting:** `egui_extras::syntax_highlighting` with the `syntect` feature, which follows
  the light/dark theme. Fence tags are mapped to the extensions syntect looks syntaxes up by, e.g.
  `rust` to `rs`, `python` to `py`.
- **Making one:** "Monospaced" in the paragraph-style menu turns the selected paragraphs into a
  code block.

## Consequences

- Highlighting for every language syntect bundles, with no system libraries needed.
- syntect adds build time and binary size, along with its bundled syntax definitions.
- The highlighting palette comes from egui_extras' code theme, not from the skin.
