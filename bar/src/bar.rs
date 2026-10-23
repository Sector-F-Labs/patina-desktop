//! The bar: dwl's status laid out in modules, left, centre and right, as
//! waybar does, in the colours of the active Patina skin.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::{Arc, mpsc};

use egui::{Align, Color32, Layout, Rect, Sense, Stroke, Ui, UiBuilder, vec2};
use patina::skin::{self, Skin};

use crate::config::{Config, Position};
use crate::dwl::{Monitor, Status};
use patina::tabs::{TabAction, TabStrip};
use patina_shell::{Shell, Toplevel, ToplevelRequest};
use patina_system::pins::Tab;

/// Space between modules, and at the bar's ends.
const GAP: f32 = 12.0;
/// The side of a tab's icon, in points, as Patina's TabStrip draws it.
const ICON: f32 = 18.0;

/// An app's icon, as far as the icon thread has got with it.
enum Icon {
    Asked,
    Missing,
    Found(Arc<egui::ColorImage>),
}

/// What a window's tab says (ADR-0061): its app's name, then its title,
/// as "Files - Home"; the name alone when the title only names the app
/// ("Mozilla Firefox"); and without a last part of the title that names
/// the app again, as Firefox's "Patina — Mozilla Firefox" does. Until the
/// name is known, the title.
pub fn tab_title(name: Option<&str>, title: &str, app_id: &str) -> String {
    let Some(name) = name.filter(|n| !n.is_empty()) else {
        return if title.is_empty() { app_id } else { title }.to_string();
    };
    let lower = name.to_lowercase();
    let mut rest = title.trim();
    for separator in [" — ", " – ", " - ", " | "] {
        if let Some((before, last)) = rest.rsplit_once(separator)
            && last.to_lowercase().contains(&lower)
        {
            rest = before.trim();
            break;
        }
    }
    // A title that's only the app's own ("Mozilla Firefox") says nothing
    // more.
    let only_the_app = !rest.contains(" - ")
        && !rest.contains(" — ")
        && !rest.contains(" – ")
        && rest.to_lowercase().contains(&lower);
    if rest.is_empty() || only_the_app {
        name.to_string()
    } else {
        format!("{name} - {rest}")
    }
}

/// The windows `now`, in the order of `before` (as moves left it), those
/// that opened since after them in the order they came.
fn in_order(before: &[Toplevel], now: &[Toplevel]) -> Vec<Toplevel> {
    let still = before
        .iter()
        .filter_map(|b| now.iter().find(|t| t.id == b.id));
    let new = now.iter().filter(|t| !before.iter().any(|b| b.id == t.id));
    still.chain(new).cloned().collect()
}

/// A window tab's tooltip: what it says, and for the first nine, the key
/// that switches to it, as the compositor's Super+1 to 9 follow the tabs.
pub fn tab_tip(title: &str, index: usize) -> String {
    if index < 9 {
        format!("{title}\nSuper+{}", index + 1)
    } else {
        title.to_string()
    }
}

/// The last part of a reverse-DNS app id, as a name: `Nautilus` for
/// org.gnome.Nautilus.
fn last_part(app_id: &str) -> String {
    app_id.rsplit('.').next().unwrap_or(app_id).to_string()
}

/// Each surface's textures of the icons, by app id: every output has an
/// egui context of its own, and a texture belongs to one.
#[derive(Clone, Default)]
struct Textures(HashMap<String, egui::TextureHandle>);

pub struct Bar {
    pub config: Config,
    pub status: Status,
    /// The default output's volume, for the tray's speaker.
    pub volume: Option<patina_system::audio::Volume>,
    /// The Wi-Fi signal, for the tray's Wi-Fi icon (see tray::wifi).
    pub wifi: Option<Option<u8>>,
    /// The battery, for the tray; `None` without one.
    pub battery: Option<patina_system::power::Battery>,
    /// The compositor's keyboard layouts, for the tray's indicator
    /// (ADR-0068); `None` when nothing serves them.
    pub keyboard: Option<patina_system::keyboard::Status>,
    /// Apps' status icons (ADR-0064), and the tray that serves them.
    pub tray_items: Vec<patina_system::sni::Item>,
    pub tray: Option<patina_system::sni::Tray>,
    /// Tray icons by name, by `icons::named_key`.
    named_icons: RefCell<HashMap<String, Icon>>,
    skin: Skin,
    /// The open windows, from the compositor, in the order they opened,
    /// but for those moved since (ADR-0080).
    toplevels: Vec<Toplevel>,
    /// The apps pinned to the bar, as the config says, or as a drop or a
    /// tab's menu just made it.
    pinned: RefCell<Vec<String>>,
    /// The config file, which pinning writes.
    pub config_path: Option<std::path::PathBuf>,
    /// Where a window's move goes when the compositor can't keep it, as
    /// another desktop's can't, and where its `Moved` signals go.
    pub moves: Option<patina_shell::calloop::channel::Sender<(usize, usize)>>,
    /// Clicks on the window tabs, for the host to pass on.
    requests: RefCell<Vec<ToplevelRequest>>,
    /// The window tabs' icons by app id, and where to ask for one.
    icons: RefCell<HashMap<String, Icon>>,
    /// The window tabs' apps' names by app id: `None` while asked.
    names: RefCell<HashMap<String, Option<Option<String>>>>,
    /// Where to ask for an app's icon or name.
    pub ask_app: Option<mpsc::Sender<crate::icons::Ask>>,
    /// What's hovered, and the tooltip showing for it (ADR-0057).
    tips: crate::tooltip::Tips,
    tooltip_shown: Option<crate::tooltip::Shown>,
    /// Where the window tabs' menus send what's chosen (ADR-0056).
    pub tab_menu: Option<patina_shell::calloop::channel::Sender<crate::menu::Chosen>>,
    /// Gaps from the compositor's config: floating, with rounded corners
    /// (ADR-0051).
    pub gaps: crate::gaps::Gaps,
    /// The part of a split bar this draws, if it's split (experimental).
    pub section: Option<crate::split::Section>,
    /// The size a split part wants, from its last frame.
    wanted: Cell<Option<(u32, u32)>>,
}

impl Bar {
    /// Finds `config.skin` (see [`find_skin`]). Problems are returned,
    /// never fatal.
    pub fn new(config: Config) -> (Self, Vec<String>) {
        let (skin, warnings) = find_skin(&config.skin);
        let pinned = RefCell::new(config.pinned.clone());
        (
            Self {
                config,
                status: Status::default(),
                volume: None,
                wifi: None,
                battery: None,
                keyboard: None,
                tray_items: Vec::new(),
                tray: None,
                named_icons: RefCell::new(HashMap::new()),
                skin,
                toplevels: Vec::new(),
                pinned,
                config_path: None,
                moves: None,
                requests: RefCell::new(Vec::new()),
                icons: RefCell::new(HashMap::new()),
                names: RefCell::new(HashMap::new()),
                ask_app: None,
                tab_menu: None,
                tips: Default::default(),
                tooltip_shown: None,
                gaps: crate::gaps::Gaps::default(),
                section: None,
                wanted: Cell::new(None),
            },
            warnings,
        )
    }

    /// Changes to the skin called `name`, as when the desktop's menu set it
    /// in the config file; false if it's the skin already. Restyle after.
    pub fn set_skin(&mut self, name: &str) -> (bool, Vec<String>) {
        if name == self.config.skin {
            return (false, Vec::new());
        }
        let (skin, warnings) = find_skin(name);
        self.config.skin = name.to_string();
        self.skin = skin;
        (true, warnings)
    }

    /// The pinned apps, as the config file says now. Redraw after.
    pub fn set_pinned(&mut self, pinned: Vec<String>) {
        *self.pinned.get_mut() = pinned;
    }

    /// Pins these apps, writing them into the config file.
    fn pin(&self, pinned: Vec<String>) {
        if let Some(path) = &self.config_path
            && let Err(e) = patina_system::pins::set(path, &pinned)
        {
            eprintln!("patina-bar: can't pin: {e}");
        }
        *self.pinned.borrow_mut() = pinned;
    }

    /// Window `from` moved to `to`, as the compositor said, or as the bar
    /// keeps it itself. Redraw after.
    pub fn moved(&mut self, from: usize, to: usize) {
        if from < self.toplevels.len() && to < self.toplevels.len() {
            let window = self.toplevels.remove(from);
            self.toplevels.insert(to, window);
        }
    }

    /// The icon thread's answer for `app_id`. Redraw after.
    /// An app's icon or name, found on the icon thread.
    pub fn app_found(&mut self, found: crate::icons::Found) {
        match found {
            crate::icons::Found::Icon(app_id, image) => {
                let icon = match image {
                    Some(image) => Icon::Found(Arc::new(image)),
                    None => Icon::Missing,
                };
                self.icons.get_mut().insert(app_id, icon);
            }
            crate::icons::Found::Name(app_id, name) => {
                self.names.get_mut().insert(app_id, Some(name));
            }
            crate::icons::Found::Named(key, image) => {
                let icon = match image {
                    Some(image) => Icon::Found(Arc::new(image)),
                    None => Icon::Missing,
                };
                self.named_icons.get_mut().insert(key, icon);
            }
        }
    }

    /// A tray item's icon as a texture `pixels` a side, once it's ready:
    /// its own pixels, or its icon by name, asked of the icon thread.
    fn tray_texture(
        &self,
        ctx: &egui::Context,
        item: &patina_system::sni::Item,
        pixels: u32,
    ) -> Option<egui::TextureId> {
        use patina_system::sni::Icon as TrayIcon;
        use std::hash::{Hash, Hasher};
        let (key, image) = match item.icon.as_ref()? {
            TrayIcon::Pixels { size, rgba } => {
                // Keyed by the pixels too, so a new icon is a new texture.
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                rgba.hash(&mut hasher);
                let key = format!("tray:{}:{:x}", item.key, hasher.finish());
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [*size as usize, *size as usize],
                    rgba,
                );
                (key, Arc::new(image))
            }
            TrayIcon::Named { name, theme_path } => {
                let key = crate::icons::named_key(name, theme_path.as_deref());
                let mut named = self.named_icons.borrow_mut();
                match named.get(&key) {
                    Some(Icon::Found(image)) => (format!("tray-named:{key}"), image.clone()),
                    Some(_) => return None,
                    None => {
                        let ask = self.ask_app.as_ref()?;
                        let question =
                            crate::icons::Ask::Named(name.clone(), theme_path.clone(), pixels);
                        if ask.send(question).is_ok() {
                            named.insert(key, Icon::Asked);
                        }
                        return None;
                    }
                }
            }
        };
        let id = egui::Id::new("patina-bar-icons");
        let known = ctx.data(|d| d.get_temp::<Textures>(id)).unwrap_or_default();
        if let Some(texture) = known.0.get(&key) {
            return Some(texture.id());
        }
        let texture = ctx.load_texture(&key, (*image).clone(), egui::TextureOptions::LINEAR);
        let texture_id = texture.id();
        ctx.data_mut(|d| {
            d.get_temp_mut_or_default::<Textures>(id)
                .0
                .insert(key, texture)
        });
        Some(texture_id)
    }

    /// The apps' status icons, before the bar's own (ADR-0064): a click
    /// activates the app (or shows the menu of one that's only a menu), a
    /// middle click is its secondary action, and a right click opens its
    /// menu at the icon.
    fn tray_items(&self, ui: &mut Ui) {
        use patina_system::sni::Status;
        let pixels = ((crate::tray::side(ui) - 2.0 * crate::tray::PADDING).round()
            * ui.ctx().pixels_per_point())
        .round() as u32;
        for item in self
            .tray_items
            .iter()
            .filter(|i| i.status != Status::Passive)
        {
            let name = if item.title.is_empty() {
                &item.id
            } else {
                &item.title
            };
            let tip = if item.tooltip.is_empty() {
                name
            } else {
                &item.tooltip
            };
            let texture = self.tray_texture(ui.ctx(), item, pixels.max(1));
            let (rect, response) = crate::tray::status_item(ui, texture, name, tip);
            let Some(tray) = self.tray.clone() else {
                continue;
            };
            let at = self.popup(ui.ctx(), rect.center());
            if response.clicked() {
                if item.item_is_menu {
                    open_tray_menu(&tray, &item.key, at);
                } else {
                    tray.activate(&item.key, at.x as i32, at.y as i32);
                }
            } else if response.middle_clicked() {
                tray.secondary_activate(&item.key, at.x as i32, at.y as i32);
            } else if response.secondary_clicked() {
                open_tray_menu(&tray, &item.key, at);
            }
        }
    }

    /// The layout in use, with more than one (ADR-0068): a click lists them
    /// in patina-menu to choose from.
    fn keyboard_layout(&self, ui: &mut Ui) {
        let Some(status) = &self.keyboard else { return };
        let Some((rect, response)) = crate::tray::layout(ui, status) else {
            return;
        };
        if response.clicked() {
            let args = crate::menu::layout_args(self.popup(ui.ctx(), rect.center()), status);
            crate::menu::open_layouts(args);
        }
    }

    /// The name of `app_id`'s app, once it's known: its `.desktop` entry's,
    /// or else the last part of the app id. Asked for the first time it's
    /// wanted; until the answer comes, `None`.
    fn app_name(&self, app_id: &str) -> Option<String> {
        let mut names = self.names.borrow_mut();
        match names.get(app_id) {
            Some(Some(name)) => Some(name.clone().unwrap_or_else(|| last_part(app_id))),
            Some(None) => None,
            None => {
                let ask = self.ask_app.as_ref()?;
                if ask
                    .send(crate::icons::Ask::Name(app_id.to_string()))
                    .is_ok()
                {
                    names.insert(app_id.to_string(), None);
                }
                None
            }
        }
    }

    /// `app_id`'s icon as a texture in `ctx`, asking the icon thread for it
    /// the first time it's wanted.
    fn icon(&self, ctx: &egui::Context, app_id: &str) -> Option<egui::TextureId> {
        let mut icons = self.icons.borrow_mut();
        let image = match icons.get(app_id) {
            Some(Icon::Found(image)) => image.clone(),
            Some(_) => return None,
            None => {
                let ask = self.ask_app.as_ref()?;
                let size = (ICON * ctx.pixels_per_point()).ceil() as u32;
                if ask
                    .send(crate::icons::Ask::Icon(app_id.to_string(), size))
                    .is_ok()
                {
                    icons.insert(app_id.to_string(), Icon::Asked);
                }
                return None;
            }
        };
        let id = egui::Id::new("patina-bar-icons");
        let known = ctx.data(|d| d.get_temp::<Textures>(id)).unwrap_or_default();
        if let Some(texture) = known.0.get(app_id) {
            return Some(texture.id());
        }
        let texture = ctx.load_texture(
            format!("icon-{app_id}"),
            (*image).clone(),
            egui::TextureOptions::LINEAR,
        );
        let texture_id = texture.id();
        ctx.data_mut(|d| {
            d.get_temp_mut_or_default::<Textures>(id)
                .0
                .insert(app_id.to_string(), texture)
        });
        Some(texture_id)
    }

    /// The monitor dwl calls `output`, or, if dwl hasn't mentioned it (a
    /// compositor that names outputs differently), the selected one.
    fn monitor(&self, output: &str) -> Monitor {
        self.status
            .monitors
            .get(output)
            .or_else(|| self.status.selected().map(|(_, m)| m))
            .cloned()
            .unwrap_or_default()
    }

    fn module(&self, ui: &mut Ui, name: &str, m: &Monitor) {
        // Tags and the layout are dwl's; with nothing reporting them, as
        // under patina-compositor, they would only be nine dead buttons.
        let from_dwl = matches!(name, "tags" | "layout");
        if from_dwl && self.status.monitors.is_empty() {
            return;
        }
        match name {
            "tags" => self.tags(ui, m),
            "layout" => {
                ui.label(&m.layout);
            }
            "windows" => self.windows(ui),
            "title" => {
                ui.add(egui::Label::new(&m.title).truncate());
            }
            "appid" => {
                ui.label(&m.appid);
            }
            "tray" => {
                let ctx = ui.ctx().clone();
                let opening = |rect| self.opening(&ctx, rect);
                // The tray's own spacing, tighter than between modules.
                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    self.tray_items(ui);
                    self.keyboard_layout(ui);
                    crate::tray::wifi(ui, self.wifi, &self.config.wifi, &self.skin.name, &opening);
                    crate::tray::speaker(
                        ui,
                        self.volume,
                        &self.config.volume,
                        &self.skin.name,
                        &opening,
                    );
                    ui.add_space(4.0);
                    crate::tray::battery(
                        ui,
                        self.battery,
                        &self.config.battery,
                        &self.skin.name,
                        &opening,
                    );
                });
            }
            "launcher" => {
                let ctx = ui.ctx().clone();
                let opening = |rect| self.opening(&ctx, rect);
                crate::launcher::button(ui, &self.config.launcher, &self.skin.name, &opening)
            }
            "clock" => {
                // In a column, the hours over the minutes.
                if ui.layout().main_dir().is_vertical() {
                    ui.add(egui::Label::new(clock("%H\n%M")).halign(Align::Center));
                } else {
                    ui.label(clock(&self.config.clock));
                }
            }
            "power" => {
                let ctx = ui.ctx().clone();
                let opening = |rect| self.opening(&ctx, rect);
                crate::tray::power(ui, &self.config.power, &self.skin.name, &opening);
            }
            _ => {}
        }
    }

    /// dwm's tags: the viewed ones in the selection colour, a square on
    /// occupied ones (filled where the focused window is), urgent ones in
    /// the error colour.
    fn tags(&self, ui: &mut Ui, m: &Monitor) {
        let v = ui.visuals().clone();
        let font = egui::TextStyle::Body.resolve(ui.style());
        // In a scope, so the tags sit edge to edge but the next module still
        // gets the usual gap.
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (i, label) in self.config.tags.iter().enumerate().take(32) {
                let bit = 1u32 << i;
                let (occupied, viewed, client, urgent) = (
                    m.tags.occupied & bit != 0,
                    m.tags.selected & bit != 0,
                    m.tags.client & bit != 0,
                    m.tags.urgent & bit != 0,
                );
                if self.config.hide_vacant_tags && !occupied && !viewed {
                    continue;
                }
                let (fill, text) = if urgent {
                    (v.error_fg_color, v.extreme_bg_color)
                } else if viewed {
                    (v.selection.bg_fill, v.selection.stroke.color)
                } else if occupied {
                    (Color32::TRANSPARENT, v.strong_text_color())
                } else {
                    (Color32::TRANSPARENT, v.weak_text_color())
                };
                let galley = ui
                    .painter()
                    .layout_no_wrap(label.clone(), font.clone(), text);
                let width = (galley.size().x + 14.0).max(ui.available_height());
                let (rect, response) =
                    ui.allocate_exact_size(vec2(width, ui.available_height()), Sense::hover());
                let fill = if response.hovered() && fill == Color32::TRANSPARENT {
                    v.widgets.hovered.weak_bg_fill
                } else {
                    fill
                };
                ui.painter().rect_filled(rect, 0.0, fill);
                ui.painter()
                    .galley(rect.center() - galley.size() / 2.0, galley, text);
                if occupied {
                    let square = Rect::from_min_size(rect.min + vec2(3.0, 3.0), vec2(4.0, 4.0));
                    if client {
                        ui.painter().rect_filled(square, 0.0, text);
                    } else {
                        ui.painter().rect_stroke(
                            square,
                            0.0,
                            Stroke::new(1.0, text),
                            egui::StrokeKind::Inside,
                        );
                    }
                }
            }
        });
    }

    fn row(
        &self,
        ui: &mut Ui,
        rect: Rect,
        layout: Layout,
        modules: &[String],
        m: &Monitor,
        sizing: bool,
    ) -> Rect {
        let mut builder = UiBuilder::new().max_rect(rect).layout(layout);
        if sizing {
            builder = builder.sizing_pass().invisible();
        }
        let mut child = ui.new_child(builder);
        child.spacing_mut().item_spacing = vec2(GAP, GAP / 2.0);
        for name in modules {
            self.module(&mut child, name, m);
        }
        child.min_rect()
    }
}

impl Bar {
    /// The bar's tabs: the pinned apps, each as its windows or, with none
    /// open, a launcher; then the other windows (ADR-0080).
    fn tabs(&self) -> Vec<Tab> {
        let app_ids: Vec<&str> = self.toplevels.iter().map(|t| t.app_id.as_str()).collect();
        patina_system::pins::arrange(&self.pinned.borrow(), &app_ids)
    }

    /// The app id of a tab's app.
    fn tab_app(&self, tab: Tab) -> String {
        match tab {
            Tab::Window(i) => self.toplevels[i].app_id.clone(),
            Tab::Launcher(p) => self.pinned.borrow()[p].clone(),
        }
    }

    /// The open windows as Patina tabs, after the pinned apps, the active
    /// one shown: a click activates a window or starts a pinned app, its
    /// cross or a middle click closes it, its menu opens another window of
    /// its app (ADR-0055) or pins it, and a drag puts it elsewhere
    /// (ADR-0080).
    fn windows(&self, ui: &mut Ui) {
        let tabs = self.tabs();
        if tabs.is_empty() {
            return;
        }
        let titles: Vec<String> = tabs
            .iter()
            .map(|&tab| match tab {
                Tab::Window(i) => {
                    let t = &self.toplevels[i];
                    tab_title(self.app_name(&t.app_id).as_deref(), &t.title, &t.app_id)
                }
                Tab::Launcher(_) => {
                    let app_id = self.tab_app(tab);
                    tab_title(self.app_name(&app_id).as_deref(), "", &app_id)
                }
            })
            .collect();
        let launchers: Vec<bool> = tabs.iter().map(|t| matches!(t, Tab::Launcher(_))).collect();
        let shown = tabs
            .iter()
            .position(|t| matches!(t, Tab::Window(i) if self.toplevels[*i].activated))
            .unwrap_or(usize::MAX);
        let label = self.config.window_tabs.into();
        // A launcher is its icon, whatever the tabs show.
        let iconic = label != patina::tabs::TabLabel::Title || launchers.contains(&true);
        let icons: Vec<Option<egui::TextureId>> = if iconic {
            tabs.iter()
                .map(|&tab| self.icon(ui.ctx(), &self.tab_app(tab)))
                .collect()
        } else {
            Vec::new()
        };
        // The bar is the tabs' "title bar", so the shown tab is set off
        // from it the way TabStrip sets tabs off a panel-coloured bar.
        let mut style = self.skin.frame_style(ui.visuals());
        style.title_bar = ui.visuals().panel_fill;
        let span = ui.max_rect().y_range();
        let mut menu_at = None;
        let mut open_menu = |i: usize, at: egui::Pos2| menu_at = Some((i, at));
        let mut hovered_tab = None;
        let mut hover_tab = |i: usize, rect: egui::Rect| hovered_tab = Some((i, rect));
        let mut dropped = None;
        let mut moved = |from: usize, to: usize| dropped = Some((from, to));
        // TabStrip zeroes the item spacing; keep that from the next module.
        let action = ui
            .scope(|ui| {
                TabStrip::new(&titles, shown, &style)
                    .id_salt("windows")
                    .new_tab(false)
                    .noun("Window")
                    .icons(&icons)
                    .label(label)
                    .launchers(&launchers)
                    .span(span)
                    .movable(&mut moved)
                    .own_menu(&mut open_menu)
                    .own_tooltip(&mut hover_tab)
                    // On the bar, a line sets the tabs off from the launcher
                    // before them; split, they stand alone.
                    .separator(self.section.is_none())
                    .show(ui)
            })
            .inner;
        if let Some((i, rect)) = hovered_tab {
            crate::tooltip::note(ui, rect, tab_tip(&titles[i], i));
        }
        if let Some((i, at)) = menu_at {
            self.open_tab_menu(ui.ctx(), tabs[i], at);
        }
        if let Some((from, to)) = dropped {
            self.dropped(&tabs, from, to);
        }
        let request = match action.and_then(|a| match a {
            TabAction::Show(i) | TabAction::Close(i) => Some((a, *tabs.get(i)?)),
            TabAction::New => None,
        }) {
            Some((TabAction::Show(_), Tab::Window(i))) => {
                Some(ToplevelRequest::Activate(self.toplevels[i].id))
            }
            Some((TabAction::Close(_), Tab::Window(i))) => {
                Some(ToplevelRequest::Close(self.toplevels[i].id))
            }
            Some((TabAction::Show(_), launcher)) => {
                self.start(&self.tab_app(launcher), false);
                None
            }
            _ => None,
        };
        if let Some(request) = request {
            self.requests.borrow_mut().push(request);
        }
    }
}

impl Bar {
    /// Shows, hides or waits to show the tooltip for what was hovered on
    /// `output` this frame, in patina-menu past the bar's edge (ADR-0057).
    fn tooltip(&mut self, ui: &Ui, output: &str) {
        let hovered = crate::tooltip::take(ui);
        match self.tips.frame(output, hovered, std::time::Instant::now()) {
            crate::tooltip::Action::Show(tip) => {
                let args = crate::tooltip::args(&tip.text, self.popup(ui.ctx(), tip.rect.center()));
                self.tooltip_shown = crate::tooltip::Shown::start(&args);
            }
            crate::tooltip::Action::Hide => self.tooltip_shown = None,
            crate::tooltip::Action::Wait(after) => ui.ctx().request_repaint_after(after),
            crate::tooltip::Action::Nothing => {}
        }
    }

    /// The edge the bar is on, as the pieces it opens are told (they open
    /// from the top or the bottom, unless told where their button is).
    fn edge(&self) -> &'static str {
        let top = match self.section.and_then(|s| s.spot(&self.config)) {
            None => self.config.position == Position::Top,
            Some(spot) => spot.y() == crate::config::Along::Start,
        };
        if top { "top" } else { "bottom" }
    }

    /// Where a piece opened by the button at `rect` goes: from the bar's
    /// edge, at the button.
    fn opening(&self, ctx: &egui::Context, rect: Rect) -> crate::launcher::Opening {
        crate::launcher::Opening {
            edge: self.edge(),
            at: Some(self.popup(ctx, rect.center()).env()),
        }
    }

    /// Where a menu or tooltip for `point` on this surface opens, just past
    /// its edge, wherever the bar or its part is on the screen.
    fn popup(&self, ctx: &egui::Context, point: egui::Pos2) -> crate::split::Popup {
        crate::split::placement(self.section, &self.config, self.gaps).popup(
            point,
            ctx.content_rect().size(),
            patina_shell::output_size(ctx),
        )
    }

    /// Opens `tab`'s menu in patina-menu, at `at` on the bar, just past
    /// its edge (ADR-0056).
    fn open_tab_menu(&self, ctx: &egui::Context, tab: Tab, at: egui::Pos2) {
        let Some(send) = self.tab_menu.clone() else {
            return;
        };
        let app_id = self.tab_app(tab);
        let window = match tab {
            Tab::Window(i) => Some(self.toplevels[i].id),
            Tab::Launcher(_) => None,
        };
        let pinned = self.pinned.borrow().contains(&app_id);
        let choices = crate::menu::choices(window.is_some(), pinned);
        let args = crate::menu::args(self.popup(ctx, at), &choices);
        crate::menu::open(args, window, app_id, move |chosen| {
            let _ = send.send(chosen);
        });
    }

    /// Does what was chosen in a tab's menu.
    pub fn tab_menu_chosen(&mut self, chosen: crate::menu::Chosen) {
        use crate::menu::Choice;
        let mut pinned = self.pinned.borrow().clone();
        match chosen.choice {
            Choice::NewWindow => self.start(&chosen.app_id, true),
            Choice::Open => self.start(&chosen.app_id, false),
            Choice::Pin if !pinned.contains(&chosen.app_id) => {
                pinned.push(chosen.app_id);
                self.pin(pinned);
            }
            Choice::Unpin => {
                pinned.retain(|p| *p != chosen.app_id);
                self.pin(pinned);
            }
            Choice::Pin => {}
            Choice::Close => {
                if let Some(window) = chosen.window {
                    self.requests.get_mut().push(ToplevelRequest::Close(window));
                }
            }
        }
    }

    /// Does what dropping tab `from` of `tabs` at `to` means: the pins
    /// change, or a window moves along, which the compositor keeps so its
    /// keys follow the tabs; the bar moves it itself where nothing does.
    fn dropped(&self, tabs: &[Tab], from: usize, to: usize) {
        let app_ids: Vec<&str> = self.toplevels.iter().map(|t| t.app_id.as_str()).collect();
        let drop = crate::pins::drop(tabs, &self.pinned.borrow(), &app_ids, from, to);
        match drop {
            crate::pins::Drop::Pins(pinned) => self.pin(pinned),
            crate::pins::Drop::Window { from, to } => {
                let Some(moves) = self.moves.clone() else {
                    return;
                };
                std::thread::spawn(move || {
                    // Moved, the compositor's signal moves the tab.
                    if patina_system::pins::move_window(from, to).is_err() {
                        let _ = moves.send((from, to));
                    }
                });
            }
            crate::pins::Drop::Nothing => {}
        }
    }

    /// Starts `app_id`'s app, as its `.desktop` entry says: another window
    /// (its New Window action, if it has one) if `new_window`, else as it
    /// starts. On a thread, since that reads the installed entries.
    fn start(&self, app_id: &str, new_window: bool) {
        let app_id = app_id.to_string();
        let skin = self.skin.name.clone();
        let edge = self.edge();
        std::thread::spawn(move || {
            let entries = patina_system::apps::entries();
            let command = if new_window {
                patina_system::apps::new_window_command(&entries, &app_id)
            } else {
                patina_system::apps::start_command(&entries, &app_id)
            };
            match command {
                Some(command) => {
                    let opening = crate::launcher::Opening { edge, at: None };
                    crate::launcher::run(&command, &skin, &opening)
                }
                None => eprintln!("patina-bar: no .desktop entry or program for {app_id:?}"),
            }
        });
    }
}

/// Opens tray item `key`'s menu at `at`, in patina-menu; the chosen entry
/// is clicked in the app. Reading the menu and waiting for the choice both
/// happen off the bar's thread.
fn open_tray_menu(tray: &patina_system::sni::Tray, key: &str, at: crate::split::Popup) {
    let (clicker, key_for_click) = (tray.clone(), key.to_string());
    tray.menu(key, at.x as i32, at.y as i32, move |entries| {
        if entries.is_empty() {
            return;
        }
        crate::menu::open_tray(crate::menu::tray_args(at, &entries), move |id| {
            clicker.clicked(&key_for_click, id);
        });
    });
}

/// egui's own tooltips would be cut off at the bar's edge: they never
/// show, and the bar's show in patina-menu instead (ADR-0057).
fn no_egui_tooltips(ctx: &egui::Context) {
    ctx.all_styles_mut(|s| s.interaction.tooltip_delay = f32::INFINITY);
}

impl Shell for Bar {
    /// Keeps the windows in the order the bar has them, which moves
    /// change, with the new ones after.
    fn toplevels_changed(&mut self, toplevels: &[Toplevel]) {
        self.toplevels = in_order(&self.toplevels, toplevels);
    }

    fn take_toplevel_requests(&mut self) -> Vec<ToplevelRequest> {
        std::mem::take(self.requests.get_mut())
    }

    fn setup(&mut self, ctx: &egui::Context) {
        self.skin.apply(ctx);
        no_egui_tooltips(ctx);
        // Watched whatever the skin, as the skin can change to one that
        // follows the desktop; other skins set both of egui's themes, so
        // the desktop's choice doesn't touch them.
        patina::appearance::watch(ctx);
    }

    fn restyle(&mut self, ctx: &egui::Context) {
        self.skin.apply(ctx);
        no_egui_tooltips(ctx);
    }

    fn ui(&mut self, ui: &mut Ui, output: &str) {
        match self.section {
            Some(section) => self.draw_part(ui, output, section),
            None => self.draw(ui, output),
        }
        self.tooltip(ui, output);
    }

    fn wanted_size(&self) -> Option<(u32, u32)> {
        self.wanted.get()
    }
}

impl Bar {
    /// A part of a split bar: an island of the bar's colour, as big as what
    /// it shows, which it then asks its surface to be (experimental).
    fn draw_part(&mut self, ui: &mut Ui, output: &str, section: crate::split::Section) {
        use crate::split::Section;
        let m = self.monitor(output);
        let full = ui.max_rect();
        let v = ui.visuals().clone();
        let line = v.widgets.noninteractive.bg_stroke;
        if matches!(section, Section::Reserve(_)) {
            // It only keeps the edge's space.
            self.wanted.set(None);
            return;
        }
        let vertical = section.spot(&self.config).is_some_and(|s| s.vertical());
        let island = full;
        let radius = if self.gaps.floating() {
            self.gaps.radius
        } else {
            8.0
        }
        .min(island.width().min(island.height()) / 2.0);
        ui.painter().rect_filled(island, radius, v.panel_fill);
        ui.painter()
            .rect_stroke(island, radius, line, egui::StrokeKind::Inside);
        let inner = island.shrink(line.width);
        let height = self.config.height.max(1);
        match section {
            Section::Reserve(_) => {}
            Section::Windows if vertical => {
                self.windows_column(ui, inner);
                let rows = self.tabs().len().max(1) as f32;
                let length = rows * inner.width() + 2.0 * line.width;
                self.wanted.set(Some((height, length.ceil() as u32)));
            }
            Section::Windows => {
                // Each tab as wide as a full bar's would be at most, or an
                // icon's.
                let per = if self.config.window_tabs == crate::config::WindowTabs::Icons {
                    44.0
                } else {
                    180.0
                };
                let tabs = inner.shrink2(vec2(GAP, 0.0));
                let row = UiBuilder::new()
                    .max_rect(tabs)
                    .layout(Layout::left_to_right(Align::Center));
                ui.scope_builder(row, |ui| self.windows(ui));
                // A pinned app with no window is an icon's width.
                let width = self
                    .tabs()
                    .iter()
                    .map(|t| {
                        if matches!(t, Tab::Launcher(_)) {
                            44.0
                        } else {
                            per
                        }
                    })
                    .sum::<f32>()
                    .max(per)
                    + 2.0 * GAP;
                self.wanted.set(Some((width.ceil() as u32, height)));
            }
            Section::Launcher | Section::Tray if vertical => {
                let modules = crate::split::modules(section, &self.config);
                let used = self.row(
                    ui,
                    inner.shrink2(vec2(0.0, GAP / 2.0)),
                    Layout::top_down(Align::Center),
                    &modules,
                    &m,
                    false,
                );
                let length = used.height() + GAP + 2.0 * line.width;
                self.wanted
                    .set(Some((height, length.ceil().max(1.0) as u32)));
            }
            Section::Launcher | Section::Tray => {
                let modules = crate::split::modules(section, &self.config);
                // The launcher is its button alone.
                let pad = if section == Section::Launcher {
                    0.0
                } else {
                    GAP
                };
                let used = self.row(
                    ui,
                    inner.shrink2(vec2(pad, 0.0)),
                    Layout::left_to_right(Align::Center),
                    &modules,
                    &m,
                    false,
                );
                let width = used.width() + 2.0 * pad + 2.0 * line.width;
                self.wanted
                    .set(Some((width.ceil().max(1.0) as u32, height)));
            }
        }
    }

    /// The window list as a column of icons, split on the left or right,
    /// after the pinned apps (ADR-0080): a click focuses a window or starts
    /// a pinned app, a middle click closes a window, a right click opens
    /// its menu, and hovering shows its name and title. Unlike the row,
    /// it can't be dragged.
    fn windows_column(&self, ui: &mut Ui, rect: Rect) {
        let side = rect.width();
        let v = ui.visuals().clone();
        let mut request = None;
        let mut menu_at = None;
        let tabs = self.tabs();
        for (i, &tab) in tabs.iter().enumerate() {
            let app_id = self.tab_app(tab);
            let window = match tab {
                Tab::Window(w) => Some(&self.toplevels[w]),
                Tab::Launcher(_) => None,
            };
            let cell = Rect::from_min_size(
                egui::pos2(rect.left(), rect.top() + i as f32 * side),
                vec2(side, side),
            );
            let id = match window {
                Some(t) => egui::Id::new(("window-column", t.id)),
                None => egui::Id::new(("pinned-column", &app_id)),
            };
            let response = ui.interact(cell, id, Sense::click());
            let fill = if window.is_some_and(|t| t.activated) {
                v.extreme_bg_color
            } else if response.hovered() {
                v.widgets.hovered.weak_bg_fill
            } else {
                Color32::TRANSPARENT
            };
            ui.painter().rect_filled(cell.shrink(3.0), 6.0, fill);
            let name = self.app_name(&app_id);
            // A pinned app with no window is half there, as in the row.
            let tint = if window.is_some() {
                Color32::WHITE
            } else {
                Color32::WHITE.gamma_multiply(0.5)
            };
            // At the size it was drawn for, on whole pixels: anything else
            // blurs it.
            let corner = cell.center() - vec2(ICON, ICON) / 2.0;
            let corner =
                egui::emath::GuiRounding::round_to_pixels(corner, ui.ctx().pixels_per_point());
            let mark = Rect::from_min_size(corner, vec2(ICON, ICON));
            match self.icon(ui.ctx(), &app_id) {
                Some(texture) => {
                    let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                    ui.painter().image(texture, mark, uv, tint);
                }
                None => {
                    let letter = name
                        .as_deref()
                        .unwrap_or(&app_id)
                        .chars()
                        .next()
                        .map(|c| c.to_uppercase().to_string())
                        .unwrap_or_default();
                    ui.painter().rect_filled(
                        mark,
                        5.0,
                        v.widgets
                            .inactive
                            .bg_fill
                            .gamma_multiply(tint.a() as f32 / 255.0),
                    );
                    ui.painter().text(
                        mark.center(),
                        egui::Align2::CENTER_CENTER,
                        letter,
                        egui::TextStyle::Body.resolve(ui.style()),
                        v.text_color().gamma_multiply(tint.a() as f32 / 255.0),
                    );
                }
            }
            if response.hovered() {
                let title = tab_title(name.as_deref(), window.map_or("", |t| &t.title), &app_id);
                crate::tooltip::note(ui, cell, tab_tip(&title, i));
            }
            if response.clicked() {
                match window {
                    Some(t) => request = Some(ToplevelRequest::Activate(t.id)),
                    None => self.start(&app_id, false),
                }
            } else if response.middle_clicked()
                && let Some(t) = window
            {
                request = Some(ToplevelRequest::Close(t.id));
            } else if response.secondary_clicked() {
                menu_at = Some((
                    tab,
                    response.interact_pointer_pos().unwrap_or(cell.center()),
                ));
            }
        }
        if let Some(request) = request {
            self.requests.borrow_mut().push(request);
        }
        if let Some((tab, at)) = menu_at {
            self.open_tab_menu(ui.ctx(), tab, at);
        }
    }

    fn draw(&mut self, ui: &mut Ui, output: &str) {
        let m = self.monitor(output);
        let rect = ui.max_rect();
        let v = ui.visuals();
        let line = v.widgets.noninteractive.bg_stroke;
        if self.gaps.floating() {
            // Off the screen's edges, it's outlined all round.
            let radius = self.gaps.radius.min(rect.height() / 2.0);
            ui.painter().rect_filled(rect, radius, v.panel_fill);
            ui.painter()
                .rect_stroke(rect, radius, line, egui::StrokeKind::Inside);
        } else {
            ui.painter().rect_filled(rect, 0.0, v.panel_fill);
            let y = match self.config.position {
                Position::Top => rect.bottom() - line.width / 2.0,
                Position::Bottom => rect.top() + line.width / 2.0,
            };
            ui.painter().hline(rect.x_range(), y, line);
        }

        // Inside the outline, while it floats.
        let rect = if self.gaps.floating() {
            rect.shrink(line.width)
        } else {
            rect
        };
        let inner = rect.shrink2(vec2(GAP, 0.0));
        let left = self.row(
            ui,
            inner,
            Layout::left_to_right(Align::Center),
            &self.config.left,
            &m,
            false,
        );
        // right_to_left lays out from the right edge, so reverse the modules
        // to keep them in the order the config lists them.
        let right_modules: Vec<String> = self.config.right.iter().rev().cloned().collect();
        let right = self.row(
            ui,
            inner,
            Layout::right_to_left(Align::Center),
            &right_modules,
            &m,
            false,
        );

        if self.config.center.is_empty() {
            return;
        }
        let left_end = if self.config.left.is_empty() {
            inner.left()
        } else {
            left.right() + GAP
        };
        let right_start = if self.config.right.is_empty() {
            inner.right()
        } else {
            right.left() - GAP
        };
        if right_start <= left_end {
            return;
        }
        // Measure the centre modules, then centre them on the bar where
        // there's room, or as near it as the side modules allow.
        let free = Rect::from_x_y_ranges(left_end..=right_start, inner.y_range());
        let width = self
            .row(
                ui,
                free,
                Layout::left_to_right(Align::Center),
                &self.config.center,
                &m,
                true,
            )
            .width()
            .min(free.width());
        let x = (rect.center().x - width / 2.0).clamp(free.left(), free.right() - width);
        let centre = Rect::from_x_y_ranges(x..=free.right(), inner.y_range());
        self.row(
            ui,
            centre,
            Layout::left_to_right(Align::Center),
            &self.config.center,
            &m,
            false,
        );
    }
}

/// The skin called `name` among Patina's built-ins and the bar's own skins
/// folder, or Default, with any problems to report.
fn find_skin(name: &str) -> (Skin, Vec<String>) {
    let dirs: Vec<_> = skin::app_dir("patina-bar").into_iter().collect();
    let (skins, mut warnings) = skin::load_all(skin::BUILT_IN, &dirs);
    let skin = match skins.into_iter().find(|s| s.name == name) {
        Some(s) => s,
        None => {
            if name != skin::DEFAULT_SKIN {
                warnings.push(format!(
                    "no skin called \"{name}\"; using {}",
                    skin::DEFAULT_SKIN
                ));
            }
            Skin::default_skin()
        }
    };
    warnings.extend(
        skin.warnings
            .iter()
            .map(|w| format!("skin \"{}\": {w}", skin.name)),
    );
    (skin, warnings)
}

/// Local time in `format`, or a note that the format is broken (chrono
/// reports a bad specifier as a formatting error rather than at parse time).
fn clock(format: &str) -> String {
    let mut out = String::new();
    match write!(out, "{}", chrono::Local::now().format(format)) {
        Ok(()) => out,
        Err(_) => format!("bad clock format: {format}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;

    fn bar(config_text: &str) -> Bar {
        let (config, _) = config::parse(config_text);
        Bar {
            status: Status::default(),
            volume: None,
            wifi: None,
            battery: None,
            keyboard: None,
            tray_items: Vec::new(),
            tray: None,
            named_icons: RefCell::new(HashMap::new()),
            skin: Skin::default_skin(),
            toplevels: Vec::new(),
            pinned: RefCell::new(config.pinned.clone()),
            config_path: None,
            moves: None,
            requests: RefCell::new(Vec::new()),
            icons: RefCell::new(HashMap::new()),
            names: RefCell::new(HashMap::new()),
            ask_app: None,
            tab_menu: None,
            tips: Default::default(),
            tooltip_shown: None,
            gaps: crate::gaps::Gaps::default(),
            section: None,
            wanted: Cell::new(None),
            config,
        }
    }

    #[test]
    fn each_apps_icon_is_asked_for_once_and_drawn_when_found() {
        let mut b = bar("left = [\"windows\"]\ncenter = []\nright = []");
        let (ask, asked) = mpsc::channel();
        b.ask_app = Some(ask);
        let foot = |id, title: &str| Toplevel {
            app_id: "foot".into(),
            ..window(id, title, id == 1)
        };
        b.toplevels_changed(&[foot(1, "one"), foot(2, "two")]);
        let ctx = egui::Context::default();
        let mut time = 0.0;
        frame(&mut b, &ctx, &mut time, vec![]);
        frame(&mut b, &ctx, &mut time, vec![]);
        let wanted: Vec<crate::icons::Ask> = asked.try_iter().collect();
        assert_eq!(
            wanted,
            [
                crate::icons::Ask::Name("foot".into()),
                crate::icons::Ask::Icon("foot".into(), 18)
            ],
            "both windows are foot's"
        );
        b.app_found(crate::icons::Found::Icon(
            "foot".into(),
            Some(egui::ColorImage::filled([18, 18], Color32::RED)),
        ));
        frame(&mut b, &ctx, &mut time, vec![]);
        let textures = ctx
            .data(|d| d.get_temp::<Textures>(egui::Id::new("patina-bar-icons")))
            .expect("the icon became a texture");
        assert_eq!(textures.0.len(), 1);
    }

    /// Draws the window tabs as each `window_tabs` shows them, with this
    /// machine's icons, as PNGs in the folder PATINA_BAR_PREVIEW names:
    /// `cargo test -p patina-bar preview -- --ignored`.
    #[test]
    #[ignore = "reads the installed apps' icons"]
    fn preview_the_window_tabs() {
        let dir = std::path::PathBuf::from(std::env::var("PATINA_BAR_PREVIEW").unwrap());
        let (w, h) = (1000usize, 41usize);
        let apps = [
            ("firefox", "Patina — Mozilla Firefox"),
            ("org.gnome.Nautilus", "Home"),
            ("foot", "divan@huginn: ~"),
            ("com.example.Nothing", "An app without an icon"),
        ];
        let floating = crate::gaps::Gaps {
            gap: 8,
            radius: 8.0,
        };
        for (name, tabs, gaps, pinned) in [
            ("both", "both", Default::default(), ""),
            ("icons", "icons", Default::default(), ""),
            ("titles", "titles", Default::default(), ""),
            ("floating", "both", floating, ""),
            // Files pinned, and a terminal that isn't open (ADR-0080).
            (
                "pinned",
                "both",
                Default::default(),
                "\"org.gnome.Nautilus\", \"org.gnome.Terminal\"",
            ),
        ] {
            let mut b = bar(&format!(
                "left = [\"windows\"]\ncenter = []\nright = [\"clock\"]\nwindow_tabs = \"{tabs}\"\npinned = [{pinned}]"
            ));
            b.gaps = gaps;
            let (ask, asked) = mpsc::channel();
            b.ask_app = Some(ask);
            let windows: Vec<Toplevel> = apps
                .iter()
                .enumerate()
                .map(|(i, (app_id, title))| Toplevel {
                    app_id: app_id.to_string(),
                    ..window(i as u64, title, i == 0)
                })
                .collect();
            b.toplevels_changed(&windows);
            let ctx = egui::Context::default();
            b.setup(&ctx);
            let mut painter = patina_shell::raster::Painter::default();
            let mut pixels = vec![0u32; w * h];
            for _ in 0..3 {
                let input = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        vec2(w as f32, h as f32),
                    )),
                    max_texture_side: Some(8192),
                    ..Default::default()
                };
                let out = ctx.run_ui(input, |ui| b.ui(ui, "A"));
                let primitives = ctx.tessellate(out.shapes, out.pixels_per_point);
                painter.paint(
                    &mut pixels,
                    w,
                    h,
                    out.pixels_per_point,
                    &primitives,
                    &out.textures_delta,
                );
                for question in asked.try_iter() {
                    b.app_found(crate::icons::installed(question));
                }
            }
            let rgba: Vec<u8> = pixels
                .iter()
                .flat_map(|p| {
                    let [b, g, r, a] = p.to_le_bytes();
                    [r, g, b, a]
                })
                .collect();
            let file = dir.join(format!("{name}.png"));
            image::save_buffer(&file, &rgba, w as u32, h as u32, image::ColorType::Rgba8).unwrap();
        }
    }

    /// Draws each part of a split bar as PNGs in PATINA_BAR_PREVIEW, at the
    /// size each asks for: `cargo test -p patina-bar preview_split --
    /// --ignored`.
    #[test]
    #[ignore = "reads the installed apps' icons"]
    fn preview_split() {
        use crate::split::Section;
        let dir = std::path::PathBuf::from(std::env::var("PATINA_BAR_PREVIEW").unwrap());
        let apps = [
            ("firefox", "Patina — Mozilla Firefox"),
            ("org.gnome.Nautilus", "Home"),
            ("com.sectorflabs.Teleprinter", "~"),
        ];
        for (name, section, edge, size) in [
            ("split-windows", Section::Windows, "bottom", (600, 41)),
            ("split-launcher", Section::Launcher, "bottom", (41, 41)),
            ("split-tray", Section::Tray, "bottom", (260, 41)),
            ("split-column", Section::Windows, "center-left", (41, 200)),
            ("split-tray-column", Section::Tray, "center-left", (41, 200)),
        ] {
            let mut b = bar(&format!(
                "layout = \"split\"\nwindows_at = \"{edge}\"\ntray_at = \"{edge}\"\nright = [\"clock\", \"power\"]"
            ));
            b.section = Some(section);
            let (ask, asked) = mpsc::channel();
            b.ask_app = Some(ask);
            let windows: Vec<Toplevel> = apps
                .iter()
                .enumerate()
                .map(|(i, (app_id, title))| Toplevel {
                    app_id: app_id.to_string(),
                    ..window(i as u64, title, i == 0)
                })
                .collect();
            b.toplevels_changed(&windows);
            let ctx = egui::Context::default();
            b.setup(&ctx);
            let mut painter = patina_shell::raster::Painter::default();
            let (mut w, mut h) = size;
            let mut pixels = Vec::new();
            for _ in 0..4 {
                if let Some(wanted) = b.wanted_size() {
                    (w, h) = (wanted.0 as usize, wanted.1 as usize);
                }
                pixels = vec![0u32; w * h];
                let input = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        vec2(w as f32, h as f32),
                    )),
                    max_texture_side: Some(8192),
                    ..Default::default()
                };
                let out = ctx.run_ui(input, |ui| b.ui(ui, "A"));
                let primitives = ctx.tessellate(out.shapes, out.pixels_per_point);
                painter.paint(
                    &mut pixels,
                    w,
                    h,
                    out.pixels_per_point,
                    &primitives,
                    &out.textures_delta,
                );
                for question in asked.try_iter() {
                    b.app_found(crate::icons::installed(question));
                }
            }
            let rgba: Vec<u8> = pixels
                .iter()
                .flat_map(|p| {
                    let [b, g, r, a] = p.to_le_bytes();
                    [r, g, b, a]
                })
                .collect();
            image::save_buffer(
                dir.join(format!("{name}.png")),
                &rgba,
                w as u32,
                h as u32,
                image::ColorType::Rgba8,
            )
            .unwrap();
        }
    }

    #[test]
    fn titles_only_asks_for_names_but_no_icons() {
        let mut b = bar("left = [\"windows\"]\ncenter = []\nright = []\nwindow_tabs = \"titles\"");
        let (ask, asked) = mpsc::channel();
        b.ask_app = Some(ask);
        b.toplevels_changed(&[window(1, "one", true)]);
        let ctx = egui::Context::default();
        frame(&mut b, &ctx, &mut 0.0, vec![]);
        let wanted: Vec<crate::icons::Ask> = asked.try_iter().collect();
        assert!(
            wanted
                .iter()
                .all(|a| matches!(a, crate::icons::Ask::Name(_))),
            "{wanted:?}"
        );
    }

    #[test]
    fn a_tabs_tooltip_says_its_key() {
        assert_eq!(tab_tip("Files - Home", 0), "Files - Home\nSuper+1");
        assert_eq!(tab_tip("Firefox", 8), "Firefox\nSuper+9");
        assert_eq!(tab_tip("Tenth", 9), "Tenth", "no key past nine");
    }

    #[test]
    fn a_tab_says_the_app_then_the_title() {
        assert_eq!(
            tab_title(Some("Files"), "Home", "org.gnome.Nautilus"),
            "Files - Home"
        );
        assert_eq!(tab_title(Some("Teleprinter"), "~", "x"), "Teleprinter - ~");
        // The title is only the app's name: the name alone.
        assert_eq!(
            tab_title(Some("Teleprinter"), "Teleprinter", "x"),
            "Teleprinter"
        );
        assert_eq!(tab_title(Some("Foot"), "", "foot"), "Foot");
        // A last part naming the app again goes.
        assert_eq!(
            tab_title(Some("Firefox"), "Patina — Mozilla Firefox", "firefox"),
            "Firefox - Patina"
        );
        assert_eq!(
            tab_title(Some("Firefox"), "Mozilla Firefox", "firefox"),
            "Firefox"
        );
        assert_eq!(
            tab_title(
                Some("Code"),
                "main.rs - patina - Visual Studio Code",
                "code"
            ),
            "Code - main.rs - patina"
        );
        // Until the name is known, the title as before.
        assert_eq!(tab_title(None, "Home", "org.gnome.Nautilus"), "Home");
        assert_eq!(tab_title(None, "", "foot"), "foot");
    }

    /// Runs the bar for a 1000x26 output and returns the text it drew, with
    /// each galley's left edge.
    fn texts(bar: &mut Bar, output: &str) -> Vec<(String, f32)> {
        let ctx = egui::Context::default();
        bar.setup(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 26.0))),
            ..Default::default()
        };
        let out = ctx.run_ui(input, |ui| bar.ui(ui, output));
        let mut found = Vec::new();
        for clipped in &out.shapes {
            collect(&clipped.shape, &mut found);
        }
        found
    }

    fn collect(shape: &egui::Shape, found: &mut Vec<(String, f32)>) {
        match shape {
            egui::Shape::Text(t) => found.push((t.galley.text().to_string(), t.pos.x)),
            egui::Shape::Vec(v) => v.iter().for_each(|s| collect(s, found)),
            _ => {}
        }
    }

    fn feed(bar: &mut Bar, lines: &[&str]) {
        for l in lines {
            bar.status.apply(l);
        }
    }

    #[test]
    fn the_tray_shows_the_layout_only_with_more_than_one() {
        use patina_system::keyboard::{Layout, Status};
        let layout = |code: &str| Layout {
            code: code.into(),
            variant: String::new(),
            name: code.into(),
            label: code.to_uppercase(),
        };
        let mut b = bar("left = []\ncenter = []\nright = [\"tray\"]");
        b.keyboard = Some(Status {
            layouts: vec![layout("us"), layout("se")],
            current: 1,
        });
        let words: Vec<_> = texts(&mut b, "A").into_iter().map(|(s, _)| s).collect();
        assert!(words.contains(&"SE".to_string()), "{words:?}");
        b.keyboard = Some(Status {
            layouts: vec![layout("us")],
            current: 0,
        });
        let words: Vec<_> = texts(&mut b, "A").into_iter().map(|(s, _)| s).collect();
        assert!(!words.contains(&"US".to_string()), "{words:?}");
    }

    #[test]
    fn shows_the_status_of_its_own_output() {
        let mut b = bar("right = []");
        feed(
            &mut b,
            &[
                "A title on A",
                "A layout []=",
                "A tags 1 1 1 0",
                "B title on B",
                "B selmon 1",
            ],
        );
        let t = texts(&mut b, "A");
        let words: Vec<_> = t.iter().map(|(s, _)| s.as_str()).collect();
        assert!(words.contains(&"on A"), "{words:?}");
        assert!(words.contains(&"[]="));
        assert!(!words.contains(&"on B"));
        assert_eq!(
            words
                .iter()
                .filter(|w| w.len() == 1 && w.chars().all(|c| c.is_ascii_digit()))
                .count(),
            9
        );
    }

    #[test]
    fn unknown_outputs_follow_the_selected_monitor() {
        let mut b = bar("");
        feed(&mut b, &["X title hello", "X selmon 1"]);
        assert!(texts(&mut b, "Y").iter().any(|(s, _)| s == "hello"));
    }

    fn window(id: u64, title: &str, activated: bool) -> Toplevel {
        Toplevel {
            id,
            title: title.into(),
            app_id: format!("app{id}"),
            activated,
        }
    }

    /// Runs frames of the bar at `time` steps, so clicks don't merge.
    fn frame(bar: &mut Bar, ctx: &egui::Context, time: &mut f64, events: Vec<egui::Event>) {
        *time += 0.05;
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 41.0))),
            time: Some(*time),
            events,
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| bar.ui(ui, "A"));
    }

    #[test]
    fn open_windows_are_tabs_and_a_click_activates_one() {
        let mut b = bar("center = []\nright = []");
        b.toplevels_changed(&[window(7, "notes", false), window(9, "", true)]);
        let words: Vec<String> = texts(&mut b, "A").into_iter().map(|(s, _)| s).collect();
        assert!(words.contains(&"notes".to_string()), "{words:?}");
        assert!(
            words.contains(&"app9".to_string()),
            "an untitled window shows its app id"
        );

        let ctx = egui::Context::default();
        b.setup(&ctx);
        let mut time = 0.0;
        frame(&mut b, &ctx, &mut time, vec![]);
        let tab = ctx
            .read_response(egui::Id::new("windows").with(("tab", 0usize)))
            .expect("the first window has a tab")
            .rect;
        // Away from its close cross, which shows at the right end on hover.
        let at = tab.left_center() + vec2(12.0, 0.0);
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame(&mut b, &ctx, &mut time, vec![egui::Event::PointerMoved(at)]);
        frame(&mut b, &ctx, &mut time, vec![button(true)]);
        frame(&mut b, &ctx, &mut time, vec![button(false)]);
        assert_eq!(b.take_toplevel_requests(), [ToplevelRequest::Activate(7)]);
        assert!(b.take_toplevel_requests().is_empty(), "taken once");
    }

    /// Drags from `from` to `to` in steps, and lets go there.
    fn drag(b: &mut Bar, ctx: &egui::Context, time: &mut f64, from: egui::Pos2, to: egui::Pos2) {
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame(b, ctx, time, vec![egui::Event::PointerMoved(from)]);
        frame(b, ctx, time, vec![button(from, true)]);
        for step in 1..=4 {
            let at = from.lerp(to, step as f32 / 4.0);
            frame(b, ctx, time, vec![egui::Event::PointerMoved(at)]);
        }
        frame(b, ctx, time, vec![button(to, false)]);
    }

    #[test]
    fn pinned_apps_come_first_and_one_not_open_starts_it() {
        let mut b = bar("center = []\nright = []\npinned = [\"app9\", \"steam\"]");
        b.toplevels_changed(&[window(7, "notes", true), window(9, "nine", false)]);
        let ctx = egui::Context::default();
        b.setup(&ctx);
        let mut time = 0.0;
        frame(&mut b, &ctx, &mut time, vec![]);
        let tab = |i: usize| {
            ctx.read_response(egui::Id::new("windows").with(("tab", i)))
                .unwrap()
                .rect
        };
        assert_eq!(b.tabs(), [Tab::Window(1), Tab::Launcher(1), Tab::Window(0)]);
        assert!(
            tab(1).width() < tab(0).width(),
            "steam, not open, is its icon"
        );
        let at = tab(0).left_center() + vec2(12.0, 0.0);
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame(&mut b, &ctx, &mut time, vec![egui::Event::PointerMoved(at)]);
        frame(&mut b, &ctx, &mut time, vec![button(true)]);
        frame(&mut b, &ctx, &mut time, vec![button(false)]);
        assert_eq!(
            b.take_toplevel_requests(),
            [ToplevelRequest::Activate(9)],
            "the pinned app's window, first"
        );
    }

    #[test]
    fn a_window_dragged_among_the_pins_pins_its_app() {
        let dir = std::env::temp_dir().join(format!("patina-bar-pins-{}", std::process::id()));
        let path = dir.join("config.toml");
        let mut b = bar("center = []\nright = []\npinned = [\"app9\"]");
        b.config_path = Some(path.clone());
        b.toplevels_changed(&[window(7, "notes", true), window(9, "nine", false)]);
        let ctx = egui::Context::default();
        b.setup(&ctx);
        let mut time = 0.0;
        frame(&mut b, &ctx, &mut time, vec![]);
        let tab = |i: usize| {
            ctx.read_response(egui::Id::new("windows").with(("tab", i)))
                .unwrap()
                .rect
        };
        // notes, last, to before nine, the first.
        let (from, to) = (tab(1).center(), tab(0).left_center() + vec2(4.0, 0.0));
        drag(&mut b, &ctx, &mut time, from, to);
        let written = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(patina_system::pins::read(&written), ["app7", "app9"]);
        assert_eq!(b.tabs(), [Tab::Window(0), Tab::Window(1)]);
        assert!(
            b.take_toplevel_requests().is_empty(),
            "a drag isn't a click"
        );
    }

    #[test]
    fn windows_keep_the_order_they_were_moved_to() {
        let mut b = bar("");
        b.toplevels_changed(&[
            window(1, "a", false),
            window(2, "b", false),
            window(3, "c", false),
        ]);
        b.moved(0, 2);
        b.toplevels_changed(&[
            window(2, "b", true),
            window(3, "c", false),
            window(1, "a!", false),
            window(4, "d", false),
        ]);
        let ids: Vec<u64> = b.toplevels.iter().map(|t| t.id).collect();
        assert_eq!(ids, [2, 3, 1, 4]);
        assert_eq!(b.toplevels[2].title, "a!", "with what they say now");
        b.toplevels_changed(&[
            window(3, "c", false),
            window(4, "d", false),
            window(1, "a", false),
        ]);
        let ids: Vec<u64> = b.toplevels.iter().map(|t| t.id).collect();
        assert_eq!(ids, [3, 1, 4], "closed ones go, the rest stay put");
        b.moved(7, 0);
        assert_eq!(b.toplevels.len(), 3, "a move past the end is ignored");
    }

    #[test]
    fn no_windows_no_tabs() {
        let mut b = bar("left = [\"windows\"]\ncenter = []\nright = []");
        assert!(texts(&mut b, "A").is_empty());
    }

    #[test]
    fn no_tags_without_dwl() {
        let mut b = bar("right = []");
        let t = texts(&mut b, "A");
        assert!(t.iter().all(|(s, _)| s.is_empty()), "{t:?}");
    }

    #[test]
    fn hides_vacant_tags() {
        let mut b = bar("hide_vacant_tags = true\ncenter = []\nright = []\nleft = [\"tags\"]");
        feed(&mut b, &["A tags 5 2 0 0"]);
        let t: Vec<_> = texts(&mut b, "A").into_iter().map(|(s, _)| s).collect();
        assert_eq!(t, ["1", "2", "3"]);
    }

    #[test]
    fn centres_the_title_and_keeps_the_right_in_order() {
        let mut b = bar("left = []\nright = [\"layout\", \"appid\"]");
        feed(&mut b, &["A title mid", "A layout LAY", "A appid APP"]);
        let t = texts(&mut b, "A");
        let x = |w: &str| t.iter().find(|(s, _)| s == w).map(|(_, x)| *x).unwrap();
        assert!((x("mid") - 500.0).abs() < 20.0, "title at {}", x("mid"));
        assert!(x("LAY") < x("APP"));
        assert!(x("APP") > 900.0);
    }

    #[test]
    fn a_bad_clock_format_says_so() {
        assert!(clock("%Q").starts_with("bad clock format"));
        assert_eq!(clock("x"), "x");
    }
}
