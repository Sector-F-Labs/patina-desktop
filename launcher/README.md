# patina-launcher

The app launcher: a menu of every installed app, opened from the bar's Sector F button or the
Super key, which filters as you type and starts the one you pick.

## On the desktop

It opens at the bar's left end, like a menu from its button. The apps are listed by name with
their icons; typing filters them by name, then words of the name, then the generic name,
keywords, comment and program. Up and Down choose, Enter or a click starts one, Escape, a click
elsewhere or the button again closes it.

## How it fits in

- **Started by** the bar's launcher button (with `PATINA_SKIN` and `PATINA_BAR_EDGE`) or the
  compositor's Super key (which leaves the skin and edge to the bar's config). A second run
  closes the open one over its socket (patina-shell's `Single`), so one button toggles it.
- **Where apps come from** (`apps.rs`, ADR-0010): the `.desktop` files in every XDG data
  directory, the user's first, honouring `NoDisplay`, `Hidden`, `TryExec` and
  `OnlyShowIn`/`NotShowIn` for the desktop name `Patina`. And folder apps, `~/Apps/Name.app`
  with an `app.toml` (`folders.rs`, ADR-0035), read by the shared
  patina-apps crate that Files uses too (its own repository on remotes, ADR-0036): copy one in and
  it's listed, delete it and it's gone.
- **Starting an app:** the command line from the entry (field codes removed), in a terminal if
  it asks for one (`$TERMINAL`, else foot), in its own process group so it outlives the
  launcher. Folder apps run from the home folder.
- **Icons** (`icons.rs`) are looked up in the icon theme (Adwaita, then hicolor and pixmaps) or
  taken from the app's folder, and decoded on a thread, so the list shows at once.
- **Drawn** on a layer-shell surface with the keyboard, through patina-shell.

## Code

`main.rs` (one copy, the surface, icons arriving), `ui.rs` (the list and search box), `apps.rs`
(.desktop entries, search, launching), `folders.rs` (folder apps through patina-apps),
`icons.rs`.

## Running

```sh
cargo run -p patina-launcher      # on a layer-shell compositor, e.g. inside make run
```
