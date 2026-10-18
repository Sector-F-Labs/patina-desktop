//! Blitz spike: render a message's HTML with Blitz, under ADR-0004's rules.
//! `Guard` is the only way the engine can load anything: it serves `cid:`
//! parts and `data:` URLs, and refuses (and records) everything else.
//! `Links` catches link clicks and form submissions instead of following them.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyrender::ImageRenderer;
use anyrender_vello_cpu::VelloCpuImageRenderer;
use atomic_refcell::AtomicRefCell;
use blitz_dom::{DocumentConfig, EventDriver, NoopEventHandler, UserAgentStylesheet, local_name};
use blitz_traits::events::{
    BlitzPointerEvent, BlitzPointerId, MouseEventButton, MouseEventButtons, PointerCoords, UiEvent,
};
use blitz_html::HtmlDocument;
use blitz_traits::navigation::{NavigationOptions, NavigationProvider};
use blitz_traits::net::{NetHandler, NetProvider, Request};
use blitz_traits::shell::{ColorScheme, Viewport};
use bytes::Bytes;

#[derive(Default)]
pub struct Guard {
    /// Parts of the message by Content-ID, without the angle brackets.
    pub parts: HashMap<String, Bytes>,
    pub refused: Mutex<Vec<String>>,
    pub served: Mutex<Vec<String>>,
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
            Some(b) => {
                self.served.lock().unwrap().push(short(&url));
                handler.bytes(url, b);
            }
            // Answered empty rather than dropped: a stylesheet that never arrives is
            // a pending critical resource, and holds back layout.
            None => {
                self.refused.lock().unwrap().push(short(&url));
                handler.bytes(url, Bytes::new());
            }
        }
    }
}

fn short(url: &str) -> String {
    if url.len() > 90 { format!("{}…", &url[..90]) } else { url.to_string() }
}

/// Link clicks and form submissions, kept for the app to decide on.
#[derive(Default)]
pub struct Links {
    pub clicked: Mutex<Vec<(String, String)>>,
}

impl NavigationProvider for Links {
    fn navigate_to(&self, options: NavigationOptions) {
        self.clicked.lock().unwrap().push((format!("{:?}", options.method), options.url.to_string()));
    }
}

/// Mail's own defaults, after the browser ones: a sans-serif body like other
/// mail clients, and the parts ADR-0004 removes hidden outright.
const MAIL_CSS: &str = "
body { font-family: sans-serif; font-size: 15px; line-height: 1.4; margin: 12px; overflow-wrap: anywhere; }
form, input, button, select, textarea, iframe, object, embed, frame, frameset { display: none !important; }
img { max-width: 100%; }
";

pub struct Message {
    pub doc: HtmlDocument,
    pub guard: Arc<Guard>,
    pub links: Arc<Links>,
}

impl Message {
    /// Parses `html` laid out `width` CSS pixels wide.
    pub fn new(html: &str, parts: HashMap<String, Bytes>, width: u32, scale: f32, dark: bool) -> Self {
        let guard = Arc::new(Guard { parts, ..Default::default() });
        let links = Arc::new(Links::default());
        let scheme = if dark { ColorScheme::Dark } else { ColorScheme::Light };
        let physical = (width as f32 * scale) as u32;
        let config = DocumentConfig {
            viewport: Some(Viewport::new(physical, (600.0 * scale) as u32, scale, scheme)),
            // Relative URLs resolve to cid:, so they can't reach the network either.
            base_url: Some("cid:/".into()),
            net_provider: Some(guard.clone() as Arc<dyn NetProvider>),
            navigation_provider: Some(links.clone() as Arc<dyn NavigationProvider>),
            ua_stylesheets: vec![UserAgentStylesheet::default_sheet(), UserAgentStylesheet::parse(MAIL_CSS)],
            ..Default::default()
        };
        let mut doc = HtmlDocument::from_html(html, config);
        doc.resolve(0.0);
        // The guard answers at once; take in what it served, and lay out again.
        doc.handle_messages();
        doc.resolve(0.0);
        Message { doc, guard, links }
    }

    /// Passes a pointer event at (`x`, `y`) CSS px to Blitz: hover, selection
    /// and link clicks all come from these.
    pub fn pointer(&mut self, event: fn(BlitzPointerEvent) -> UiEvent, x: f32, y: f32, down: bool) {
        let coords = PointerCoords { page_x: x, page_y: y, screen_x: x, screen_y: y, client_x: x, client_y: y };
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
    }

    /// The real target of the link under the pointer, if any.
    pub fn hovered_link(&self) -> Option<String> {
        let mut id = self.doc.get_hover_node_id();
        while let Some(node) = id.and_then(|i| self.doc.get_node(i)) {
            if let Some(el) = node.element_data()
                && el.name.local == local_name!("a")
                && let Some(href) = el.attr(local_name!("href"))
            {
                return Some(href.to_string());
            }
            id = node.parent;
        }
        None
    }

    /// The laid-out height in CSS pixels.
    pub fn height(&self) -> f32 {
        self.doc.root_element().final_layout().size.height
    }

    /// Paints `height` physical pixels from `top` (CSS px), `width` CSS px wide.
    pub fn paint(&mut self, width: u32, top: f32, height: u32, scale: f32) -> Vec<u8> {
        let w = (width as f32 * scale) as u32;
        let mut renderer = VelloCpuImageRenderer::new(w, height);
        let mut buf = Vec::with_capacity((w * height * 4) as usize);
        let doc = &mut *self.doc;
        renderer.render_to_vec(
            |scene| blitz_paint::paint_scene(scene, doc, scale as f64, w, height, 0, (top * scale) as u32),
            &mut buf,
        );
        buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click(m: &mut Message, x: f32, y: f32) {
        m.pointer(UiEvent::PointerMove, x, y, false);
        m.pointer(UiEvent::PointerDown, x, y, true);
        m.pointer(UiEvent::PointerUp, x, y, false);
    }

    #[test]
    fn links_are_caught_not_followed() {
        let mut m = Message::new(r#"<a href="https://evil.example/login">https://proton.me/login</a>"#, HashMap::new(), 600, 1.0, false);
        m.pointer(UiEvent::PointerMove, 30.0, 22.0, false);
        assert_eq!(m.hovered_link().as_deref(), Some("https://evil.example/login"));
        click(&mut m, 30.0, 22.0);
        let clicked = m.links.clicked.lock().unwrap();
        assert_eq!(clicked.len(), 1, "{clicked:?}");
        assert_eq!(clicked[0].1, "https://evil.example/login");
        assert!(m.guard.refused.lock().unwrap().is_empty());
    }

    #[test]
    fn drag_selects_text() {
        let mut m = Message::new("<p>Hello brave new world</p>", HashMap::new(), 600, 1.0, false);
        m.pointer(UiEvent::PointerMove, 13.0, 30.0, false);
        m.pointer(UiEvent::PointerDown, 13.0, 30.0, true);
        m.pointer(UiEvent::PointerMove, 120.0, 30.0, true);
        m.pointer(UiEvent::PointerUp, 120.0, 30.0, false);
        let text = m.doc.get_selected_text().unwrap_or_default();
        assert!(text.starts_with("Hello"), "{text:?}");
    }

    #[test]
    fn scripts_refreshes_and_forms_do_nothing() {
        let html = r#"<meta http-equiv="refresh" content="0; url=https://evil.example/r">
            <script>location = "https://evil.example/s"</script>
            <form action="https://evil.example/f"><button>Go</button></form><p>Body</p>"#;
        let mut m = Message::new(html, HashMap::new(), 600, 1.0, false);
        click(&mut m, 20.0, 20.0);
        assert!(m.links.clicked.lock().unwrap().is_empty());
        assert!(m.guard.refused.lock().unwrap().is_empty());
    }
}
