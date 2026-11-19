# 0002. Podcasts come from their RSS feeds, found through Apple's search

Date: 2026-10-23

## Status

Proposed

## Context

A podcast is published as an RSS feed, with an `<enclosure>` per episode linking its audio and
iTunes tags for artwork, duration and so on. Directories such as Apple Podcasts list feeds but
don't host them. To subscribe, the app needs a feed URL; to be pleasant, it needs a way to find
one by name.

Ways considered to find feeds:

- **Apple's iTunes Search and Lookup APIs** (`itunes.apple.com/search` and `/lookup`): no key, no
  account, the largest directory, and `feedUrl` in every answer. The lookup also turns an Apple
  Podcasts link (`…/id1494880178`) into its feed, which is how most people share podcasts.
- **Podcast Index**: open and complete, but it needs an API key and request signing, so every
  user, or the app, would need credentials.
- **Only pasted feed URLs**: works, but almost nobody knows a podcast's feed URL.

## Decision

- Shows and episodes come from the feeds themselves, parsed with the `rss` crate and its iTunes
  extension. A show is identified by the feed URL it was subscribed with; an episode by its guid,
  or its audio URL without one.
- Searching in the sidebar asks Apple's Search API. Pasting a link subscribes to it, through
  Apple's Lookup API when it's an Apple Podcasts link and directly otherwise.
- Feeds are cached in `~/.cache/podcasts/shows.json` and fetched again when older than an hour or
  when asked (Ctrl+R).

## Consequences

- No accounts or keys. Apple sees searches, and each podcast's host sees its feed being fetched,
  as with any podcast app.
- Apple's API is undocumented as a contract and rate limited (about 20 requests a minute). Search
  as you type waits for each answer before asking again, which stays well under that.
- Podcasts Apple doesn't list can still be added by their feed URL.
- Paid or private feeds work only if their URL carries their credentials.
