# How an app gets administrator rights in Patina

Open GParted from the launcher and a small dialog appears in the middle of the screen: what is being
asked, who you are, a box for your password. Type it, and GParted opens, running as root. This page
explains the parts behind that, which are polkit, a program called an *agent*, and a root helper,
and why Patina has its own agent. The README next to it covers running; the decision is ADR-0128.

## The idea in one paragraph

A graphical app can't just become root: it would need a password, and apps must not see it. The
standard arrangement (polkit) splits the job in three. The app, or `pkexec` on its behalf, asks
**polkitd** (the system's decision-maker) whether it may; polkitd says "ask the user", and sends
that to an **agent**, a program in the user's login session whose only job is to show a password
box; and the password is checked by a small **helper** that runs as root and tells polkitd the
answer itself. So no program that draws anything ever has to be trusted with root, and the program
that is root never draws anything. Patina's piece is the agent, `patina-polkit`. The rule that shapes
it: **the agent never checks a password**. It carries the helper's questions to a dialog and
the typed answer back.

## The picture

<img src="images/architecture.svg" alt="The app, or pkexec, asks polkitd; polkitd calls the agent, patina-polkit, registered for the login session; the agent starts the dialog and talks to polkit's root agent helper, which checks the password with PAM and tells polkitd. The compositor keeps the agent running and lets root onto the X server.">

### What each part is

| Part | Who | What it does |
|------|-----|--------------|
| `pkexec`, or an app using polkit | the app | Asks polkitd for the right to do something as root; waits. |
| **polkitd** | system service | Decides who may do what from its rules. When the answer is "if you authenticate", it hands the request to the agent registered for the app's login session. |
| **The agent**, `patina-polkit` | in your session, kept running by the compositor | Registered with polkitd for this session. Gets each request and runs the conversation. Never sees whether a password is right, only what the helper says. |
| **The dialog**, `patina-polkit --ask` | a child of the agent | A patina-shell overlay in the skin. Shows the message and the box; says what was typed. |
| **polkit's agent helper** | system, root | Runs the PAM conversation and, when the password is right, tells polkitd itself. |

## One request, in time

<img src="images/sequence.svg" alt="pkexec asks polkitd, which calls BeginAuthentication on the agent. The agent starts the dialog and opens a conversation with the helper. The helper's password prompt goes to the dialog, the typed password goes back through the agent to the helper, and PAM checks it. On success the helper tells polkitd and the agent, the dialog closes, BeginAuthentication returns and pkexec runs the app as root.">

A wrong password makes the helper say `FAILURE`; the agent tells the dialog (the box shakes and
shows "Sorry, that didn't work"), opens a new helper conversation with the same cookie, and asks
again. After five it gives up and refuses. Cancel, Escape, or polkitd cancelling the request
(`pkexec` was killed) closes the dialog and refuses.

## The details that matter

**Registering.** At start the agent calls `RegisterAuthenticationAgent` on the system bus with its
login session (`XDG_SESSION_ID`, else asked of logind as `session/auto`) and an object path. Only
one agent can be registered per session, which is why the compositor doesn't start it when running
nested inside another desktop: that desktop's agent is the session's. polkitd forgets its agents when
it restarts, so the agent watches for the name coming back and registers again.

**The cookie.** polkitd gives each request a random cookie. It is how the helper, polkitd and the
agent agree they mean the same request; the agent hands it to the helper and never interprets it.

**Who authenticates.** polkitd lists who may approve (here, the administrators). The agent picks the
user running the session if listed, else the first user listed, and the helper does PAM for that
user. The dialog says "as divan" so it is plain.

**The helper's two forms.** On current polkit (Ubuntu 26.04, polkit 127) the helper is not setuid.
It is a socket-activated systemd service at `/run/polkit/agent-helper.socket`; the agent connects,
sends the user name and the cookie as two lines, then reads PAM's prompts as lines
(`PAM_PROMPT_ECHO_OFF Password:`) and answers with the password as a line, ending with `SUCCESS` or
`FAILURE`. On older polkit the same lines go over a pipe to a setuid
`polkit-agent-helper-1`, with the user name as its argument and no first line. The agent tries the
socket first.

**Why the dialog is a second process.** A patina-shell host is made once, for one surface, and
blocks; an agent has to stay up and answer the bus while a dialog is open, and while another
request might arrive. So each request runs the same program again as `--ask`, which talks to the
agent in lines on its stdin and stdout (`prompt`, `info`, `wrong`, `ok` one way; `password` the
other). A dialog that crashes or is killed takes nothing else down, and cancelling is killing it.

**The password.** It is typed into a surface the compositor gives the keyboard exclusively, as the
lock screen does, kept as a plain string rather than in a text edit (whose undo history would hold
copies), shown only as dots, sent to the agent in one line, and from there to the helper. Each side
overwrites its copy with zeros once it is sent. It is never logged.

## When root opens a window: X11

GParted is an X11 program, and `pkexec` runs it as *root*. Xwayland, the X server the compositor
starts for X apps, only lets its own user connect, so a root GParted died with `cannot open
display` and "no authorization protocol specified", even though the password had been right. GNOME
fixes this by allowing root on the X server (what `xhost +SI:localuser:root` does); the compositor
does the same through the X protocol as soon as Xwayland is ready (`allow_root` in
`compositor/src/xwayland.rs`). Wayland-native apps run as root don't need it, as they use the
user's socket.

## What the dialog doesn't do yet

Every prompt is shown as a password box, so there is no fingerprint reader or other PAM prompts. The
dialog never shows the app's icon, which polkit sends (`icon_name`). There is no "remember for a
while": that is polkit's own, per action, and applies without anyone asking.

## Where to look

| Thing | Where |
|-------|-------|
| The agent: bus, registering, the helper's lines, picking the user | `agent.rs` |
| The dialog and its line protocol | `dialog.rs` |
| Starting it with the session | `compositor/src/main.rs` (`keep_client("patina-polkit")`) |
| Root on the X server | `compositor/src/xwayland.rs` (`allow_root`) |
| Why | ADR-0128 |

To try it from inside a Patina session: `pkexec id`. Over ssh on a test machine, start commands in the
session with the compositor's `Spawn` (`busctl --user call com.sectorflabs.Patina.Processes
/com/sectorflabs/Patina/Processes com.sectorflabs.Patina.Processes Spawn "sa{ss}" "pkexec id" 0`),
since polkit sends the request to the agent of the *caller's* session, not yours.
