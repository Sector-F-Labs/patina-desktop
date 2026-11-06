# 0012. Show images on their own line as embedded blocks, copying in ones from outside

Date: 2026-10-13

## Status

Proposed. The user asked for image support; the scope and the copying rule are my design.

## Context

Images were kept as raw `![alt](src)` text (ADR-0004), so a note with pictures showed markdown
syntax. Most images in notes stand alone in their own paragraph, as Obsidian, GitHub and the
example notes write them. An image inside a sentence is rare, and it can't sit in a text galley
the way the editor lays text out (ADR-0002).

When an image is added, it has to live somewhere. We considered:

- **Linking the file wherever it is.** This breaks the note when you tidy `~/Downloads`, and it
  ties the note to one machine.
- **Copying into an `attachments/` or hidden folder.** That folder would show up in the sidebar,
  or be hidden from other tools.
- **Copying next to the note.** The sidebar only lists notes, so the copy doesn't clutter it, the
  link is just the file name, and the note and its pictures move together in a file manager.

## Decision

- **An image alone in a top-level paragraph** becomes `Block::Image` / `Para::Image`. This
  doesn't apply in lists or quotes. It keeps the alt text, the source as written, and the title,
  and is written back as `![alt](src "title")`. Images inside text stay raw.
- **Shown as an embed** (see `embeds::image`): at its own size, scaled down to the column and to
  640 px tall. The description (alt text) can be edited underneath it. Its right-click menu has
  Open Image, Copy Image Path and Remove Image. A missing file says "Image not found: <src>".
- **Sources:** relative paths are resolved from the note's folder, and `%20`-style escapes are
  decoded. Absolute paths and `file://` work too. egui_extras loads PNG, JPEG, GIF, WebP and BMP.
- **Adding images:** Edit → Insert Image…, the toolbar button, or dropping image files on the
  window. A file already inside the workspace is linked by its relative path. Any other file is
  copied next to the note first (`name 2.png` if the name is taken). Dropping a `.md` file opens
  it.

## Consequences

- Notes with pictures read as they will look elsewhere, and adding a picture never leaves a link
  to a stray file.
- There's no http loader, so remote (`https://`) images show egui's error mark, with the reason
  on hover. Adding one would make Notebook fetch from the network when a note is opened.
- Images can't be pasted from the clipboard yet. egui only gives us pasted text.
- An image file changed on disk keeps showing the old picture until restart, because egui caches
  it by URI.
- An image with styled alt text loses the styling, since the description is plain text.
