//! The project: the library of media and the timeline's three tracks, saved
//! as a `.montage` file (JSON, ADR-0004). The video track is a sequence that
//! plays clip after clip; music and voiceover clips sit at times of their own
//! under it (ADR-0003). Everything here is plain data and pure edits, so it's
//! tested without a window or ffmpeg.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The file format's version, written into every project.
pub const FORMAT: u32 = 1;

/// No clip gets shorter than this by trimming or splitting, in seconds.
pub const MIN_LENGTH: f64 = 0.2;

pub type MediaId = u64;

/// What ffprobe found in a file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Info {
    /// In seconds.
    pub duration: f64,
    /// The picture's size, for a file with video.
    pub size: Option<[u32; 2]>,
    pub audio: bool,
}

/// A file in the library.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Media {
    pub id: MediaId,
    pub path: PathBuf,
    /// Saved, so an opened project lays out before its files are probed again.
    #[serde(default)]
    pub info: Option<Info>,
}

impl Media {
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }

    pub fn has_video(&self) -> bool {
        self.info.as_ref().is_some_and(|i| i.size.is_some())
    }

    pub fn has_audio(&self) -> bool {
        self.info.as_ref().is_some_and(|i| i.audio)
    }

    pub fn duration(&self) -> f64 {
        self.info.as_ref().map_or(0.0, |i| i.duration)
    }
}

/// A stretch of one file: `length` seconds from `from` in it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub media: MediaId,
    pub from: f64,
    pub length: f64,
    /// 1 leaves the sound as it is, 0 silences it.
    #[serde(default = "one")]
    pub volume: f32,
}

fn one() -> f32 {
    1.0
}

impl Clip {
    /// All of a file.
    pub fn whole(media: &Media) -> Self {
        Clip { media: media.id, from: 0.0, length: media.duration(), volume: 1.0 }
    }
}

/// A clip on the music or voiceover track, starting `at` seconds into the movie.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Placed {
    pub at: f64,
    pub clip: Clip,
}

impl Placed {
    pub fn end(&self) -> f64 {
        self.at + self.clip.length
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Track {
    Video,
    Music,
    Voice,
}

impl Track {
    pub const ALL: [Track; 3] = [Track::Video, Track::Music, Track::Voice];

    pub fn label(self) -> &'static str {
        match self {
            Track::Video => "Video",
            Track::Music => "Music",
            Track::Voice => "Voiceover",
        }
    }
}

/// A clip on the timeline: its track and its place in that track's list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sel {
    pub track: Track,
    pub index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Start,
    End,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub format: u32,
    pub media: Vec<Media>,
    pub next_id: MediaId,
    pub video: Vec<Clip>,
    pub music: Vec<Placed>,
    pub voice: Vec<Placed>,
}

impl Default for Project {
    fn default() -> Self {
        Project {
            format: FORMAT,
            media: Vec::new(),
            next_id: 1,
            video: Vec::new(),
            music: Vec::new(),
            voice: Vec::new(),
        }
    }
}

impl Project {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let project: Project = serde_json::from_str(&text).map_err(|e| format!("not a Montage project: {e}"))?;
        if project.format > FORMAT {
            return Err(format!("made by a newer Montage (format {})", project.format));
        }
        Ok(project)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        // Through a temporary file, so a failed write leaves the old project whole.
        let tmp = path.with_extension("montage.tmp");
        std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }

    pub fn media(&self, id: MediaId) -> Option<&Media> {
        self.media.iter().find(|m| m.id == id)
    }

    pub fn media_mut(&mut self, id: MediaId) -> Option<&mut Media> {
        self.media.iter_mut().find(|m| m.id == id)
    }

    /// Adds a file to the library, or finds it there already.
    pub fn add_media(&mut self, path: PathBuf) -> (MediaId, bool) {
        if let Some(m) = self.media.iter().find(|m| m.path == path) {
            return (m.id, false);
        }
        let id = self.next_id;
        self.next_id += 1;
        self.media.push(Media { id, path, info: None });
        (id, true)
    }

    /// Takes a file out of the library, and every clip of it off the timeline.
    pub fn remove_media(&mut self, id: MediaId) {
        self.media.retain(|m| m.id != id);
        self.video.retain(|c| c.media != id);
        self.music.retain(|p| p.clip.media != id);
        self.voice.retain(|p| p.clip.media != id);
    }

    /// The music or voiceover track's clips.
    pub fn placed(&self, track: Track) -> &[Placed] {
        match track {
            Track::Music => &self.music,
            Track::Voice => &self.voice,
            Track::Video => &[],
        }
    }

    /// The music or voiceover track's clips. Only those two are asked for;
    /// Video, which has no placed clips, gets the voiceover's rather than a panic.
    pub fn placed_mut(&mut self, track: Track) -> &mut Vec<Placed> {
        match track {
            Track::Music => &mut self.music,
            Track::Voice | Track::Video => &mut self.voice,
        }
    }

    /// How long the movie is: its video track. Music past the end is cut off.
    pub fn duration(&self) -> f64 {
        self.video.iter().map(|c| c.length).sum()
    }

    /// Where the timeline's last clip, on any track, ends.
    pub fn extent(&self) -> f64 {
        self.music.iter().chain(&self.voice).map(Placed::end).fold(self.duration(), f64::max)
    }

    /// Where each video clip starts in the movie.
    pub fn video_starts(&self) -> Vec<f64> {
        let mut at = 0.0;
        self.video
            .iter()
            .map(|c| {
                let start = at;
                at += c.length;
                start
            })
            .collect()
    }

    /// The video clip playing at `t`, and how far into it `t` is.
    pub fn video_at(&self, t: f64) -> Option<(usize, f64)> {
        let mut at = 0.0;
        for (i, c) in self.video.iter().enumerate() {
            if t < at + c.length {
                return Some((i, (t - at).max(0.0)));
            }
            at += c.length;
        }
        None
    }

    /// Where a video clip dropped at `t` goes in the sequence: before the
    /// first clip whose middle is after `t`.
    pub fn insertion_index(&self, t: f64) -> usize {
        let mut at = 0.0;
        for (i, c) in self.video.iter().enumerate() {
            if t < at + c.length / 2.0 {
                return i;
            }
            at += c.length;
        }
        self.video.len()
    }

    /// Moves the video clip at `from` to sit before the clip now at `to`.
    /// Returns its new index.
    pub fn move_video(&mut self, from: usize, to: usize) -> usize {
        let clip = self.video.remove(from);
        let to = if to > from { to - 1 } else { to }.min(self.video.len());
        self.video.insert(to, clip);
        to
    }

    /// Splits the video clip at `t` into two, if `t` isn't at either end of it.
    pub fn split_video(&mut self, t: f64) -> Option<usize> {
        let (i, offset) = self.video_at(t)?;
        let clip = &self.video[i];
        if offset < MIN_LENGTH || clip.length - offset < MIN_LENGTH {
            return None;
        }
        let mut second = clip.clone();
        second.from += offset;
        second.length -= offset;
        self.video[i].length = offset;
        self.video.insert(i + 1, second);
        Some(i + 1)
    }

    /// Splits a music or voiceover clip at movie time `t`.
    pub fn split_placed(&mut self, track: Track, index: usize, t: f64) -> Option<usize> {
        let list = self.placed_mut(track);
        let p = list.get(index)?;
        let offset = t - p.at;
        if offset < MIN_LENGTH || p.clip.length - offset < MIN_LENGTH {
            return None;
        }
        let mut second = p.clone();
        second.at = t;
        second.clip.from += offset;
        second.clip.length -= offset;
        list[index].clip.length = offset;
        list.insert(index + 1, second);
        Some(index + 1)
    }

    pub fn remove(&mut self, sel: Sel) {
        match sel.track {
            Track::Video if sel.index < self.video.len() => {
                self.video.remove(sel.index);
            }
            Track::Music | Track::Voice if sel.index < self.placed(sel.track).len() => {
                self.placed_mut(sel.track).remove(sel.index);
            }
            _ => {}
        }
    }

    pub fn clip(&self, sel: Sel) -> Option<&Clip> {
        match sel.track {
            Track::Video => self.video.get(sel.index),
            _ => self.placed(sel.track).get(sel.index).map(|p| &p.clip),
        }
    }

    pub fn clip_mut(&mut self, sel: Sel) -> Option<&mut Clip> {
        match sel.track {
            Track::Video => self.video.get_mut(sel.index),
            _ => self.placed_mut(sel.track).get_mut(sel.index).map(|p| &mut p.clip),
        }
    }

    /// Trims a clip, from how it was before the drag began (`was`, and
    /// `was_at` for a placed clip), by `by` seconds at one edge. Keeps the
    /// clip inside its file and at least `MIN_LENGTH` long. A placed clip's
    /// start moves with its start edge, so the sound stays where it was.
    pub fn trim(&mut self, sel: Sel, was: &Clip, was_at: f64, edge: Edge, by: f64) {
        let duration = self.media(was.media).map_or(was.from + was.length, Media::duration);
        let mut clip = was.clone();
        match edge {
            Edge::Start => {
                // How far the start can move: back to the file's start (and,
                // for a placed clip, the movie's), on to MIN_LENGTH from the end.
                let mut earliest = -was.from;
                if sel.track != Track::Video {
                    earliest = earliest.max(-was_at);
                }
                let by = by.clamp(earliest, was.length - MIN_LENGTH);
                clip.from += by;
                clip.length -= by;
                if sel.track != Track::Video
                    && let Some(p) = self.placed_mut(sel.track).get_mut(sel.index)
                {
                    p.at = was_at + by;
                }
            }
            Edge::End => {
                clip.length = (was.length + by).clamp(MIN_LENGTH, (duration - was.from).max(MIN_LENGTH));
            }
        }
        if let Some(c) = self.clip_mut(sel) {
            *c = clip;
        }
    }

    /// Moves a music or voiceover clip to start at `at`, no earlier than the movie's start.
    pub fn move_placed(&mut self, track: Track, index: usize, at: f64) {
        if let Some(p) = self.placed_mut(track).get_mut(index) {
            p.at = at.max(0.0);
        }
    }

    /// Puts a clip of `media` on a track: into the video sequence where `at`
    /// falls, or at `at` on the music or voiceover track. Returns where it went.
    pub fn drop_media(&mut self, track: Track, media: MediaId, at: f64) -> Result<Sel, String> {
        let m = self.media(media).ok_or("That file isn't in the library")?;
        if m.info.is_none() {
            return Err(format!("{} is still being read", m.name()));
        }
        let clip = Clip::whole(m);
        match track {
            Track::Video => {
                if !m.has_video() {
                    return Err(format!("{} has no picture: put it on Music or Voiceover", m.name()));
                }
                let index = self.insertion_index(at);
                self.video.insert(index, clip);
                Ok(Sel { track, index })
            }
            Track::Music | Track::Voice => {
                if !m.has_audio() {
                    return Err(format!("{} has no sound", m.name()));
                }
                let list = self.placed_mut(track);
                list.push(Placed { at: at.max(0.0), clip });
                Ok(Sel { track, index: list.len() - 1 })
            }
        }
    }
}

/// `m:ss.d`, or `h:mm:ss.d` from an hour.
pub fn timecode(t: f64) -> String {
    let tenths = (t.max(0.0) * 10.0).round() as u64;
    let (h, m, s, d) = (tenths / 36000, tenths / 600 % 60, tenths / 10 % 60, tenths % 10);
    if h > 0 { format!("{h}:{m:02}:{s:02}.{d}") } else { format!("{m}:{s:02}.{d}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(info: Info) -> (Project, MediaId) {
        let mut p = Project::default();
        let (id, _) = p.add_media(PathBuf::from("/films/a.mp4"));
        p.media_mut(id).unwrap().info = Some(info);
        (p, id)
    }

    fn film(duration: f64) -> Info {
        Info { duration, size: Some([1920, 1080]), audio: true }
    }

    fn lengths(p: &Project) -> Vec<f64> {
        p.video.iter().map(|c| c.length).collect()
    }

    #[test]
    fn media_is_added_once() {
        let mut p = Project::default();
        let (a, new) = p.add_media("/x.mp4".into());
        assert!(new);
        let (b, new) = p.add_media("/x.mp4".into());
        assert_eq!((a, new), (b, false));
        assert_ne!(p.add_media("/y.mp4".into()).0, a);
    }

    #[test]
    fn video_drops_go_between_clips() {
        let (mut p, id) = with(film(10.0));
        p.drop_media(Track::Video, id, 0.0).unwrap();
        p.video[0].length = 4.0;
        p.drop_media(Track::Video, id, 99.0).unwrap();
        p.video[1].length = 6.0;
        // Before the middle of the first clip, so first; after it, second.
        assert_eq!(p.insertion_index(1.9), 0);
        assert_eq!(p.insertion_index(2.1), 1);
        assert_eq!(p.insertion_index(7.1), 2);
        assert_eq!(p.duration(), 10.0);
        assert_eq!(p.video_starts(), vec![0.0, 4.0]);
        assert_eq!(p.video_at(5.0), Some((1, 1.0)));
        assert_eq!(p.video_at(10.0), None);
    }

    #[test]
    fn tracks_take_only_what_suits_them() {
        let (mut p, id) = with(Info { duration: 30.0, size: None, audio: true });
        assert!(p.drop_media(Track::Video, id, 0.0).is_err());
        assert_eq!(p.drop_media(Track::Music, id, -3.0).unwrap(), Sel { track: Track::Music, index: 0 });
        assert_eq!(p.music[0].at, 0.0);
        let (mut p, id) = with(Info { duration: 30.0, size: Some([640, 480]), audio: false });
        assert!(p.drop_media(Track::Voice, id, 0.0).is_err());
        p.media_mut(id).unwrap().info = None;
        assert!(p.drop_media(Track::Video, id, 0.0).is_err(), "not probed yet");
    }

    #[test]
    fn moving_a_video_clip() {
        let (mut p, id) = with(film(10.0));
        for len in [1.0, 2.0, 3.0] {
            p.video.push(Clip { media: id, from: 0.0, length: len, volume: 1.0 });
        }
        assert_eq!(p.move_video(0, 3), 2);
        assert_eq!(lengths(&p), [2.0, 3.0, 1.0]);
        assert_eq!(p.move_video(2, 0), 0);
        assert_eq!(lengths(&p), [1.0, 2.0, 3.0]);
        assert_eq!(p.move_video(1, 2), 1, "dropped just after itself, it stays");
        assert_eq!(lengths(&p), [1.0, 2.0, 3.0]);
    }

    #[test]
    fn splitting() {
        let (mut p, id) = with(film(10.0));
        p.drop_media(Track::Video, id, 0.0).unwrap();
        assert_eq!(p.split_video(4.0), Some(1));
        assert_eq!(lengths(&p), [4.0, 6.0]);
        assert_eq!(p.video[1].from, 4.0);
        assert_eq!(p.split_video(4.05), None, "too close to a cut");
        p.drop_media(Track::Music, id, 2.0).unwrap();
        assert_eq!(p.split_placed(Track::Music, 0, 5.0), Some(1));
        assert_eq!((p.music[0].clip.length, p.music[1].at, p.music[1].clip.from), (3.0, 5.0, 3.0));
    }

    #[test]
    fn trimming_stays_inside_the_file() {
        let (mut p, id) = with(film(10.0));
        p.video.push(Clip { media: id, from: 2.0, length: 5.0, volume: 1.0 });
        let sel = Sel { track: Track::Video, index: 0 };
        let was = p.video[0].clone();
        p.trim(sel, &was, 0.0, Edge::Start, -5.0);
        assert_eq!((p.video[0].from, p.video[0].length), (0.0, 7.0));
        p.trim(sel, &was, 0.0, Edge::Start, 9.0);
        assert!((p.video[0].from - 6.8).abs() < 1e-9 && (p.video[0].length - MIN_LENGTH).abs() < 1e-9);
        p.trim(sel, &was, 0.0, Edge::End, 100.0);
        assert_eq!((p.video[0].from, p.video[0].length), (2.0, 8.0));
        p.trim(sel, &was, 0.0, Edge::End, -100.0);
        assert_eq!(p.video[0].length, MIN_LENGTH);
    }

    #[test]
    fn trimming_a_placed_clip_keeps_its_sound_in_place() {
        let (mut p, id) = with(film(10.0));
        p.music.push(Placed { at: 1.0, clip: Clip { media: id, from: 3.0, length: 4.0, volume: 1.0 } });
        let sel = Sel { track: Track::Music, index: 0 };
        let was = p.music[0].clip.clone();
        p.trim(sel, &was, 1.0, Edge::Start, 2.0);
        assert_eq!((p.music[0].at, p.music[0].clip.from, p.music[0].clip.length), (3.0, 5.0, 2.0));
        // Back past the movie's start: stops at it.
        p.trim(sel, &was, 1.0, Edge::Start, -3.0);
        assert_eq!((p.music[0].at, p.music[0].clip.from, p.music[0].clip.length), (0.0, 2.0, 5.0));
    }

    #[test]
    fn removing_media_takes_its_clips() {
        let (mut p, id) = with(film(10.0));
        p.drop_media(Track::Video, id, 0.0).unwrap();
        p.drop_media(Track::Voice, id, 0.0).unwrap();
        p.remove_media(id);
        assert!(p.media.is_empty() && p.video.is_empty() && p.voice.is_empty());
    }

    #[test]
    fn the_extent_covers_every_track() {
        let (mut p, id) = with(film(10.0));
        p.drop_media(Track::Video, id, 0.0).unwrap();
        p.drop_media(Track::Voice, id, 5.0).unwrap();
        assert_eq!((p.duration(), p.extent()), (10.0, 15.0));
    }

    #[test]
    fn a_project_survives_saving() {
        let (mut p, id) = with(film(10.0));
        p.drop_media(Track::Video, id, 0.0).unwrap();
        p.drop_media(Track::Music, id, 1.5).unwrap();
        let dir = std::env::temp_dir().join(format!("montage-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("p.montage");
        p.save(&path).unwrap();
        assert_eq!(Project::load(&path).unwrap(), p);
        std::fs::write(&path, r#"{"format": 99, "media": [], "next_id": 1, "video": [], "music": [], "voice": []}"#)
            .unwrap();
        assert!(Project::load(&path).unwrap_err().contains("newer"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn timecodes() {
        assert_eq!(timecode(0.0), "0:00.0");
        assert_eq!(timecode(83.46), "1:23.5");
        assert_eq!(timecode(3723.0), "1:02:03.0");
    }
}
