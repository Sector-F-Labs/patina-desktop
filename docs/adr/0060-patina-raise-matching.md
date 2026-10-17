# 0060. patina-raise matches as written, with -i and -p, and by app names

Date: 2026-10-17

## Status

Accepted. The user asked for `--partial` and `--insensitive`, so that
`patina-raise telegram` with them finds the right app.

## Context

patina-raise (ADR-0058) matched a window's app id, or the last part of a reverse-DNS id, always
ignoring case. That missed the apps the user wanted next:

- **Telegram**, installed as a snap, has the window id `telegram-desktop_telegram-desktop`, and
  there's no `telegram` command to start it.
- **WhatsApp** is a Chromium web app. Its window id is
  `chrome-hnpfjngllnobngcgfapefoaidbinmjnm-Default`, and only its `.desktop` entry's Name,
  "WhatsApp Web", says what it is.

The `.desktop` entries tie a window's app id to a name and to the command that starts the app.
The bar already read them, for its tab icons and New Window (ADR-0050, ADR-0055).

## Decision

- **Matching is whole and as written by default.** `-i` (`--insensitive`) ignores case, and
  `-p` (`--partial`) matches anywhere. They combine as `-ip`, before the app's name. Without
  them, the old behaviour of always ignoring case would have made `-i` a no-op.
- **A window also matches by its app's name.** That's the Name of the entry its app id leads to,
  so `patina-raise -ip whatsapp` finds the web app.
- **With no command, it starts the matched entry's Exec**, in a terminal if the entry says so,
  else the name as a command. So `-ip telegram` starts `/snap/bin/telegram-desktop`.
- **The entries code moves from the bar to `patina_system::apps`**, shared by both. Entries now
  carry their Name and plain Exec too.
- `--list` shows each window's app name beside its id.

## Consequences

- Short, forgiving names work for apps whose ids are long or opaque, and start them when they
  aren't open.
- Plain matching is now case-sensitive, so a binding that relied on the old behaviour needs `-i`.
  The user's Alt+T became `patina-raise -i teleprinter`. Alt+B's default, `firefox`, is
  unaffected.
- A partial match can find more than one app: `-p code` would match any id containing "code".
  The windows are taken in the bar's order, and the first matching entry is started.
- Each run reads the installed entries, a few milliseconds.
