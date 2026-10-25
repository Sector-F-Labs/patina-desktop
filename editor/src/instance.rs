//! One window (ADR-0005): a second `text-editor FILE` hands its files to
//! the one running, over a socket in `$XDG_RUNTIME_DIR`, and exits. Two
//! windows would each keep a session and overwrite the other's.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};

/// The socket, one per data folder, so a test run with its own data
/// folder is a window of its own.
fn socket() -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    dirs::data_dir().hash(&mut hasher);
    let dir = dirs::runtime_dir().unwrap_or_else(std::env::temp_dir);
    dir.join(format!("text-editor-{:08x}.sock", hasher.finish() as u32))
}

/// Gives `files` to the window already running. Whether there was one.
pub fn forward(files: &[PathBuf]) -> bool {
    let Ok(mut stream) = UnixStream::connect(socket()) else { return false };
    let message: String = files.iter().map(|f| format!("{}\n", f.display())).collect();
    // An empty message still asks the window to come forward.
    stream.write_all(if message.is_empty() { b"\n" } else { message.as_bytes() }).is_ok()
}

/// Listens for files from later starts; each batch arrives on the
/// receiver and wakes the window.
pub fn listen(ctx: &egui::Context) -> Option<Receiver<Vec<PathBuf>>> {
    let path = socket();
    // A socket nobody answers on was left by a crash.
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)
        .inspect_err(|e| eprintln!("text-editor: can't listen on {}: {e}", path.display()))
        .ok()?;
    let (send, receive) = channel();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let files: Vec<PathBuf> = BufReader::new(stream)
                .lines()
                .map_while(Result::ok)
                .filter(|l| !l.is_empty())
                .map(PathBuf::from)
                .collect();
            if send.send(files).is_err() {
                break;
            }
            ctx.request_repaint();
        }
    });
    Some(receive)
}

/// Stops listening, as the window closes.
pub fn release() {
    let _ = std::fs::remove_file(socket());
}
