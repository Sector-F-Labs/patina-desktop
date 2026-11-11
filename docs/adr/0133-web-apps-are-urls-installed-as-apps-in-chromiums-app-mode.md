# 0133. Web apps are URLs installed as apps, opened in a WebKitGTK window the compositor frames in the site's colour

Date: 2026-11-11

## Status

Accepted. The user asked for it, moved the engine from Chromium's `--app` to a WebKitGTK host of
our own after trying Chromium and Firefox's kiosk mode (below), and asked for the frame to follow the
page's colour and to have the thin lighter border Patina's Firefox theme has.

## Context

The user wants what Chrome's "install as app" does: put a URL in a list and get an app, such as
WhatsApp Web, that looks like part of the desktop. It should have the same rounded corners and
frame as any window, and eventually the Patina look inside the page too.

The compositor already frames any Wayland window that takes xdg-decoration's server side
(ADR-0085), with rounded corners and a shadow (ADR-0112). A browser surface therefore needs
nothing of its own from the compositor: the web view is a rectangle, and the frame goes round it.

Tested on 2026-11-11 in a live Patina session: snap Chromium 156 with `--ozone-platform=wayland
--app=https://web.whatsapp.com` and a throwaway profile opened chromeless and was given Patina's
title bar and corners, and dragging, resizing, maximising and closing all worked, with no
compositor change.

Ruled out or put off:

- **WebKitGTK/WPE in a host of our own:** a smaller, fully controlled window, but weaker site
  compatibility (WhatsApp, DRM, WebRTC), and GTK doesn't speak xdg-decoration. Kept as a second
  engine for later, when the page should be skinned.
- **CEF:** a Chromium we ship, which is large, hard to package and not on FreeBSD.
- **Firefox:** it no longer has a site-specific-browser mode.
- **An engine inside the compositor:** a page that crashes would take the desktop with it.

## Decision

- **A web app is a name and a URL** in `~/.config/patina-webapp/apps.toml`, with a slug
  (`google-meet`) from the name, an engine (WebKit unless it says `chromium`), and what the site
  said about itself: its theme `color` and an `icon` file. The `patina-webapp` crate (a library,
  and the `patina-webapp` binary) owns the list.
- **Adding one installs it:** it writes `~/.local/share/applications/patina-webapp-<slug>.desktop`
  (`Exec=patina-webapp launch <slug>`, `Icon=` the downloaded icon), so the launcher, the bar and
  Alt+Tab list it like any app. Removing one deletes the entry, the icon and the profile. The
  Settings page "Web apps" does both, and `patina-webapp add|refresh|remove|list|sync` does them
  from a shell.
- **The colour and icon are read on install and on refresh, and stored**, so launching never
  waits on the network or asks a browser: the manifest's `theme_color` (else the page's
  `<meta name="theme-color">`, else the manifest's background) and the smallest bitmap icon of
  128 pixels or more. They're read with `curl`; a site that won't answer it (WhatsApp Web
  answers 400) is read by `patina-webview probe`, which loads the page in a window that's never
  shown, as the app itself would, and reports its tags and manifest. Settings has a Refresh
  button per app, and reads a new one after Add, on a thread.
- **`patina-webview SLUG` is the default engine:** a GTK 4 window around a WebKitGTK 6 web view,
  with the app-id `patina-webapp-<slug>`, a profile of its own, links to other sites sent to
  `xdg-open`, no browser chrome. `patina-webapp launch` replaces itself with it. Chromium's
  `--app` (`engine = "chromium"`) stays as the engine for a site WebKit can't run.
- **The compositor frames a web app's window by its app-id** (`frames::is_web_app`), whatever
  the engine says about decorations. The title bar is the site's colour, with text and buttons
  that read on it and a border a little darker (`patina_webapp::look`, shared with the host so
  the two agree); the compositor follows the list file's changes within a second.
- **The host rounds the bottom corners itself**, as Patina's own apps do: the compositor's
  renderer is software (pixman) and can't clip a client's surface, so the page sits in a box with
  a one-pixel border, `border-radius` and `Overflow::Hidden` (a GTK widget property: GTK 4's CSS
  has no `overflow`), on a transparent window. The compositor draws no side border for it.

## Consequences

- A web app needs `patina-webview` (so WebKitGTK 6 to build: `libwebkitgtk-6.0-dev`), and
  `curl` to read a site's icon. Chromium, as a snap, works
  but is confined by AppArmor (D-Bus calls are denied), so notifications and portals may not, and it can't write to a hidden folder: a snap engine's profiles go in `~/snap/<name>/common/patina-webapp/<slug>` instead of `~/.local/share/patina-webapp/<slug>`.
- The engine is chosen at launch, so adding WebKitGTK later changes one function.
- Removing a web app deletes its logins: Settings asks first.
- The host can't read the skin, so its corner radius is Patina's default (15): a skin with another
  radius isn't followed at the bottom. The colour is the site's, so a site with a light colour
  gets a light frame; it can't be overridden yet.
- WebKit has no Widevine and some sites need more than it does; those get Chromium.
- Reading with `patina-webview probe` needs a display, so a refresh on a headless machine reads
  only what `curl` can.
- The `webkit6` crate (0.5, with `gtk4` 0.10) doesn't build with `gtk4` 0.11.
- The app-id is Chromium's own naming, which could change; for an address with a path the entry
  has no `StartupWMClass`, and only the suffix is reliable.
- Not done: the Patina skin inside a page, and following the skin's radius.

## Revisit when

- A skin with another corner radius, or a dark frame on a light site, is wanted.
- Pages are to be skinned: the WebKit host can inject the skin as CSS variables.
