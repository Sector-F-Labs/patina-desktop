//! The library: every file imported into the project, as tiles with a
//! thumbnail (or, for sound, its waveform) and its length. Tiles are dragged
//! onto the timeline's tracks, double-clicked to add them to the end, and
//! right-clicked for the rest.

use std::collections::HashMap;
use std::path::PathBuf;

use egui::{Color32, DragAndDrop, Id, Rect, Sense, Stroke, StrokeKind, pos2, vec2};

use crate::project::{MediaId, Project, Track, timecode};
use crate::timeline::Colours;

pub enum Action {
    Import,
    /// Add it at the end of a track (at the playhead, for music and voiceover).
    Add(MediaId, Track),
    Remove(MediaId),
    Reveal(PathBuf),
}

const TILE: f32 = 132.0;

pub struct Look<'a> {
    pub thumbs: &'a HashMap<MediaId, egui::TextureHandle>,
    pub peaks: &'a HashMap<MediaId, Vec<f32>>,
    pub errors: &'a HashMap<MediaId, String>,
    pub colours: Colours,
}

pub fn show(ui: &mut egui::Ui, p: &Project, look: &Look) -> Vec<Action> {
    let mut actions = Vec::new();
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.strong("Library");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Import…").on_hover_text("Add videos and sound to the library (Ctrl+I)").clicked() {
                actions.push(Action::Import);
            }
        });
    });
    ui.add_space(4.0);
    if p.media.is_empty() {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new("No clips yet").weak());
            ui.label(egui::RichText::new("Import some, or drop files here").weak().small());
        });
        return actions;
    }
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 10.0);
            for media in &p.media {
                tile(ui, p, media.id, look, &mut actions);
            }
        });
    });
    actions
}

fn tile(ui: &mut egui::Ui, p: &Project, id: MediaId, look: &Look, actions: &mut Vec<Action>) {
    let Some(media) = p.media(id) else { return };
    let ctx = ui.ctx().clone();
    let picture = vec2(TILE, TILE * 9.0 / 16.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(TILE, picture.y + 34.0), Sense::click_and_drag());
    let v = ui.visuals().clone();
    let pic = Rect::from_min_size(rect.min, picture);
    let painter = ui.painter();
    let error = look.errors.get(&id);
    let ready = media.info.is_some() && error.is_none();

    if resp.hovered() {
        painter.rect_filled(rect.expand(3.0), 6.0, v.widgets.hovered.weak_bg_fill);
    }
    draw_picture(painter, pic, id, media.has_video(), look, &v);
    if let Some(info) = &media.info {
        let badge = timecode(info.duration);
        let galley = painter.layout_no_wrap(badge, egui::FontId::proportional(10.5), v.text_color());
        let at = pic.right_bottom() - galley.size() - vec2(5.0, 4.0);
        painter.rect_filled(
            Rect::from_min_size(at - vec2(3.0, 1.0), galley.size() + vec2(6.0, 2.0)),
            3.0,
            v.extreme_bg_color.gamma_multiply(0.8),
        );
        painter.galley(at, galley, v.text_color());
    }
    let name_at = pos2(rect.left() + 1.0, pic.bottom() + 3.0);
    let name = patina::emoji::line(painter, &media.name(), egui::FontId::proportional(12.0), v.text_color(), TILE);
    name.paint(&painter.with_clip_rect(Rect::from_min_size(name_at, vec2(TILE, name.size().y + 1.0))), name_at);
    let note = match (error, &media.info) {
        (Some(e), _) => egui::RichText::new(e).color(v.error_fg_color),
        (None, None) => egui::RichText::new("Reading…").weak(),
        (None, Some(_)) => egui::RichText::new(match (media.has_video(), media.has_audio()) {
            (true, true) => "Video",
            (true, false) => "Video, no sound",
            _ => "Sound",
        })
        .weak(),
    };
    painter.text(
        pos2(rect.left() + 1.0, pic.bottom() + 19.0),
        egui::Align2::LEFT_TOP,
        note.text(),
        egui::FontId::proportional(10.5),
        if error.is_some() { v.error_fg_color } else { v.weak_text_color() },
    );
    let resp = match error {
        Some(e) => resp.on_hover_text(format!("{}\n{e}", media.path.display())),
        None => resp.on_hover_text(media.path.display().to_string()),
    };

    let main_track = if media.has_video() { Track::Video } else { Track::Music };
    if ready && resp.drag_started() {
        DragAndDrop::set_payload(&ctx, id);
    }
    if ready && resp.double_clicked() {
        actions.push(Action::Add(id, main_track));
    }
    // The tile follows the pointer while it's dragged.
    if DragAndDrop::payload::<MediaId>(&ctx).is_some_and(|m| *m == id)
        && let Some(pos) = ctx.pointer_interact_pos()
    {
        let layer = egui::LayerId::new(egui::Order::Tooltip, Id::new("dragged media"));
        let ghost = Rect::from_min_size(pos - picture * 0.25, picture * 0.5);
        let painter = ctx.layer_painter(layer);
        draw_picture(&painter, ghost, id, media.has_video(), look, &v);
        painter.rect_stroke(ghost, 4.0, Stroke::new(1.5, v.selection.bg_fill), StrokeKind::Outside);
    }
    patina::menu::context(&resp, |ui| {
        for track in Track::ALL {
            let fits = match track {
                Track::Video => media.has_video(),
                _ => media.has_audio(),
            };
            if ui.add_enabled(ready && fits, egui::Button::new(format!("Add to {}", track.label()))).clicked() {
                actions.push(Action::Add(id, track));
                ui.close();
            }
        }
        ui.separator();
        if ui.button("Show in Folder").clicked() {
            actions.push(Action::Reveal(media.path.clone()));
            ui.close();
        }
        if ui.button("Remove from Library").on_hover_text("Its clips come off the timeline too").clicked() {
            actions.push(Action::Remove(id));
            ui.close();
        }
    });
}

/// A thumbnail, or a sound's waveform on its track's colour.
fn draw_picture(painter: &egui::Painter, r: Rect, id: MediaId, video: bool, look: &Look, v: &egui::Visuals) {
    match look.thumbs.get(&id) {
        Some(thumb) if video => {
            painter.image(thumb.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        _ if video => {
            painter.rect_filled(r, 4.0, v.extreme_bg_color);
        }
        _ => {
            painter.rect_filled(r, 4.0, look.colours.music);
            if let Some(peaks) = look.peaks.get(&id).filter(|p| !p.is_empty()) {
                let columns = (r.width() / 2.0) as usize;
                let per = peaks.len().div_ceil(columns.max(1)).max(1);
                for (k, chunk) in peaks.chunks(per).enumerate() {
                    let peak = chunk.iter().copied().fold(0.0, f32::max);
                    let x = r.left() + 1.0 + k as f32 * 2.0;
                    let h = (peak.sqrt() * r.height() * 0.38).max(0.5);
                    painter.line_segment(
                        [pos2(x, r.center().y - h), pos2(x, r.center().y + h)],
                        Stroke::new(1.0, look.colours.waveform),
                    );
                }
            }
        }
    }
    painter.rect_stroke(r, 4.0, Stroke::new(1.0, v.widgets.noninteractive.bg_stroke.color), StrokeKind::Inside);
}
