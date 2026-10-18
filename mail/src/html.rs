//! HTML mail drawn by Blitz in the reading pane (ADR-0005), under ADR-0004's
//! rules: `Guard` is the only way the engine can load anything, and serves
//! only the message's own `cid:` parts and `data:` URLs; `Links` takes link
//! clicks and form submissions instead of following them; and Blitz has no
//! script engine at all.
//!
//! A `Page` lays the message out at the pane's width, or at the mail's own
//! width zoomed down to fit when that's wider, and paints only the part in
//! view, on the CPU, into an egui texture. Pointer events go back to Blitz
//! for hover, text selection and clicks.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyrender::ImageRenderer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use atomic_refcell::AtomicRefCell;
use blitz_dom::{DocumentConfig, EventDriver, NoopEventHandler, UserAgentStylesheet, local_name};
use blitz_html::HtmlDocument;
use blitz_traits::events::{
    BlitzPointerEvent, BlitzPointerId, MouseEventButton, MouseEventButtons, PointerCoords, UiEvent,
};
use blitz_traits::navigation::{NavigationOptions, NavigationProvider};
use blitz_traits::net::{NetHandler, NetProvider, Request};
use blitz_traits::shell::{ColorScheme, Viewport};
use bytes::Bytes;

/// Mail's defaults, after the browser ones: a sans-serif body like other
/// mail clients, and what ADR-0004 removes hidden outright.
const MAIL_CSS: &str = "
body { font-family: sans-serif; font-size: 15px; line-height: 1.4; margin: 12px; overflow-wrap: anywhere; }
form, input, button, select, textarea, iframe, object, embed, frame, frameset { display: none !important; }
img { max-width: 100%; }
";

/// Mail narrower than this zooms no further, and scrolls sideways instead.
const MIN_ZOOM: f32 = 0.4;

/// Serves the message's own parts and refuses everything else.
#[derive(Default)]
struct Guard {
    /// Parts by Content-ID, without the angle brackets.
    parts: HashMap<String, Bytes>,
    refused: Mutex<Vec<String>>,
}

impl NetProvider for Guard {
    fn fetch(&self, _doc_id: usize, request: Request, handler: Box<dyn NetHandler>) {
        let url = request.url.as_str().to_string();
        let bytes = match request.url.scheme() {
            "cid" => self.parts.get(request.url.path()).cloned(),
            "data" => data_url::DataUrl::process(&url).ok().and_then(|d| d.decode_to_vec().ok()).map(|(b, _)| b.into()),
            _ => None,
        };
        match bytes {
            Some(b) => handler.bytes(url, b),
            None => {
                // Answered empty rather than dropped: a stylesheet that never arrives
                // stays a pending critical resource, and holds back layout.
                self.refused.lock().unwrap().push(url.clone());
                handler.bytes(url, Bytes::new());
            }
        }
    }
}

/// Link clicks and form submissions, for the app to decide on.
#[derive(Default)]
struct Links {
    clicked: Mutex<Vec<String>>,
}

impl NavigationProvider for Links {
    fn navigate_to(&self, options: NavigationOptions) {
        // Forms are hidden, but a submission that gets here anyway is dropped.
        if options.method == blitz_traits::net::Method::GET {
            self.clicked.lock().unwrap().push(options.url.to_string());
        }
    }
}

/// A link the user clicked: where it goes, and what its text said.
#[derive(Debug, Clone, PartialEq)]
pub struct Click {
    pub url: String,
    pub text: String,
}

pub struct Page {
    doc: HtmlDocument,
    guard: Arc<Guard>,
    links: Arc<Links>,
    /// The pane width (points) and pixels per point it was laid out for.
    laid_out: (f32, f32),
    zoom: f32,
    /// How far the laid-out page reaches, in CSS pixels (see `measure`).
    extent: egui::Vec2,
    texture: Option<egui::TextureHandle>,
    /// What the texture shows: top and height in physical pixels, and the
    /// generation of the document it was painted from.
    painted: Option<(u32, u32, u64)>,
    generation: u64,
    /// The pointer is pressed in the page, so moves select and a release ends it.
    pressed: bool,
    /// The link under the pointer, kept for when it's clicked.
    hovered: Option<Click>,
}

impl Page {
    /// Lays `html` out for a pane `width` points wide. `parts` are the
    /// message's inline parts by Content-ID.
    pub fn new(html: &str, parts: HashMap<String, Vec<u8>>, width: f32, ppp: f32) -> Self {
        let parts = parts.into_iter().map(|(k, v)| (k, Bytes::from(v))).collect();
        let guard = Arc::new(Guard { parts, ..Default::default() });
        let links = Arc::new(Links::default());
        let config = DocumentConfig {
            // Mail assumes a white page, so it gets one whatever the skin (ADR-0005).
            viewport: Some(Viewport::new(1, 1, ppp, ColorScheme::Light)),
            // Relative URLs resolve to cid:, so they can't reach the network either.
            base_url: Some("cid:/".into()),
            net_provider: Some(guard.clone() as Arc<dyn NetProvider>),
            navigation_provider: Some(links.clone() as Arc<dyn NavigationProvider>),
            ua_stylesheets: vec![UserAgentStylesheet::default_sheet(), UserAgentStylesheet::parse(MAIL_CSS)],
            ..Default::default()
        };
        let mut doc = HtmlDocument::from_html(html, config);
        // The guard answers at once; take in what it served before the first layout.
        doc.resolve(0.0);
        doc.handle_messages();
        let mut page = Page {
            doc,
            guard,
            links,
            laid_out: (0.0, 0.0),
            zoom: 1.0,
            extent: egui::Vec2::ZERO,
            texture: None,
            painted: None,
            generation: 0,
            pressed: false,
            hovered: None,
        };
        page.layout(width, ppp);
        page
    }

    /// Lays out at `width` points, zooming wide mail down to fit.
    fn layout(&mut self, width: f32, ppp: f32) {
        let width = width.max(100.0);
        let set = |doc: &mut HtmlDocument, zoom: f32| {
            let mut viewport = Viewport::new((width * ppp) as u32, (600.0 * ppp) as u32, ppp, ColorScheme::Light);
            viewport.zoom = zoom;
            doc.set_viewport(viewport);
            doc.resolve(0.0);
        };
        set(&mut self.doc, 1.0);
        self.extent = self.measure();
        self.zoom = if self.extent.x > width + 1.0 { (width / self.extent.x).max(MIN_ZOOM) } else { 1.0 };
        if self.zoom < 1.0 {
            set(&mut self.doc, self.zoom);
            self.extent = self.measure();
        }
        self.laid_out = (width, ppp);
        self.generation += 1;
    }

    /// What the guard refused to load: remote images, styles and fonts.
    pub fn blocked(&self) -> Vec<String> {
        self.guard.refused.lock().unwrap().clone()
    }

    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    pub fn selected_text(&self) -> Option<String> {
        self.doc.get_selected_text().filter(|t| !t.is_empty())
    }

    /// The laid-out size in points.
    fn size(&self) -> egui::Vec2 {
        self.extent * self.zoom
    }

    /// How far right and down anything reaches, in CSS pixels. The root's
    /// scroll size isn't enough: Blitz doesn't pass overflow up through
    /// tables, so a 600 px newsletter inside a `width="100%"` table would
    /// still measure as the pane's width.
    fn measure(&self) -> egui::Vec2 {
        let root = self.doc.root_element();
        let mut extent = egui::vec2(root.scroll_width(), root.scroll_height());
        let mut stack = vec![root.id];
        while let Some(id) = stack.pop() {
            let Some(node) = self.doc.get_node(id) else { continue };
            if node.is_element() {
                let size = node.final_layout().size;
                let at = node.absolute_position(0.0, 0.0);
                extent = extent.max(egui::vec2(at.x + size.width, at.y + size.height));
            }
            stack.extend(node.children.iter().copied());
        }
        extent
    }

    /// Draws the page into `ui` (inside a scroll area), and returns a link
    /// if one was clicked.
    ///
    /// `width` is the pane's, taken outside the scroll area: inside one that
    /// scrolls sideways, egui offers more width than the pane has.
    pub fn show(&mut self, ui: &mut egui::Ui, width: f32) -> Option<Click> {
        let ppp = ui.ctx().pixels_per_point();
        if (width - self.laid_out.0).abs() > 0.5 || ppp != self.laid_out.1 {
            self.layout(width, ppp);
        }
        let size = self.size();
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(width.max(size.x), size.y), egui::Sense::click_and_drag());
        ui.painter().rect_filled(rect, 0.0, egui::Color32::WHITE);

        let clicked = self.pointer(ui, &resp, rect);

        // Only the part in view is painted, aligned to physical pixels.
        let visible = rect.intersect(ui.clip_rect());
        if visible.height() > 0.0 {
            let top = ((visible.top() - rect.top()) * ppp).round() as u32;
            let height = (visible.height() * ppp).round().max(1.0) as u32;
            let width_px = (rect.width() * ppp).round() as u32;
            if self.painted != Some((top, height, self.generation)) {
                self.paint(ui.ctx(), top, width_px, height);
            }
            if let Some(texture) = &self.texture {
                let min = egui::pos2(rect.left(), rect.top() + top as f32 / ppp);
                let image_rect = egui::Rect::from_min_size(min, egui::vec2(width_px as f32, height as f32) / ppp);
                let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                ui.painter().image(texture.id(), image_rect, uv, egui::Color32::WHITE);
            }
        }
        clicked
    }

    /// Paints `height` physical pixels from `top` into the texture.
    fn paint(&mut self, ctx: &egui::Context, top: u32, width: u32, height: u32) {
        let (_, ppp) = self.laid_out;
        let scale = ppp * self.zoom;
        // Blitz paints from its viewport's scroll position, in CSS pixels.
        let mut viewport = self.doc.viewport().clone();
        if viewport.window_size != (width, height) {
            viewport.window_size = (width, height);
            self.doc.set_viewport(viewport);
            self.doc.resolve(0.0);
        }
        self.doc.set_viewport_scroll(blitz_dom::Point { x: 0.0, y: top as f64 / scale as f64 });
        let mut renderer = VelloCpuImageRenderer::new(width, height);
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        let doc = &mut *self.doc;
        renderer
            .render_to_vec(|scene| blitz_paint::paint_scene(scene, doc, scale as f64, width, height, 0, 0), &mut rgba);
        let image = egui::ColorImage::from_rgba_premultiplied([width as usize, height as usize], &rgba);
        match &mut self.texture {
            Some(t) => t.set(image, egui::TextureOptions::NEAREST),
            None => self.texture = Some(ctx.load_texture("html-mail", image, egui::TextureOptions::NEAREST)),
        }
        self.painted = Some((top, height, self.generation));
    }

    /// Passes the pointer to Blitz. Page coordinates are CSS pixels from the
    /// top of the message; Blitz takes away its own scroll for the viewport.
    fn pointer(&mut self, ui: &egui::Ui, resp: &egui::Response, rect: egui::Rect) -> Option<Click> {
        let ctx = ui.ctx();
        let (pressed, released, moved) = ctx.input(|i| {
            (i.pointer.primary_pressed(), i.pointer.primary_released(), i.pointer.delta() != egui::Vec2::ZERO)
        });
        let pos = resp.hover_pos().or_else(|| ctx.pointer_interact_pos().filter(|_| self.pressed));
        let mut clicked = None;
        if let Some(pos) = pos {
            let page = (pos - rect.min) / self.zoom;
            if moved || pressed {
                self.send(UiEvent::PointerMove, page, self.pressed);
                self.hovered = self.link_under_pointer();
            }
            if pressed && resp.hovered() {
                self.pressed = true;
                self.send(UiEvent::PointerDown, page, true);
            }
            if released && self.pressed {
                self.pressed = false;
                self.send(UiEvent::PointerUp, page, false);
                let urls: Vec<String> = std::mem::take(&mut *self.links.clicked.lock().unwrap());
                clicked = urls.into_iter().last().map(|url| {
                    let text = self.hovered.as_ref().map(|h| h.text.clone()).unwrap_or_default();
                    Click { url, text }
                });
            }
        } else if self.pressed && released {
            self.pressed = false;
        }
        if resp.hovered() {
            let icon = match self.doc.get_cursor().map(|c| format!("{c:?}")) {
                Some(c) if c == "Pointer" => egui::CursorIcon::PointingHand,
                Some(c) if c == "Text" => egui::CursorIcon::Text,
                _ => egui::CursorIcon::Default,
            };
            ctx.set_cursor_icon(icon);
            if let Some(link) = &self.hovered {
                resp.clone().on_hover_text_at_pointer(&link.url);
            }
        }
        clicked
    }

    fn send(&mut self, event: fn(BlitzPointerEvent) -> UiEvent, page: egui::Vec2, down: bool) {
        let scroll = self.doc.viewport_scroll();
        let coords = PointerCoords {
            page_x: page.x,
            page_y: page.y,
            screen_x: page.x,
            screen_y: page.y,
            client_x: page.x - scroll.x as f32,
            client_y: page.y - scroll.y as f32,
        };
        let e = BlitzPointerEvent {
            id: BlitzPointerId::Mouse,
            is_primary: true,
            coords,
            button: MouseEventButton::Main,
            buttons: if down { MouseEventButtons::Primary } else { MouseEventButtons::None },
            mods: Default::default(),
            details: Default::default(),
            element: Default::default(),
            active_pointers: Arc::new(AtomicRefCell::new(Vec::new())),
        };
        EventDriver::new(&mut self.doc, NoopEventHandler).handle_ui_event(event(e));
        self.doc.resolve(0.0);
        self.generation += 1;
    }

    /// The link under the pointer: its real target and its text.
    fn link_under_pointer(&self) -> Option<Click> {
        let mut id = self.doc.get_hover_node_id();
        while let Some(node) = id.and_then(|i| self.doc.get_node(i)) {
            if let Some(el) = node.element_data()
                && el.name.local == local_name!("a")
                && let Some(href) = el.attr(local_name!("href"))
            {
                return Some(Click { url: href.to_string(), text: node.text_content().trim().to_string() });
            }
            id = node.parent;
        }
        None
    }
}

/// The host a link's text claims, when the text looks like an address and
/// names a different site from where the link goes: the classic phishing
/// link. `None` when it's fine, or the text isn't an address.
pub fn misleading(text: &str, url: &str) -> Option<String> {
    let host = |s: &str| -> Option<String> {
        let s = s.trim().trim_end_matches('/');
        if s.contains(char::is_whitespace) || !s.contains('.') {
            return None;
        }
        let with_scheme = if s.contains("://") { s.to_string() } else { format!("https://{s}") };
        let host = url_host(&with_scheme)?;
        // Needs a dot and a letter-only top level, so "3.5" and "v1.2" aren't hosts.
        let tld = host.rsplit('.').next()?;
        (host.contains('.') && tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic())).then_some(host)
    };
    let claimed = host(text)?;
    let real = url_host(url)?;
    let bare = |h: &str| h.trim_start_matches("www.").to_ascii_lowercase();
    (bare(&claimed) != bare(&real)).then_some(claimed)
}

fn url_host(s: &str) -> Option<String> {
    let rest = s.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit('@').next()?.split(':').next()?;
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(html: &str, width: f32) -> Page {
        Page::new(html, HashMap::new(), width, 1.0)
    }

    fn click(p: &mut Page, x: f32, y: f32) {
        let at = egui::vec2(x, y);
        p.send(UiEvent::PointerMove, at, false);
        p.send(UiEvent::PointerDown, at, true);
        p.send(UiEvent::PointerUp, at, false);
    }

    #[test]
    fn remote_loads_are_refused_and_inline_parts_served() {
        let html = r#"<link rel="stylesheet" href="https://t.example/a.css">
            <style>@import url("https://t.example/b.css"); body { background: url(https://t.example/bg.png) }</style>
            <img src="cid:logo@x"><img src="https://t.example/pixel.gif"><img src="data:image/png;base64,iVBORw0KGgo=">
            <img src="relative.png">"#;
        let png = std::fs::read("assets/mail-icon-128.png").unwrap();
        let p = Page::new(html, HashMap::from([("logo@x".to_string(), png)]), 600.0, 1.0);
        let mut blocked = p.blocked();
        blocked.sort();
        assert_eq!(
            blocked,
            vec![
                "cid:/relative.png",
                "https://t.example/a.css",
                "https://t.example/b.css",
                "https://t.example/bg.png",
                "https://t.example/pixel.gif",
            ]
        );
    }

    #[test]
    fn links_are_caught_with_their_text() {
        let mut p = page(r#"<a href="https://evil.example/login">https://proton.me/login</a>"#, 600.0);
        click(&mut p, 30.0, 22.0);
        p.hovered = p.link_under_pointer();
        let hovered = p.hovered.clone().unwrap();
        assert_eq!(hovered, Click { url: "https://evil.example/login".into(), text: "https://proton.me/login".into() });
        assert_eq!(*p.links.clicked.lock().unwrap(), vec!["https://evil.example/login".to_string()]);
    }

    #[test]
    fn drag_selects_text() {
        let mut p = page("<p>Hello brave new world</p>", 600.0);
        p.send(UiEvent::PointerMove, egui::vec2(13.0, 30.0), false);
        p.send(UiEvent::PointerDown, egui::vec2(13.0, 30.0), true);
        p.send(UiEvent::PointerMove, egui::vec2(120.0, 30.0), true);
        p.send(UiEvent::PointerUp, egui::vec2(120.0, 30.0), false);
        assert!(p.selected_text().unwrap_or_default().starts_with("Hello"));
    }

    #[test]
    fn scripts_refreshes_and_forms_do_nothing() {
        let html = r#"<meta http-equiv="refresh" content="0; url=https://evil.example/r">
            <script>location = "https://evil.example/s"</script>
            <form action="https://evil.example/f" method="post"><button>Go</button></form><p>Body</p>"#;
        let mut p = page(html, 600.0);
        click(&mut p, 20.0, 20.0);
        assert!(p.links.clicked.lock().unwrap().is_empty());
        assert!(p.blocked().is_empty());
    }

    #[test]
    fn wide_mail_zooms_to_fit() {
        let html = r#"<table width="600"><tr><td>Fixed-width newsletter</td></tr></table>"#;
        let narrow = page(html, 400.0);
        // 400 / (600 + the body's 12 px margin).
        assert!((narrow.zoom() - 400.0 / 612.0).abs() < 0.01, "zoom {}", narrow.zoom());
        assert!(narrow.size().x <= 401.0, "{:?}", narrow.size());
        assert_eq!(page(html, 900.0).zoom(), 1.0);
        assert_eq!(page("<p>Plain words wrap</p>", 300.0).zoom(), 1.0);
    }

    #[test]
    fn newsletters_nested_in_full_width_tables_zoom_too() {
        let html = r#"<table width="100%"><tr><td align="center" style="padding:12px">
            <table width="600"><tr><td>Newsletter</td></tr></table></td></tr></table>"#;
        let p = page(html, 380.0);
        assert!(p.zoom() < 0.62, "zoom {}", p.zoom());
        assert!(p.size().x <= 381.0, "{:?}", p.size());
    }

    #[test]
    fn spots_misleading_links() {
        assert_eq!(misleading("https://proton.me/login", "https://evil.example/x"), Some("proton.me".into()));
        assert_eq!(misleading("paypal.com", "http://paypa1.example"), Some("paypal.com".into()));
        assert_eq!(misleading("www.proton.me", "https://proton.me/mail"), None);
        assert_eq!(misleading("Read more", "https://evil.example"), None);
        assert_eq!(misleading("version 3.5", "https://x.example"), None);
        assert_eq!(misleading("v1.2", "https://x.example"), None);
        assert_eq!(misleading("me@proton.me", "mailto:me@proton.me"), None);
    }
}
