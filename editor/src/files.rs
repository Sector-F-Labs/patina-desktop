//! Reading and writing files: UTF-8 with or without a byte-order mark,
//! Unix or Windows line ends kept as they were, and saves that replace the
//! file whole, so a crash mid-save never leaves half of one (ADR-0007).

use std::hash::{Hash, Hasher};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const BOM: &str = "\u{feff}";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Ending {
    #[default]
    Lf,
    CrLf,
}

impl Ending {
    pub fn label(self) -> &'static str {
        match self {
            Ending::Lf => "LF",
            Ending::CrLf => "CRLF",
        }
    }
}

/// A file as read, ready to edit: line ends made `\n`.
#[derive(Clone, Debug)]
pub struct Loaded {
    pub text: String,
    pub ending: Ending,
    pub bom: bool,
    /// It wasn't UTF-8: bytes that weren't are shown as �, and saving would
    /// lose them.
    pub lossy: bool,
}

/// A fingerprint of a text, to tell whether it's what was saved.
pub fn digest(text: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

pub fn decode(bytes: Vec<u8>) -> Loaded {
    let (text, lossy) = match String::from_utf8(bytes) {
        Ok(text) => (text, false),
        Err(e) => (String::from_utf8_lossy(e.as_bytes()).into_owned(), true),
    };
    let (text, bom) = match text.strip_prefix(BOM) {
        Some(rest) => (rest.to_string(), true),
        None => (text, false),
    };
    // Windows line ends if most of them are.
    let crlf = text.matches("\r\n").count();
    let ending = if crlf > 0 && crlf * 2 >= text.matches('\n').count() { Ending::CrLf } else { Ending::Lf };
    let text = if crlf > 0 { text.replace("\r\n", "\n") } else { text };
    Loaded { text, ending, bom, lossy }
}

pub fn encode(text: &str, ending: Ending, bom: bool) -> Vec<u8> {
    let mut out = String::with_capacity(text.len() + 3);
    if bom {
        out.push_str(BOM);
    }
    match ending {
        Ending::Lf => out.push_str(text),
        Ending::CrLf => out.push_str(&text.replace('\n', "\r\n")),
    }
    out.into_bytes()
}

pub fn read(path: &Path) -> io::Result<Loaded> {
    Ok(decode(std::fs::read(path)?))
}

/// Writes `bytes` to `path`. Through a new file renamed over the old one,
/// keeping its permissions, where that's safe; in place where it isn't (a
/// folder that can't be written, a file owned by someone else or linked
/// from elsewhere), which keeps whatever the file is to others.
pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    // A link is followed, so the link stays one.
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let existing = std::fs::metadata(&path).ok();
    if existing.as_ref().is_some_and(must_write_in_place) {
        return write_in_place(&path, bytes);
    }
    match replace(&path, bytes, existing.as_ref()) {
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied && existing.is_some() => write_in_place(&path, bytes),
        other => other,
    }
}

#[cfg(unix)]
fn must_write_in_place(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    // SAFETY: geteuid has no preconditions and can't fail.
    meta.nlink() > 1 || meta.uid() != unsafe { libc::geteuid() }
}

#[cfg(not(unix))]
fn must_write_in_place(_meta: &std::fs::Metadata) -> bool {
    false
}

fn write_in_place(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = std::fs::OpenOptions::new().write(true).truncate(true).create(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn replace(path: &Path, bytes: &[u8], existing: Option<&std::fs::Metadata>) -> io::Result<()> {
    let temp = temp_beside(path);
    let result = (|| {
        let mut file = std::fs::File::create(&temp)?;
        file.write_all(bytes)?;
        if let Some(meta) = existing {
            file.set_permissions(meta.permissions())?;
        }
        file.sync_all()?;
        std::fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

fn temp_beside(path: &Path) -> PathBuf {
    let name = path.file_name().map_or_else(|| "file".into(), |n| n.to_string_lossy().into_owned());
    path.with_file_name(format!(".{name}.{}.saving", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_line_ends_and_the_bom() {
        let loaded = decode(b"\xef\xbb\xbfa\r\nb\r\n".to_vec());
        assert_eq!(
            (loaded.text.as_str(), loaded.ending, loaded.bom, loaded.lossy),
            ("a\nb\n", Ending::CrLf, true, false)
        );
        assert_eq!(encode(&loaded.text, loaded.ending, loaded.bom), b"\xef\xbb\xbfa\r\nb\r\n");
        let unix = decode(b"a\nb".to_vec());
        assert_eq!((unix.ending, unix.bom), (Ending::Lf, false));
        assert_eq!(encode("a\nb", Ending::Lf, false), b"a\nb");
    }

    #[test]
    fn says_when_it_isnt_utf8() {
        let loaded = decode(b"caf\xe9".to_vec());
        assert!(loaded.lossy);
        assert_eq!(loaded.text, "caf\u{fffd}");
    }

    #[cfg(unix)]
    #[test]
    fn saving_keeps_permissions_and_links() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("text-editor-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("run.sh");
        std::fs::write(&file, "old").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o750)).unwrap();
        write(&file, b"new").unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "new");
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o750);
        let link = dir.join("link.sh");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        write(&link, b"through the link").unwrap();
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "through the link");
        write(&dir.join("fresh.txt"), b"x").unwrap();
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 3, "no temporary files left");
        std::fs::remove_dir_all(&dir).ok();
    }
}
