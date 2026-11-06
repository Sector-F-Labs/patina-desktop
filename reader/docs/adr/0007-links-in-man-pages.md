# 0007. References in man pages are links

Date: 2026-10-16

## Status

Accepted

## Context

The user asked for links in man pages to work where possible, to get to examples and the like.
Formatted pages are plain text: references are only text that looks like one.

## Decision

Reader finds, in a page's text, references to other pages (`name(section)`, section 1 to 9 with
optional letters), web addresses, and absolute paths that exist. They're drawn in the link colour,
underlined, the pointer becomes a hand over them, and a click (not a drag) follows them: a page
through `man -w`, a web address in the browser, a path Reader shows in Reader, any other path
(a folder of examples, a configuration file) in the desktop's app for it, Files for a folder.
Man's own underlining is drawn underlined in the text colour, so it isn't mistaken for a link.

## Consequences

- SEE ALSO sections and the pages' examples are a click away.
- A reference broken across lines, or a path that doesn't exist here, stays plain text.
