//! For tests: a tiny project of files made by ffmpeg from its test sources.

use std::path::PathBuf;
use std::process::Command;

use crate::project::{Info, Project, Track};
use crate::render::ffmpeg;

/// A 3 s movie in a fresh folder named for `test`: a 2 s clip with sound,
/// a 1 s clip without, a song from 0.5 s and the first clip's sound again as
/// a voiceover from 1 s. `None` (and the test skipped) without ffmpeg.
pub fn tiny_project(test: &str) -> Option<(PathBuf, Project)> {
    if Command::new("ffmpeg").arg("-version").output().is_err() {
        eprintln!("no ffmpeg: skipped");
        return None;
    }
    let dir = std::env::temp_dir().join(format!("montage-{test}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let make = |name: &str, inputs: &[&str]| {
        let path = dir.join(name);
        let mut c = ffmpeg();
        for input in inputs {
            c.args(["-f", "lavfi", "-i", input]);
        }
        assert!(c.args(["-shortest", "-y"]).arg(&path).status().unwrap().success(), "{name}");
        path
    };
    let talking = make("talking.mp4", &["testsrc2=size=320x240:rate=25:duration=2", "sine=duration=2"]);
    let silent = make("silent.mkv", &["testsrc2=size=200x200:rate=30:duration=1"]);
    let song = make("song.flac", &["sine=frequency=220:duration=5"]);
    let mut p = Project::default();
    for (path, info) in [
        (talking, Info { duration: 2.0, size: Some([320, 240]), audio: true }),
        (silent, Info { duration: 1.0, size: Some([200, 200]), audio: false }),
        (song, Info { duration: 5.0, size: None, audio: true }),
    ] {
        let (id, _) = p.add_media(path);
        p.media_mut(id).unwrap().info = Some(info);
    }
    p.drop_media(Track::Video, 1, 0.0).unwrap();
    p.drop_media(Track::Video, 2, 9.0).unwrap();
    p.drop_media(Track::Music, 3, 0.5).unwrap();
    p.drop_media(Track::Voice, 1, 1.0).unwrap();
    Some((dir, p))
}
