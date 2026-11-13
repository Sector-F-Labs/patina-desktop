# patina-search

What the Patina launcher and Settings share for searching files (ADR-0137): the config in
`~/.config/patina-search/config.toml` (whether to search, and which folders, each by name with fd,
by contents with ripgrep, or both), and the runs of fd and ripgrep that find the files.

## How it fits in

- **Settings'** Search page edits the config (`config::read`, `config::write`).
- **The launcher** reads it each time it opens and calls `engine::search` on a thread, a moment
  after the typing pauses.
- **fd and ripgrep** are found on `$PATH` (`fdfind` too, as Debian names it); a run that takes more
  than 1.5 s is killed, and a run stops once it has read 20 lines.

## Code

`config.rs`: the config and its file, the default folders (XDG's, so a localised home works);
`engine.rs`: the commands, reading their output, and combining names and contents.

```sh
cargo test -p patina-search   # with fd and ripgrep installed, it searches a folder of its own
```
