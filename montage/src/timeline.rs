//! The timeline: a ruler with the playhead, and the Video, Music and
//! Voiceover tracks under it (ADR-0003). Clips are dragged to move them and
//! by their edges to trim them; media is dragged in from the library. It
//! returns `Edit`s for the app to apply, except for trims and moves of
//! placed clips, which change the project as the pointer moves, after an
//! `Edit::Snapshot` for undo.

use std::collections::HashMap;

use egui::{
    Color32, CornerRadius, CursorIcon, DragAndDrop, Id, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2, pos2, vec2,
};
use serde::Deserialize;

use crate::media::PEAKS_PER_SECOND;
use crate::project::{Clip, Edge, MediaId, Project, Sel, Track, timecode};

/// The track names' column.
pub const HEADER_WIDTH: f32 = 92.0;
const RULER: f32 = 22.0;
const GAP: f32 = 4.0;
/// Space before the movie's start, so its first clip's edge can be grabbed.
const PAD: f32 = 8.0;
/// How near an edge the pointer must be to trim, and how near a cut to snap to it.
const GRAB: f32 = 8.0;

pub const MIN_ZOOM: f32 = 2.0;
pub const MAX_ZOOM: f32 = 400.0;

fn lane_height(track: Track) -> f32 {
    match track {
        Track::Video => 64.0,
        Track::Music | Track::Voice => 44.0,
    }
}

/// How high the whole timeline is, ruler and tracks.
pub fn height() -> f32 {
    RULER + Track::ALL.iter().map(|&t| lane_height(t) + GAP).sum::<f32>()
}

/// The timeline's colours, from the skin's `[app.montage]` section.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct SkinColours {
    pub video: Option<patina::skin::Hex>,
    pub music: Option<patina::skin::Hex>,
    pub voiceover: Option<patina::skin::Hex>,
    pub waveform: Option<patina::skin::Hex>,
    pub playhead: Option<patina::skin::Hex>,
}

#[derive(Debug, Clone, Copy)]
pub struct Colours {
    pub video: Color32,
    pub music: Color32,
    pub voice: Color32,
    pub waveform: Color32,
    pub playhead: Color32,
}

impl Colours {
    /// The skin's, or else ones that suit `v`.
    pub fn new(skin: &SkinColours, v: &egui::Visuals) -> Self {
        let or = |c: Option<patina::skin::Hex>, d: Color32| c.map_or(d, |h| h.0);
        Colours {
            video: or(skin.video, v.widgets.inactive.bg_fill),
            music: or(skin.music, Color32::from_rgb(0x3D, 0x8B, 0x5F)),
            voice: or(skin.voiceover, Color32::from_rgb(0x84, 0x5E, 0xC2)),
            waveform: or(skin.waveform, Color32::from_white_alpha(170)),
            playhead: or(skin.playhead, v.error_fg_color),
        }
    }

    fn of(&self, track: Track) -> Color32 {
        match track {
            Track::Video => self.video,
            Track::Music => self.music,
            Track::Voice => self.voice,
        }
    }
}

/// What the timeline needs besides the project.
pub struct Look<'a> {
    pub thumbs: &'a HashMap<MediaId, egui::TextureHandle>,
    pub peaks: &'a HashMap<MediaId, Vec<f32>>,
    pub colours: Colours,
    pub playhead: f64,
    pub selected: Option<Sel>,
    /// Keep the playhead in view, as while playing.
    pub follow: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    Seek(f64),
    Select(Option<Sel>),
    /// The project is about to change as the pointer moves: keep it for undo.
    Snapshot,
    /// The project changed.
    Changed,
    MoveVideo {
        from: usize,
        to: usize,
    },
    Drop {
        track: Track,
        media: MediaId,
        at: f64,
    },
    Split(Sel),
    Remove(Sel),
    Volume(Sel, f32),
}

enum Drag {
    Playhead,
    Trim {
        sel: Sel,
        edge: Edge,
        was: Clip,
        was_at: f64,
        origin: f32,
    },
    Move {
        sel: Sel,
        was_at: f64,
        origin: f32,
    },
    /// A video clip being moved to another place in the sequence, on release.
    Reorder {
        index: usize,
        origin: f32,
    },
}

pub struct Timeline {
    /// Pixels a second.
    pub zoom: f32,
    drag: Option<Drag>,
    /// The scroll position to set on the next frame, and what was seen on the last.
    scroll_to: Option<f32>,
    offset: f32,
    viewport: f32,
    /// Where the timeline was last drawn, on screen.
    pub rect: Rect,
    /// A length to fit into the timeline's width on the next frame.
    fit: Option<f64>,
}

impl Default for Timeline {
    fn default() -> Self {
        Timeline {
            zoom: 24.0,
            drag: None,
            scroll_to: None,
            offset: 0.0,
            viewport: 600.0,
            rect: Rect::NOTHING,
            fit: None,
        }
    }
}

/// Where along the timeline time `t` is drawn, from its left edge.
fn x_of(zoom: f32, t: f64) -> f32 {
    PAD + t as f32 * zoom
}

fn t_of(zoom: f32, x: f32) -> f64 {
    ((x - PAD) / zoom) as f64
}

/// `t`, moved to the nearest of `marks` if one is within `within` seconds.
fn snap(t: f64, marks: &[f64], within: f64) -> f64 {
    marks
        .iter()
        .copied()
        .filter(|m| (m - t).abs() <= within)
        .min_by(|a, b| (a - t).abs().total_cmp(&(b - t).abs()))
        .unwrap_or(t)
}

/// A step between the ruler's labels that leaves them at least 70 px apart.
fn ruler_step(zoom: f32) -> f64 {
    const STEPS: [f64; 12] = [0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0, 1800.0];
    STEPS.into_iter().find(|s| *s as f32 * zoom >= 70.0).unwrap_or(3600.0)
}

/// `m:ss`, for the ruler.
fn ruler_label(t: f64) -> String {
    let code = timecode(t);
    code.strip_suffix(".0").map(str::to_string).unwrap_or(code)
}

impl Timeline {
    pub fn zoom_by(&mut self, factor: f32) {
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
    }

    /// Fits `duration` seconds into the timeline, once it's laid out.
    pub fn fit(&mut self, duration: f64) {
        if duration > 0.0 {
            self.fit = Some(duration);
        }
    }

    /// The track names, beside the timeline.
    pub fn headers(&self, ui: &mut egui::Ui, colours: &Colours) {
        let (rect, _) = ui.allocate_exact_size(vec2(HEADER_WIDTH, height()), Sense::hover());
        let v = ui.visuals();
        let mut y = rect.top() + RULER;
        for track in Track::ALL {
            let h = lane_height(track);
            let lane = Rect::from_min_size(pos2(rect.left(), y), vec2(HEADER_WIDTH, h));
            let swatch = Rect::from_min_size(pos2(lane.left() + 8.0, lane.center().y - 5.0), vec2(10.0, 10.0));
            ui.painter().rect_filled(swatch, 2.0, colours.of(track));
            ui.painter().text(
                pos2(swatch.right() + 6.0, lane.center().y),
                egui::Align2::LEFT_CENTER,
                track.label(),
                egui::TextStyle::Body.resolve(ui.style()),
                v.text_color(),
            );
            y += h + GAP;
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, p: &mut Project, look: &Look) -> Vec<Edit> {
        let mut edits = Vec::new();
        if let Some(duration) = self.fit.take() {
            self.zoom = ((ui.available_width() - PAD - 40.0) / duration as f32).clamp(MIN_ZOOM, MAX_ZOOM);
            self.scroll_to = Some(0.0);
        }
        let zoom = self.zoom;
        let mut area = egui::ScrollArea::horizontal().id_salt("timeline").auto_shrink([false, true]);
        if let Some(x) = self.scroll_to.take() {
            area = area.horizontal_scroll_offset(x.max(0.0));
        } else if look.follow {
            // Turn the page when the playhead reaches the edge, as editors do.
            let x = x_of(zoom, look.playhead);
            if x > self.offset + self.viewport - 40.0 || x < self.offset {
                area = area.horizontal_scroll_offset((x - 40.0).max(0.0));
            }
        }
        let out = area.show(ui, |ui| self.content(ui, p, look, &mut edits));
        self.offset = out.state.offset.x;
        self.viewport = out.inner_rect.width();

        // Ctrl and the wheel (or a pinch) zooms about the pointer.
        let zoom_delta = ui.input(|i| i.zoom_delta());
        if zoom_delta != 1.0
            && let Some(pointer) = ui.input(|i| i.pointer.hover_pos()).filter(|p| out.inner_rect.contains(*p))
        {
            let along = pointer.x - out.inner_rect.left();
            let t = t_of(zoom, self.offset + along);
            self.zoom_by(zoom_delta);
            self.scroll_to = Some(x_of(self.zoom, t) - along);
        }
        edits
    }

    fn content(&mut self, ui: &mut egui::Ui, p: &mut Project, look: &Look, edits: &mut Vec<Edit>) {
        let zoom = self.zoom;
        let ctx = ui.ctx().clone();
        let width = ui.available_width().max(x_of(zoom, p.extent() + 30.0));
        let (rect, bg) = ui.allocate_exact_size(vec2(width, height()), Sense::click_and_drag());
        self.rect = rect;
        let painter = ui.painter_at(rect);
        let v = ui.visuals().clone();
        let x = |t: f64| rect.left() + x_of(zoom, t);
        let t_at = |px: f32| t_of(zoom, px - rect.left()).max(0.0);
        let pointer = ctx.input(|i| i.pointer.interact_pos());
        let down = ctx.input(|i| i.pointer.primary_down());

        // Lanes.
        let mut lanes = HashMap::new();
        let mut y = rect.top() + RULER;
        for track in Track::ALL {
            let lane = Rect::from_min_size(pos2(rect.left(), y), vec2(width, lane_height(track)));
            painter.rect_filled(lane, 0.0, v.extreme_bg_color);
            lanes.insert(track, lane);
            y += lane_height(track) + GAP;
        }
        for track in Track::ALL {
            let empty = match track {
                Track::Video => p.video.is_empty(),
                _ => p.placed(track).is_empty(),
            };
            if empty {
                let hint = match track {
                    Track::Video => "Drag clips here from the library",
                    Track::Music => "Drag music here",
                    Track::Voice => "Drag a recording here, or record a voiceover",
                };
                painter.text(
                    lanes[&track].left_center() + vec2(PAD + 6.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    hint,
                    egui::FontId::proportional(12.0),
                    v.weak_text_color(),
                );
            }
        }

        // The ruler.
        let ruler = Rect::from_min_size(rect.min, vec2(width, RULER));
        painter.rect_filled(ruler, 0.0, v.faint_bg_color);
        let step = ruler_step(zoom);
        let mut t = 0.0;
        while x(t) < rect.right() {
            let tx = x(t);
            painter.line_segment(
                [pos2(tx, ruler.bottom() - 6.0), pos2(tx, ruler.bottom())],
                Stroke::new(1.0, v.weak_text_color()),
            );
            painter.text(
                pos2(tx + 3.0, ruler.top() + 2.0),
                egui::Align2::LEFT_TOP,
                ruler_label(t),
                egui::FontId::proportional(11.0),
                v.weak_text_color(),
            );
            // Small ticks between.
            for k in 1..5 {
                let sx = x(t + step * k as f64 / 5.0);
                painter.line_segment(
                    [pos2(sx, ruler.bottom() - 3.0), pos2(sx, ruler.bottom())],
                    Stroke::new(1.0, v.weak_text_color()),
                );
            }
            t += step;
        }
        // Where the movie ends.
        let end_x = x(p.duration());
        painter.line_segment(
            [pos2(end_x, ruler.bottom()), pos2(end_x, rect.bottom())],
            Stroke::new(1.0, v.weak_text_color().gamma_multiply(0.5)),
        );

        // Where clips snap to: the cuts, the movie's start and the playhead.
        let mut marks: Vec<f64> = p.video_starts();
        marks.push(p.duration());
        marks.push(look.playhead);
        let within = (GRAB / zoom) as f64;

        // The video track's clips.
        let starts = p.video_starts();
        let lane = lanes[&Track::Video];
        for (i, clip) in p.video.iter().enumerate() {
            let sel = Sel { track: Track::Video, index: i };
            let r = Rect::from_x_y_ranges(x(starts[i])..=x(starts[i] + clip.length), lane.y_range())
                .shrink2(vec2(0.5, 2.0));
            let reordering = matches!(self.drag, Some(Drag::Reorder { index, .. }) if index == i);
            paint_clip(&painter, r, p, clip, look, sel, &v, zoom, reordering);
            self.interact_clip(ui, r, sel, clip, 0.0, pointer, edits);
        }
        // Music and voiceover.
        for track in [Track::Music, Track::Voice] {
            let lane = lanes[&track];
            for (i, placed) in p.placed(track).iter().enumerate() {
                let sel = Sel { track, index: i };
                let r = Rect::from_x_y_ranges(x(placed.at)..=x(placed.end()), lane.y_range()).shrink2(vec2(0.5, 2.0));
                paint_clip(&painter, r, p, &placed.clip, look, sel, &v, zoom, false);
                self.interact_clip(ui, r, sel, &placed.clip, placed.at, pointer, edits);
            }
        }

        // The ruler and empty track space move the playhead.
        if (bg.drag_started() || bg.clicked())
            && let Some(pos) = pointer
        {
            edits.push(Edit::Seek(t_at(pos.x)));
            if bg.drag_started() {
                self.drag = Some(Drag::Playhead);
            }
            if pos.y > ruler.bottom() {
                edits.push(Edit::Select(None));
            }
        }

        // A drag under way.
        if let (Some(drag), Some(pos)) = (&self.drag, pointer) {
            let by = |origin: f32| ((pos.x - origin) / zoom) as f64;
            match drag {
                Drag::Playhead => edits.push(Edit::Seek(t_at(pos.x).min(p.extent()))),
                Drag::Trim { sel, edge, was, was_at, origin } => {
                    // The edge being moved snaps to the cuts and the playhead.
                    let start =
                        if sel.track == Track::Video { starts.get(sel.index).copied().unwrap_or(0.0) } else { *was_at };
                    let at_edge = match edge {
                        Edge::Start => start,
                        Edge::End => start + was.length,
                    };
                    let mut by = by(*origin);
                    if sel.track != Track::Video {
                        by = snap(at_edge + by, &marks, within) - at_edge;
                    }
                    p.trim(*sel, was, *was_at, *edge, by);
                    edits.push(Edit::Changed);
                }
                Drag::Move { sel, was_at, origin } => {
                    let length = p.clip(*sel).map_or(0.0, |c| c.length);
                    let mut at = was_at + by(*origin);
                    let start = snap(at, &marks, within);
                    let end = snap(at + length, &marks, within);
                    if start != at {
                        at = start;
                    } else if end != at + length {
                        at = end - length;
                    }
                    p.move_placed(sel.track, sel.index, at);
                    edits.push(Edit::Changed);
                }
                Drag::Reorder { index, origin } => {
                    if let Some(clip) = p.video.get(*index) {
                        let ghost = Rect::from_x_y_ranges(
                            x(starts[*index]) + pos.x - origin..=x(starts[*index] + clip.length) + pos.x - origin,
                            lane.y_range(),
                        )
                        .shrink(3.0);
                        painter.rect_stroke(ghost, 4.0, Stroke::new(2.0, v.selection.bg_fill), StrokeKind::Inside);
                        let to = p.insertion_index(t_at(pos.x));
                        insertion_mark(&painter, lane, x(starts.get(to).copied().unwrap_or(p.duration())), &v);
                    }
                    ctx.set_cursor_icon(CursorIcon::Grabbing);
                }
            }
            if !down {
                if let Drag::Reorder { index, origin } = drag
                    && (pos.x - origin).abs() > 4.0
                {
                    let to = p.insertion_index(t_at(pos.x));
                    edits.push(Edit::MoveVideo { from: *index, to });
                }
                self.drag = None;
            }
        } else if !down {
            self.drag = None;
        }

        // Media dragged in from the library.
        if let (Some(media), Some(pos)) = (DragAndDrop::payload::<MediaId>(&ctx), pointer)
            && let Some((&track, &lane)) = lanes.iter().find(|(_, l)| l.contains(pos))
        {
            let at = match track {
                Track::Video => p.video_starts().get(p.insertion_index(t_at(pos.x))).copied().unwrap_or(p.duration()),
                _ => snap(t_at(pos.x), &marks, within),
            };
            painter.rect_stroke(lane.shrink(1.0), 2.0, Stroke::new(1.5, v.selection.bg_fill), StrokeKind::Inside);
            let length = p.media(*media).map_or(0.0, |m| m.duration());
            match track {
                Track::Video => insertion_mark(&painter, lane, x(at), &v),
                _ => {
                    let ghost = Rect::from_x_y_ranges(x(at)..=x(at + length), lane.y_range()).shrink2(vec2(0.5, 2.0));
                    painter.rect_filled(ghost, 4.0, look.colours.of(track).gamma_multiply(0.5));
                }
            }
            if ctx.input(|i| i.pointer.any_released())
                && let Some(media) = DragAndDrop::take_payload::<MediaId>(&ctx)
            {
                edits.push(Edit::Drop { track, media: *media, at });
            }
        }

        // The playhead, over everything.
        let px = x(look.playhead);
        painter.line_segment([pos2(px, rect.top()), pos2(px, rect.bottom())], Stroke::new(2.0, look.colours.playhead));
        painter.add(egui::Shape::convex_polygon(
            vec![pos2(px - 6.0, rect.top()), pos2(px + 6.0, rect.top()), pos2(px, rect.top() + 8.0)],
            look.colours.playhead,
            Stroke::NONE,
        ));
    }

    /// A clip's body selects it and drags it; its edges trim it.
    #[allow(clippy::too_many_arguments)]
    fn interact_clip(
        &mut self,
        ui: &mut egui::Ui,
        r: Rect,
        sel: Sel,
        clip: &Clip,
        at: f64,
        pointer: Option<Pos2>,
        edits: &mut Vec<Edit>,
    ) {
        let id = Id::new(("clip", sel.track, sel.index));
        let body = ui.interact(r, id, Sense::click_and_drag());
        let grab = GRAB.min(r.width() / 3.0);
        let start_edge = ui.interact(
            Rect::from_x_y_ranges(r.left()..=r.left() + grab, r.y_range()),
            id.with("start"),
            Sense::drag(),
        );
        let end_edge = ui.interact(
            Rect::from_x_y_ranges(r.right() - grab..=r.right(), r.y_range()),
            id.with("end"),
            Sense::drag(),
        );
        for edge in [&start_edge, &end_edge] {
            if edge.hovered() || edge.dragged() {
                ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
            }
        }
        // Where the button went down: the drag starts a few pixels later.
        let origin = ui.input(|i| i.pointer.press_origin()).or(pointer).map_or(r.left(), |p| p.x);
        for (resp, edge) in [(&start_edge, Edge::Start), (&end_edge, Edge::End)] {
            if resp.drag_started() {
                edits.push(Edit::Select(Some(sel)));
                edits.push(Edit::Snapshot);
                self.drag = Some(Drag::Trim { sel, edge, was: clip.clone(), was_at: at, origin });
            }
        }
        if body.clicked() {
            edits.push(Edit::Select(Some(sel)));
        }
        if body.drag_started() {
            edits.push(Edit::Select(Some(sel)));
            self.drag = Some(match sel.track {
                Track::Video => Drag::Reorder { index: sel.index, origin },
                _ => {
                    edits.push(Edit::Snapshot);
                    Drag::Move { sel, was_at: at, origin }
                }
            });
        }
        if body.hovered() && !body.dragged() && self.drag.is_none() {
            ui.ctx().set_cursor_icon(CursorIcon::Grab);
        }
        if body.secondary_clicked() {
            edits.push(Edit::Select(Some(sel)));
        }
        patina::menu::context(&body, |ui| {
            if ui.button("Split at Playhead").clicked() {
                edits.push(Edit::Split(sel));
                ui.close();
            }
            if ui.button("Delete").clicked() {
                edits.push(Edit::Remove(sel));
                ui.close();
            }
            ui.separator();
            for (label, volume) in [("Mute", 0.0), ("Quieter", 0.5), ("Normal Volume", 1.0), ("Louder", 1.5)] {
                if ui.radio((clip.volume - volume).abs() < 0.01, label).clicked() {
                    edits.push(Edit::Volume(sel, volume));
                    ui.close();
                }
            }
        });
    }
}

fn insertion_mark(painter: &egui::Painter, lane: Rect, x: f32, v: &egui::Visuals) {
    painter.line_segment([pos2(x, lane.top()), pos2(x, lane.bottom())], Stroke::new(3.0, v.selection.bg_fill));
}

/// A clip: its thumbnails along it for video, its waveform for sound, and its name.
#[allow(clippy::too_many_arguments)]
fn paint_clip(
    painter: &egui::Painter,
    r: Rect,
    p: &Project,
    clip: &Clip,
    look: &Look,
    sel: Sel,
    v: &egui::Visuals,
    zoom: f32,
    faded: bool,
) {
    let track = sel.track;
    let radius = CornerRadius::same(4);
    let painter = painter.with_clip_rect(painter.clip_rect().intersect(r));
    painter.rect_filled(r, radius, look.colours.of(track));
    match (track, look.thumbs.get(&clip.media)) {
        (Track::Video, Some(thumb)) => {
            let tile = Vec2::new(r.height() * 16.0 / 9.0, r.height());
            let mut tx = r.left();
            while tx < r.right() {
                let tint = if faded { Color32::from_white_alpha(90) } else { Color32::WHITE };
                painter.image(
                    thumb.id(),
                    Rect::from_min_size(pos2(tx, r.top()), tile),
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    tint,
                );
                tx += tile.x;
            }
        }
        (Track::Music | Track::Voice, _) => {
            if let Some(peaks) = look.peaks.get(&clip.media) {
                let mid = r.center().y + 6.0;
                let half = (r.height() - 18.0) / 2.0;
                let mut px = r.left().max(painter.clip_rect().left());
                let right = r.right().min(painter.clip_rect().right());
                while px < right {
                    let t = clip.from + ((px - r.left()) / zoom) as f64;
                    let k = (t * PEAKS_PER_SECOND as f64) as usize;
                    let peak = (peaks.get(k).copied().unwrap_or(0.0) * clip.volume).min(1.0);
                    let h = (peak.sqrt() * half).max(0.5);
                    painter
                        .line_segment([pos2(px, mid - h), pos2(px, mid + h)], Stroke::new(1.0, look.colours.waveform));
                    px += 2.0;
                }
            }
        }
        _ => {}
    }
    let name = p.media(clip.media).map(|m| m.name()).unwrap_or_default();
    let label = if clip.volume == 0.0 {
        format!("{name} · muted")
    } else if (clip.volume - 1.0).abs() > 0.01 {
        format!("{name} · {:.0}%", clip.volume * 100.0)
    } else {
        name
    };
    let strip = Rect::from_min_size(r.min, vec2(r.width(), 16.0));
    painter.rect_filled(
        strip,
        CornerRadius { nw: 4, ne: 4, ..Default::default() },
        v.extreme_bg_color.gamma_multiply(0.75),
    );
    painter.text(
        strip.left_center() + vec2(5.0, 0.0),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(11.0),
        v.text_color(),
    );
    if look.selected == Some(sel) {
        painter.rect_stroke(r, radius, Stroke::new(2.5, v.selection.bg_fill), StrokeKind::Inside);
    } else {
        painter.rect_stroke(r, radius, Stroke::new(1.0, v.widgets.noninteractive.bg_stroke.color), StrokeKind::Inside);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapping_takes_the_nearest_mark_in_reach() {
        assert_eq!(snap(4.85, &[0.0, 5.0, 4.8], 0.3), 4.8);
        assert_eq!(snap(4.0, &[0.0, 5.0], 0.3), 4.0);
    }

    #[test]
    fn ruler_labels_stay_apart() {
        assert_eq!(ruler_step(100.0), 1.0);
        assert_eq!(ruler_step(24.0), 5.0);
        assert_eq!(ruler_step(2.0), 60.0);
        assert_eq!(ruler_label(65.0), "1:05");
    }

    #[test]
    fn time_and_place_agree() {
        assert_eq!(t_of(24.0, x_of(24.0, 12.5)), 12.5);
    }

    use crate::project::{Info, Placed};
    use egui::{Event, PointerButton, RawInput};

    /// A timeline in a window of its own, driven by pointer events.
    struct Harness {
        ctx: egui::Context,
        timeline: Timeline,
        project: Project,
        thumbs: HashMap<MediaId, egui::TextureHandle>,
        peaks: HashMap<MediaId, Vec<f32>>,
    }

    impl Harness {
        fn new() -> Self {
            let mut project = Project::default();
            for (path, info) in [
                ("/a.mp4", Info { duration: 10.0, size: Some([1280, 720]), audio: true }),
                ("/song.mp3", Info { duration: 60.0, size: None, audio: true }),
            ] {
                let (id, _) = project.add_media(path.into());
                project.media_mut(id).unwrap().info = Some(info);
            }
            project.video.push(Clip { media: 1, from: 0.0, length: 4.0, volume: 1.0 });
            project.video.push(Clip { media: 1, from: 5.0, length: 3.0, volume: 1.0 });
            project.music.push(Placed { at: 1.0, clip: Clip { media: 2, from: 0.0, length: 5.0, volume: 1.0 } });
            let mut h = Harness {
                ctx: egui::Context::default(),
                timeline: Timeline::default(),
                project,
                thumbs: HashMap::new(),
                peaks: HashMap::new(),
            };
            // Zoomed so a second is 50 px; one frame to lay it out.
            h.timeline.zoom = 50.0;
            h.frame(vec![]);
            h
        }

        fn frame(&mut self, events: Vec<Event>) -> Vec<Edit> {
            let raw = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 400.0))),
                events,
                ..Default::default()
            };
            let mut edits = Vec::new();
            let look = Look {
                thumbs: &self.thumbs,
                peaks: &self.peaks,
                colours: Colours::new(&SkinColours::default(), &egui::Visuals::dark()),
                playhead: 0.0,
                selected: None,
                follow: false,
            };
            let (timeline, project) = (&mut self.timeline, &mut self.project);
            let _ = self.ctx.run_ui(raw, |ui| edits = timeline.show(ui, project, &look));
            edits
        }

        /// Where time `t` is on `track`'s lane, on screen.
        fn at(&self, track: Track, t: f64) -> Pos2 {
            let mut y = self.timeline.rect.top() + RULER;
            for tr in Track::ALL {
                if tr == track {
                    break;
                }
                y += lane_height(tr) + GAP;
            }
            pos2(self.timeline.rect.left() + x_of(self.timeline.zoom, t), y + lane_height(track) / 2.0)
        }

        /// Presses at `from`, drags to `to` in steps, and lets go; every frame's edits.
        fn drag(&mut self, from: Pos2, to: Pos2) -> Vec<Edit> {
            let button = |pos, pressed| Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            };
            let mut edits = self.frame(vec![Event::PointerMoved(from)]);
            edits.extend(self.frame(vec![button(from, true)]));
            for k in 1..=4 {
                edits.extend(self.frame(vec![Event::PointerMoved(from + (to - from) * k as f32 / 4.0)]));
            }
            edits.extend(self.frame(vec![button(to, false)]));
            edits.extend(self.frame(vec![]));
            edits
        }
    }

    #[test]
    fn dragging_music_moves_it_by_as_far_as_the_pointer_went() {
        let mut h = Harness::new();
        let edits = h.drag(h.at(Track::Music, 2.0), h.at(Track::Music, 4.5));
        assert!(edits.contains(&Edit::Snapshot), "kept for undo first: {edits:?}");
        assert!((h.project.music[0].at - 3.5).abs() < 0.03, "at {}", h.project.music[0].at);
    }

    #[test]
    fn dragging_a_video_clip_s_end_trims_it() {
        let mut h = Harness::new();
        let edge = h.at(Track::Video, 4.0) - vec2(2.0, 0.0);
        h.drag(edge, edge - vec2(50.0, 0.0));
        assert!((h.project.video[0].length - 3.0).abs() < 0.03, "{}", h.project.video[0].length);
        assert_eq!(h.project.video[0].from, 0.0);
    }

    #[test]
    fn dragging_a_video_clip_past_another_reorders_them() {
        let mut h = Harness::new();
        let edits = h.drag(h.at(Track::Video, 1.0), h.at(Track::Video, 6.9));
        assert!(edits.contains(&Edit::MoveVideo { from: 0, to: 2 }), "{edits:?}");
        assert_eq!(h.project.video[0].from, 0.0, "the app moves it, not the timeline");
    }

    #[test]
    fn media_dropped_on_the_voiceover_track_lands_where_it_was_let_go() {
        let mut h = Harness::new();
        let to = h.at(Track::Voice, 3.0);
        DragAndDrop::set_payload::<MediaId>(&h.ctx, 2);
        h.frame(vec![Event::PointerMoved(to)]);
        let edits = h.frame(vec![Event::PointerButton {
            pos: to,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }]);
        let dropped = edits.iter().find_map(|e| match e {
            Edit::Drop { track, media, at } => Some((*track, *media, *at)),
            _ => None,
        });
        let (track, media, at) = dropped.expect("a drop");
        assert_eq!((track, media), (Track::Voice, 2));
        assert!((at - 3.0).abs() < 0.03, "{at}");
    }

    #[test]
    fn clicking_the_ruler_moves_the_playhead() {
        let mut h = Harness::new();
        let pos = pos2(h.at(Track::Video, 5.0).x, h.timeline.rect.top() + 5.0);
        let press = |pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        h.frame(vec![Event::PointerMoved(pos)]);
        h.frame(vec![press(true)]);
        let edits = h.frame(vec![press(false)]);
        assert!(edits.iter().any(|e| matches!(e, Edit::Seek(t) if (t - 5.0).abs() < 0.03)), "{edits:?}");
    }
}
