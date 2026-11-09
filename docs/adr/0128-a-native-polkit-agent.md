# 0128. A native polkit agent asks for the password when an app needs administrator rights

Date: 2026-11-09

## Status

Accepted

## Context

GParted, Disks, Software and `pkexec` ask polkit for administrator rights, and polkit asks an
*authentication agent* in the login session to get the password. A Patina session had none, so
those apps could not run (GParted's launcher calls `pkexec`): it was the first item of
"still to do" in `docs/running-as-a-session.md`. gksu, which people think of for this, is gone from
the distributions, and running graphical apps as root is discouraged (it breaks the user's files
and Wayland access); polkit is how it is done now.

Ubuntu's polkit 127 no longer installs its agent helper setuid. It is a systemd service started
for each connection to `/run/polkit/agent-helper.socket`, which takes the user name and the cookie
as its first two lines, then talks PAM as lines (`PAM_PROMPT_ECHO_OFF Password:`, then the password
back, then `SUCCESS` or `FAILURE`).

## Decision

- **A new crate, `patina-polkit`**, is the agent. The compositor keeps it running, as the bar,
  on the hardware only: nested, the parent desktop's agent is the session's, and polkit allows one.
- It registers `org.freedesktop.PolicyKit1.AuthenticationAgent` on the system bus for the login
  session (`XDG_SESSION_ID`, else logind's `session/auto`), and registers again when polkitd restarts.
- **The dialog is the same program run as `--ask`**, a patina-shell overlay in the skin, started for
  each request and killed when polkit cancels it. It is a process of its own, rather than a
  window in the agent, because a patina-shell host is made for one surface at startup, and a
  crashed or cancelled dialog can't take the agent down. The agent and dialog speak lines over the
  child's stdin and stdout.
- **The password is checked by polkit's helper**, never by us: the socket where it exists, else the
  setuid program. The helper tells polkit when it is right, so the agent only relays prompts and
  answers, asks again after a wrong one, and gives up after five.
- **Authenticates as the user running the session** when polkit lists them (administrators
  authenticate as themselves), else as the first user it lists.

- **Root gets the X display** (`allow_root`, as `xhost +SI:localuser:root`): GParted, run as root by
  `pkexec`, is an X11 program, and Xwayland lets only its own user connect.

## Consequences

- `pkexec`, GParted, Disks and the like work in a Patina session, with a dialog that looks like the
  desktop's.
- Where a desktop already has an agent registered for the session, ours stops with polkit's error
  (the compositor starts it again a few times, then gives up, as with any kept piece).
- Only Linux with polkit: on FreeBSD there is nothing to register with, and the piece gives up
  quietly after its retries. A password is typed into a patina-shell surface the compositor gives
  the keyboard exclusively, as the lock screen does.
- No fingerprint or other non-password PAM prompts: every prompt is shown as a password box.

How the parts fit: `polkit/HOW-IT-WORKS.md`.
