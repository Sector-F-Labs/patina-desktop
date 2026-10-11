# 0007. Use freebsd.org's deep red for the Calendar skin and the icon

Date: 2026-10-11

## Status

Accepted. Changes the colour in [ADR-0006](0006-own-calendar-skin-as-default.md).

## Context

The Calendar skin's title bar used the icon's red-orange, `#D9481A`. The user asked for a darker
red, like the header of <https://www.freebsd.org/>. Its stylesheet sets the header to `#AB2B28`,
with white text.

## Decision

- **Skin:** the title bar, accent (today and the selection) and link colours move to `#AB2B28`
  and its shades. Hover is `#BE3B37`, and pressed and the border are `#8A2220`.
- **Icon:** the header and today's square move to the same red, so skin and icon still match.

## Consequences

- The skin and icon read as one deep red, closer to FreeBSD's than to Patina's Dark Orange.
- ADR-0006 stands, except for the colour.
