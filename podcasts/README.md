# Podcasts

<img src="docs/icon.png" alt="" width="96" align="right">

A podcast player for the Linux desktop. Search for a podcast or paste its link, subscribe, and
play episodes as they download, picking up where you left off. Built on egui and
[Patina](https://github.com/Sector-F-Labs/patina). Podcasts are read straight from their public
RSS feeds, and found through Apple's podcast search (no account or key needed).

- **Subscribe** by searching in the sidebar, or by pasting a feed URL or an Apple Podcasts link
  (such as `https://podcasts.apple.com/us/podcast/…/id1494880178`).
- **Episodes** newest first, with their date, length, how much is left, and their show notes.
  Click one to read its notes; double-click or press its ▶ to play.
- **Plays while it downloads**: an episode starts in about a second and is kept for offline
  listening. Right-click an episode to download it ahead, or delete the download.
- **Picks up where you left off** in every episode, and marks an episode played when it ends.
- Patina's skins, including the shared ones.

## Shortcuts

| Keys | Does |
|------|------|
| Space | Play or pause |
| ← → | Back 15 seconds, forward 30 |
| Ctrl+F or Ctrl+L | Search for a podcast to add |
| Enter in the search | Subscribe to the first result, or to the pasted link |
| Ctrl+R | Refresh every podcast |
| Ctrl+Q | Quit |

Right-click a podcast in the sidebar to refresh it, mark it all played, copy its feed link or
unsubscribe. Right-click an episode to mark it played or unplayed, download it or delete its
download.

## Build

```bash
make            # list the targets
make run        # run with your own settings
make dev        # run in a scratch config with a couple of podcasts
make check      # tests and clippy
make install    # install for the current user, with a launcher (PREFIX=~/.local)
```

Settings are in `~/.config/podcasts/config.toml`. Downloaded episodes and where you are in each
are in `~/.local/share/podcasts/`, and feeds and artwork are cached in `~/.cache/podcasts/`.

## Licence

BSD-3-Clause. Podcasts and their artwork belong to their publishers.
