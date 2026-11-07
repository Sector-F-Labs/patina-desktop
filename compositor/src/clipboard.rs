//! The clipboard's history (ADR-0114): every copy the compositor sees, kept so it can be
//! picked again, and so a copy outlives the app it came from. Served on the session bus by
//! `clipboard/dbus.rs` as `com.sectorflabs.Patina.Clipboard`.
//!
//! Every selection goes through the compositor (`SelectionHandler::new_selection`), so
//! nothing here needs a protocol of its own. A copy's data is read out of the app that owns
//! it, every type it offers, the way typing saves the clipboard (`typing.rs`); the entry
//! keeps those bytes, and picking it sets them as the compositor's own selection.
//!
//! Text only so far gets a readable preview; an image is kept and put back as it was, and
//! shown as a line saying what it is until it has a thumbnail.

pub mod dbus;

use std::collections::VecDeque;
use std::io::Read;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use smithay::reexports::calloop::channel;
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::reexports::wayland_server::{Client, Resource};
use smithay::wayland::selection::data_device::{
    SelectionRequestError, clear_data_device_selection, current_data_device_selection_userdata,
    request_data_device_client_selection, set_data_device_selection,
};

use crate::state::State;
use crate::typing::Served;

/// The most entries kept.
const MOST_ENTRIES: usize = 50;
/// The most bytes of one type, and of one entry.
const MOST_TYPE: u64 = 16 << 20;
const MOST_ENTRY: usize = 16 << 20;
/// The most bytes of all the entries together; the oldest images go first.
const MOST_TOTAL: usize = 128 << 20;
/// How long a copy's owner has to hand over what it copied.
const READING: Duration = Duration::from_millis(500);
/// The characters of a text a preview shows.
const PREVIEW: usize = 120;
/// Password managers set this on what they copy, so history leaves it out.
const PASSWORD_HINT: &str = "x-kde-passwordManagerHint";
/// Types X11's clipboard protocol lists beside the data; they carry none.
const PLUMBING: [&str; 7] = [
    "TARGETS",
    "TIMESTAMP",
    "SAVE_TARGETS",
    "MULTIPLE",
    "DELETE",
    "INSERT_PROPERTY",
    "INSERT_SELECTION",
];
const TEXT_TYPES: [&str; 5] = [
    "text/plain;charset=utf-8",
    "text/plain",
    "UTF8_STRING",
    "STRING",
    "TEXT",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    Image,
    Other,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Text => "text",
            Kind::Image => "image",
            Kind::Other => "other",
        }
    }
}

/// One copy: every type it was offered in, with the bytes.
#[derive(Debug, Clone)]
pub struct Entry {
    pub id: u64,
    /// When it was copied, in seconds since 1970.
    pub at: u64,
    pub kind: Kind,
    /// A line saying what it is: the start of a text, "Image (png, 40 KB)".
    pub preview: String,
    pub types: Arc<Vec<(String, Vec<u8>)>>,
    /// Whether its thumbnail file is written (images only).
    pub thumbnail: bool,
}

impl Entry {
    fn bytes(&self) -> usize {
        self.types.iter().map(|(_, b)| b.len()).sum()
    }
}

/// What the bus says of an entry: id, kind, preview, thumbnail path (empty if none), time.
pub type Row = (u64, String, String, String, u32);

#[derive(Debug)]
pub struct History {
    /// Newest first.
    entries: VecDeque<Entry>,
    next: u64,
    /// Off, nothing is recorded.
    pub enabled: bool,
    /// Paused, nothing new is recorded and what's there stays.
    pub paused: bool,
}

impl Default for History {
    fn default() -> Self {
        Self {
            entries: VecDeque::new(),
            next: 1,
            enabled: true,
            paused: false,
        }
    }
}

impl History {
    pub fn recording(&self) -> bool {
        self.enabled && !self.paused
    }

    /// Keeps a copy. The same content again moves its entry to the front. Nothing is kept for
    /// what's empty, or for text of only whitespace. Gives the entry's id.
    pub fn add(&mut self, types: Vec<(String, Vec<u8>)>, now: u64) -> Option<u64> {
        if !self.recording() {
            return None;
        }
        let types = kept_types(types);
        let (kind, preview) = describe(&types)?;
        if let Some(i) = self.entries.iter().position(|e| *e.types == types) {
            let mut entry = self.entries.remove(i).expect("position is in range");
            entry.at = now;
            let id = entry.id;
            self.entries.push_front(entry);
            return Some(id);
        }
        let id = self.next;
        self.next += 1;
        self.entries.push_front(Entry {
            id,
            at: now,
            kind,
            preview,
            types: Arc::new(types),
            thumbnail: false,
        });
        self.trim();
        Some(id)
    }

    /// Over the limits, the oldest go: images and the like before text.
    fn trim(&mut self) {
        self.entries.truncate(MOST_ENTRIES);
        let mut total: usize = self.entries.iter().map(Entry::bytes).sum();
        while total > MOST_TOTAL {
            let heavy = self.entries.iter().rposition(|e| e.kind != Kind::Text);
            let i = heavy.unwrap_or(self.entries.len().saturating_sub(1));
            let Some(gone) = self.entries.remove(i) else {
                break;
            };
            total -= gone.bytes();
        }
    }

    pub fn get(&self, id: u64) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// The newest.
    pub fn latest(&self) -> Option<&Entry> {
        self.entries.front()
    }

    /// Moves an entry to the front, as picking it again makes it the newest.
    pub fn promote(&mut self, id: u64, now: u64) {
        if let Some(i) = self.entries.iter().position(|e| e.id == id) {
            let mut entry = self.entries.remove(i).expect("position is in range");
            entry.at = now;
            self.entries.push_front(entry);
        }
    }

    pub fn delete(&mut self, id: u64) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.id != id);
        self.entries.len() != before
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Notes that entry `id`'s thumbnail is written; false if the entry is gone.
    pub fn set_thumbnail(&mut self, id: u64) -> bool {
        match self.entries.iter_mut().find(|e| e.id == id) {
            Some(entry) => {
                entry.thumbnail = true;
                true
            }
            None => false,
        }
    }

    pub fn ids(&self) -> Vec<u64> {
        self.entries.iter().map(|e| e.id).collect()
    }

    pub fn rows(&self) -> Vec<Row> {
        self.entries
            .iter()
            .map(|e| {
                (
                    e.id,
                    e.kind.name().to_string(),
                    e.preview.clone(),
                    if e.thumbnail {
                        thumbnail_path(e.id).to_string_lossy().into_owned()
                    } else {
                        String::new()
                    },
                    u32::try_from(e.at).unwrap_or(u32::MAX),
                )
            })
            .collect()
    }
}

/// Whether a selection offering `types` is one history keeps out: a password manager's.
pub fn is_private(types: &[String]) -> bool {
    types.iter().any(|t| t == PASSWORD_HINT)
}

/// The types worth asking for, in the order the app offered them: not X11's plumbing, not the
/// password hint, and of the images only the first, which is the one the app prefers.
pub fn wanted(types: &[String]) -> Vec<String> {
    let mut image = false;
    types
        .iter()
        .filter(|t| !PLUMBING.contains(&t.as_str()) && t.as_str() != PASSWORD_HINT)
        .filter(|t| {
            if t.starts_with("image/") {
                !std::mem::replace(&mut image, true)
            } else {
                true
            }
        })
        .cloned()
        .collect()
}

/// What's read, within the size of an entry: types that are empty go, and so do those that
/// would take it over its size.
fn kept_types(types: Vec<(String, Vec<u8>)>) -> Vec<(String, Vec<u8>)> {
    let mut total = 0;
    types
        .into_iter()
        .filter(|(_, b)| !b.is_empty())
        .filter(|(_, b)| {
            total += b.len();
            total <= MOST_ENTRY
        })
        .collect()
}

/// What an entry is, and a line to show for it; none for nothing worth keeping.
fn describe(types: &[(String, Vec<u8>)]) -> Option<(Kind, String)> {
    if let Some((_, bytes)) = types
        .iter()
        .find(|(t, _)| TEXT_TYPES.contains(&t.as_str()))
        .or_else(|| types.iter().find(|(t, _)| t.starts_with("text/")))
    {
        let text = String::from_utf8_lossy(bytes);
        if text.trim().is_empty() {
            return None;
        }
        return Some((Kind::Text, preview(&text)));
    }
    let (first, bytes) = types.first()?;
    if let Some(format) = first.strip_prefix("image/") {
        return Some((
            Kind::Image,
            format!("Image ({format}, {})", size(bytes.len())),
        ));
    }
    Some((Kind::Other, format!("{first} ({})", size(bytes.len()))))
}

/// The start of a text on one line: whitespace squeezed, cut with an ellipsis.
fn preview(text: &str) -> String {
    let squeezed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = squeezed.chars();
    let shown: String = chars.by_ref().take(PREVIEW).collect();
    if chars.next().is_some() {
        format!("{shown}…")
    } else {
        shown
    }
}

fn size(bytes: usize) -> String {
    match bytes {
        0..1024 => format!("{bytes} B"),
        1024..1_048_576 => format!("{} KB", bytes / 1024),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// What a read of a copy brings back, from its thread to the loop.
type Copied = Vec<(String, Vec<u8>)>;

/// What reaches the loop from the threads: a copy's data, or a thumbnail written.
enum Message {
    Copied(Copied),
    Thumbnail(u64),
}

/// The history and what serves it, kept by the state.
#[derive(Default)]
pub struct Clipboard {
    pub history: History,
    service: Option<dbus::Service>,
    reads: Option<channel::Sender<Message>>,
    /// Set by whoever clears the selection on purpose, so the clear isn't taken for an app
    /// quitting, and the latest copy put back.
    pub clearing: bool,
    /// The app that made the latest copy: the one that had the keyboard when it did.
    owner: Option<Client>,
}

impl State {
    /// Serves the history on the bus and takes the copies as they're read; called once at
    /// start-up.
    pub fn start_clipboard(&mut self) {
        // Thumbnails left by an earlier run belong to a history that's gone.
        forget_thumbnails(&[]);
        let (reads, incoming) = channel::channel::<Message>();
        let inserted = self
            .loop_handle
            .insert_source(incoming, |event, _, state| match event {
                channel::Event::Msg(Message::Copied(read)) => state.clipboard_read(read),
                channel::Event::Msg(Message::Thumbnail(id)) => state.clipboard_thumbnailed(id),
                channel::Event::Closed => {}
            });
        if let Err(e) = inserted {
            eprintln!("patina-compositor: clipboard: {e}");
            return;
        }
        self.clipboard.reads = Some(reads);
        let watch = Timer::from_duration(Duration::from_millis(500));
        let _ = self.loop_handle.insert_source(watch, |_, _, state| {
            state.clipboard_watch_owner();
            TimeoutAction::ToDuration(Duration::from_millis(500))
        });
        match dbus::Service::start(&self.loop_handle) {
            Ok(service) => self.clipboard.service = Some(service),
            Err(e) => eprintln!("patina-compositor: the clipboard history isn't on D-Bus: {e}"),
        }
    }

    /// The clipboard's owner changed: an app copied (`Some`, the types it offers) or went
    /// away or cleared it (`None`).
    pub fn clipboard_selected(&mut self, types: Option<Vec<String>>) {
        // Smithay calls this before the new source is the seat's selection, so what the seat
        // holds is the one before it: decide once the loop is idle.
        let owner = self.seat.get_keyboard().and_then(|keyboard| {
            use smithay::wayland::seat::WaylandFocus;
            let focus = keyboard.current_focus()?;
            let surface = focus.wl_surface()?;
            self.display_handle.get_client(surface.id()).ok()
        });
        self.loop_handle
            .insert_idle(move |state| state.clipboard_settled(types, owner));
    }

    fn clipboard_settled(&mut self, types: Option<Vec<String>>, owner: Option<Client>) {
        // What the compositor set itself (typing, a pick, a copy put back) is already
        // somewhere: nothing to keep.
        if current_data_device_selection_userdata(&self.seat).is_some() {
            return;
        }
        let Some(types) = types else {
            self.clipboard_check_gone();
            return;
        };
        self.clipboard.clearing = false;
        self.clipboard.owner = owner;
        if !self.clipboard.history.recording() || is_private(&types) {
            return;
        }
        self.clipboard_read_selection(&types);
    }

    /// Reads what the seat's selection offers, on a thread.
    fn clipboard_read_selection(&mut self, types: &[String]) {
        let wanted = wanted(types);
        let mut pipes = Vec::new();
        for mime_type in wanted {
            let Ok((reader, writer)) = std::io::pipe() else {
                continue;
            };
            if request_data_device_client_selection(&self.seat, mime_type.clone(), writer.into())
                .is_ok()
            {
                pipes.push((mime_type, reader));
            }
        }
        let Some(reads) = self.clipboard.reads.clone() else {
            return;
        };
        if pipes.is_empty() {
            return;
        }
        // The requests wait in the clients' buffers until the loop next flushes them.
        let _ = self.display_handle.flush_clients();
        std::thread::spawn(move || {
            let _ = reads.send(Message::Copied(read_all(pipes, Instant::now() + READING)));
        });
    }

    /// A copy's data arrived.
    fn clipboard_read(&mut self, read: Copied) {
        if let Some(id) = self.clipboard.history.add(read, now()) {
            self.clipboard_thumbnail(id);
            self.clipboard_changed();
        }
    }

    /// Makes entry `id`'s thumbnail, if it's a PNG image and has none, on a thread.
    fn clipboard_thumbnail(&mut self, id: u64) {
        let Some(entry) = self.clipboard.history.get(id) else {
            return;
        };
        if entry.thumbnail || entry.kind != Kind::Image {
            return;
        }
        let Some(bytes) = entry
            .types
            .iter()
            .find(|(t, _)| t == "image/png")
            .map(|(_, b)| b.clone())
        else {
            return;
        };
        let Some(reads) = self.clipboard.reads.clone() else {
            return;
        };
        std::thread::spawn(move || {
            let Some(small) = thumbnail(&bytes) else {
                return;
            };
            let path = thumbnail_path(id);
            let written = path
                .parent()
                .is_some_and(|dir| std::fs::create_dir_all(dir).is_ok())
                && std::fs::write(&path, small).is_ok();
            if written {
                let _ = reads.send(Message::Thumbnail(id));
            }
        });
    }

    fn clipboard_thumbnailed(&mut self, id: u64) {
        if self.clipboard.history.set_thumbnail(id) {
            self.clipboard_changed();
        }
    }

    /// Watches the app that copied: Smithay drops a quit app's selection without telling the
    /// handler, so every half second look whether its connection is still there.
    fn clipboard_watch_owner(&mut self) {
        let Some(owner) = &self.clipboard.owner else {
            return;
        };
        let gone = self
            .display_handle
            .backend_handle()
            .get_client_credentials(owner.id())
            .is_err();
        if !gone {
            return;
        }
        self.clipboard.owner = None;
        if !self.clearing_on_purpose()
            && current_data_device_selection_userdata(&self.seat).is_none()
        {
            self.clipboard_put_back();
        }
    }

    fn clearing_on_purpose(&self) -> bool {
        self.clipboard.clearing
    }

    /// Puts the latest copy back if nothing is on the clipboard and nobody cleared it on
    /// purpose.
    fn clipboard_check_gone(&mut self) {
        if self.clipboard.clearing {
            return;
        }
        let (_, probe) = match std::io::pipe() {
            Ok(pipe) => pipe,
            Err(_) => return,
        };
        let nothing = matches!(
            request_data_device_client_selection(&self.seat, String::new(), probe.into()),
            Err(SelectionRequestError::NoSelection)
        );
        if nothing {
            self.clipboard_put_back();
        }
    }

    /// The app that owned the clipboard is gone: the latest copy goes back on it, so it
    /// doesn't vanish with its app.
    fn clipboard_put_back(&mut self) {
        if !self.clipboard.history.recording() {
            return;
        }
        let Some(latest) = self.clipboard.history.latest() else {
            return;
        };
        let served = Served::Saved(latest.types.clone());
        self.clipboard.clearing = false;
        set_data_device_selection(&self.display_handle, &self.seat, served.types(), served);
    }

    /// Sets an entry as the clipboard, and makes it the newest.
    pub fn clipboard_pick(&mut self, id: u64) {
        let Some(entry) = self.clipboard.history.get(id) else {
            return;
        };
        let served = Served::Saved(entry.types.clone());
        self.clipboard.clearing = false;
        set_data_device_selection(&self.display_handle, &self.seat, served.types(), served);
        self.clipboard.history.promote(id, now());
        self.clipboard_changed();
    }

    pub fn clipboard_delete(&mut self, id: u64) {
        let was_latest = self.clipboard.history.latest().is_some_and(|e| e.id == id);
        if self.clipboard.history.delete(id) {
            if was_latest {
                self.clipboard_drop_served();
            }
            self.clipboard_changed();
        }
    }

    pub fn clipboard_clear(&mut self) {
        self.clipboard.history.clear();
        self.clipboard_drop_served();
        self.clipboard_changed();
    }

    /// Empties the clipboard if it holds a copy the compositor put back, so deleting a copy
    /// from history doesn't leave it pasteable.
    fn clipboard_drop_served(&mut self) {
        let ours = matches!(
            current_data_device_selection_userdata(&self.seat).as_deref(),
            Some(Served::Saved(_))
        );
        if ours {
            self.clipboard.clearing = true;
            clear_data_device_selection(&self.display_handle, &self.seat);
        }
    }

    pub fn clipboard_enable(&mut self, enabled: bool) {
        self.clipboard.history.enabled = enabled;
        self.clipboard_changed();
    }

    pub fn clipboard_pause(&mut self, paused: bool) {
        self.clipboard.history.paused = paused;
        self.clipboard_changed();
    }

    /// Tells the bus what the history holds now.
    fn clipboard_changed(&self) {
        forget_thumbnails(&self.clipboard.history.ids());
        if let Some(service) = &self.clipboard.service {
            service.publish(dbus::Status {
                rows: self.clipboard.history.rows(),
                enabled: self.clipboard.history.enabled,
                paused: self.clipboard.history.paused,
            });
        }
    }
}

/// The most pixels along a thumbnail's longer side.
const THUMBNAIL: u32 = 96;
/// Images with more pixels than this get no thumbnail: decoding them would take memory.
const MOST_PIXELS: u64 = 64 << 20;

/// Where entry `id`'s thumbnail is: `$XDG_RUNTIME_DIR/patina/clipboard/<id>.png`.
pub fn thumbnail_path(id: u64) -> std::path::PathBuf {
    thumbnail_dir().join(format!("{id}.png"))
}

fn thumbnail_dir() -> std::path::PathBuf {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    runtime.join("patina").join("clipboard")
}

/// Removes thumbnails of entries that are gone, and any left by an earlier run.
fn forget_thumbnails(kept: &[u64]) {
    let Ok(dir) = std::fs::read_dir(thumbnail_dir()) else {
        return;
    };
    for file in dir.flatten() {
        let path = file.path();
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.parse::<u64>().ok());
        if id.is_none_or(|id| !kept.contains(&id)) {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// A PNG's shrunk copy, as PNG, at most `THUMBNAIL` along its longer side; `None` when it
/// isn't a PNG this can read or is too big.
fn thumbnail(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let info = reader.info();
    if u64::from(info.width) * u64::from(info.height) > MOST_PIXELS {
        return None;
    }
    let mut frame = vec![0; reader.output_buffer_size()?];
    let out = reader.next_frame(&mut frame).ok()?;
    let (w, h) = (out.width as usize, out.height as usize);
    if w == 0 || h == 0 {
        return None;
    }
    let channels = match out.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return None,
    };
    let rgba = |x: usize, y: usize| -> [u32; 4] {
        let at = (y * w + x) * channels;
        let p = &frame[at..at + channels];
        let c = match channels {
            1 => [p[0], p[0], p[0], 255],
            2 => [p[0], p[0], p[0], p[1]],
            3 => [p[0], p[1], p[2], 255],
            _ => [p[0], p[1], p[2], p[3]],
        };
        c.map(u32::from)
    };
    let scale = (w.max(h) as f64 / f64::from(THUMBNAIL)).max(1.0);
    let (tw, th) = (
        ((w as f64 / scale).round() as usize).max(1),
        ((h as f64 / scale).round() as usize).max(1),
    );
    let mut small = Vec::with_capacity(tw * th * 4);
    for ty in 0..th {
        let (y0, y1) = (ty * h / th, ((ty + 1) * h / th).max(ty * h / th + 1));
        for tx in 0..tw {
            let (x0, x1) = (tx * w / tw, ((tx + 1) * w / tw).max(tx * w / tw + 1));
            let mut sum = [0u32; 4];
            let mut n = 0;
            for y in y0..y1.min(h) {
                for x in x0..x1.min(w) {
                    let c = rgba(x, y);
                    for i in 0..4 {
                        sum[i] += c[i];
                    }
                    n += 1;
                }
            }
            small.extend(sum.map(|v| (v / n.max(1)) as u8));
        }
    }
    let mut png_bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut png_bytes, tw as u32, th as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().ok()?.write_image_data(&small).ok()?;
    Some(png_bytes)
}

/// Each type's bytes, as many as arrive by `deadline`: an app that never answers is left
/// behind, and its type with it.
fn read_all(pipes: Vec<(String, std::io::PipeReader)>, deadline: Instant) -> Copied {
    let count = pipes.len();
    let (send, read) = std::sync::mpsc::channel();
    for (i, (mime_type, reader)) in pipes.into_iter().enumerate() {
        let send = send.clone();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            if reader.take(MOST_TYPE).read_to_end(&mut bytes).is_ok() {
                let _ = send.send((i, mime_type, bytes));
            }
        });
    }
    drop(send);
    let mut got = Vec::with_capacity(count);
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match read.recv_timeout(left) {
            Ok(one) => got.push(one),
            Err(_) => break,
        }
    }
    got.sort_by_key(|(i, _, _)| *i);
    got.into_iter().map(|(_, t, b)| (t, b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Vec<(String, Vec<u8>)> {
        vec![("text/plain;charset=utf-8".into(), s.as_bytes().to_vec())]
    }

    fn png(n: usize) -> Vec<(String, Vec<u8>)> {
        vec![("image/png".into(), vec![7; n])]
    }

    fn picture(w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        let mut e = png::Encoder::new(&mut out, w, h);
        e.set_color(png::ColorType::Rgb);
        e.set_depth(png::BitDepth::Eight);
        let data: Vec<u8> = (0..w * h).flat_map(|_| [200, 30, 30]).collect();
        e.write_header().unwrap().write_image_data(&data).unwrap();
        out
    }

    #[test]
    fn a_thumbnail_is_a_shrunk_png() {
        let small = thumbnail(&picture(400, 200)).expect("a thumbnail");
        let d = png::Decoder::new(std::io::Cursor::new(&small[..]));
        let r = d.read_info().unwrap();
        assert_eq!((r.info().width, r.info().height), (96, 48));
        // A small picture stays as it is, and what isn't a PNG gets none.
        let same = thumbnail(&picture(20, 10)).unwrap();
        let r = png::Decoder::new(std::io::Cursor::new(&same[..]))
            .read_info()
            .unwrap();
        assert_eq!((r.info().width, r.info().height), (20, 10));
        assert!(thumbnail(b"not a png").is_none());
    }

    #[test]
    fn a_copy_is_kept_newest_first() {
        let mut h = History::default();
        h.add(text("one"), 1);
        h.add(text("two"), 2);
        let rows = h.rows();
        assert_eq!(rows[0].2, "two");
        assert_eq!(rows[1].2, "one");
        assert_eq!(rows[0].1, "text");
    }

    #[test]
    fn the_same_copy_again_moves_up_and_keeps_its_id() {
        let mut h = History::default();
        let first = h.add(text("one"), 1).unwrap();
        h.add(text("two"), 2);
        assert_eq!(h.add(text("one"), 3), Some(first));
        assert_eq!(h.rows().len(), 2);
        assert_eq!(h.latest().unwrap().id, first);
        assert_eq!(h.latest().unwrap().at, 3);
    }

    #[test]
    fn nothing_is_kept_for_empty_or_blank_copies() {
        let mut h = History::default();
        assert_eq!(h.add(text(""), 1), None);
        assert_eq!(h.add(text("  \n\t "), 1), None);
        assert_eq!(h.add(vec![], 1), None);
        assert!(h.rows().is_empty());
    }

    #[test]
    fn a_preview_is_one_short_line() {
        let long = format!("first\n\n  second   {}", "x".repeat(300));
        let mut h = History::default();
        h.add(text(&long), 1);
        let p = &h.rows()[0].2;
        assert!(p.starts_with("first second xxx"), "{p}");
        assert_eq!(p.chars().count(), PREVIEW + 1);
        assert!(p.ends_with('…'));
    }

    #[test]
    fn an_image_says_what_it_is() {
        let mut h = History::default();
        h.add(png(3000), 1);
        let row = &h.rows()[0];
        assert_eq!(row.1, "image");
        assert_eq!(row.2, "Image (png, 2 KB)");
    }

    #[test]
    fn only_so_many_are_kept_and_the_oldest_go() {
        let mut h = History::default();
        for i in 0..MOST_ENTRIES + 5 {
            h.add(text(&format!("copy {i}")), i as u64);
        }
        let rows = h.rows();
        assert_eq!(rows.len(), MOST_ENTRIES);
        assert_eq!(rows[0].2, format!("copy {}", MOST_ENTRIES + 4));
        assert_eq!(rows.last().unwrap().2, "copy 5");
    }

    #[test]
    fn when_the_total_is_too_big_images_go_before_text() {
        let mut h = History::default();
        h.add(text("keep me"), 1); // the oldest
        for i in 0..9 {
            let mut big = png(MOST_ENTRY - 1);
            big[0].1[0] = i; // each different
            h.add(big, 2 + u64::from(i));
        }
        let rows = h.rows();
        assert!(rows.iter().any(|r| r.2 == "keep me"), "the text stayed");
        let bytes: usize = h.entries.iter().map(Entry::bytes).sum();
        assert!(bytes <= MOST_TOTAL);
    }

    #[test]
    fn an_entry_over_its_size_loses_the_type_that_takes_it_over() {
        let mut h = History::default();
        let huge = vec![0u8; MOST_ENTRY];
        h.add(
            vec![
                ("text/plain".into(), b"small".to_vec()),
                ("text/html".into(), huge),
            ],
            1,
        );
        let e = h.latest().unwrap();
        assert_eq!(e.types.len(), 1);
        assert_eq!(e.types[0].0, "text/plain");
    }

    #[test]
    fn delete_and_clear_forget() {
        let mut h = History::default();
        let a = h.add(text("a"), 1).unwrap();
        h.add(text("b"), 2);
        assert!(h.delete(a));
        assert!(!h.delete(a));
        assert_eq!(h.rows().len(), 1);
        h.clear();
        assert!(h.rows().is_empty());
    }

    #[test]
    fn promote_makes_an_older_entry_the_newest() {
        let mut h = History::default();
        let a = h.add(text("a"), 1).unwrap();
        h.add(text("b"), 2);
        h.promote(a, 5);
        assert_eq!(h.latest().unwrap().id, a);
    }

    #[test]
    fn a_paused_or_disabled_history_keeps_nothing_new() {
        let mut h = History::default();
        h.add(text("before"), 1);
        h.paused = true;
        assert_eq!(h.add(text("during"), 2), None);
        assert_eq!(h.rows().len(), 1, "what was there stays");
        h.paused = false;
        h.enabled = false;
        assert_eq!(h.add(text("off"), 3), None);
    }

    #[test]
    fn a_password_managers_copy_is_private() {
        let types: Vec<String> = vec!["text/plain".into(), PASSWORD_HINT.into()];
        assert!(is_private(&types));
        assert!(!is_private(&["text/plain".to_string()]));
        assert_eq!(
            wanted(&types),
            ["text/plain"],
            "the hint is never asked for"
        );
    }

    #[test]
    fn of_the_offered_types_plumbing_and_extra_images_are_left_out() {
        let offered: Vec<String> = [
            "image/png",
            "image/bmp",
            "TARGETS",
            "text/uri-list",
            "TIMESTAMP",
        ]
        .map(String::from)
        .into();
        assert_eq!(wanted(&offered), ["image/png", "text/uri-list"]);
    }
}
