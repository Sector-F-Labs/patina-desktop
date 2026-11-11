//! `patina-webview SLUG`: a web app (ADR-0133) in a window of its own, on WebKitGTK. The
//! window is the page and nothing else: no tabs, address bar or title bar, and its app-id
//! says which web app it is, so the compositor draws the frame.

use std::cell::RefCell;
use std::process::Command;
use std::rc::Rc;

use gtk4::prelude::*;
use patina_webapp::look::{self, RADIUS};
use patina_webapp::{ID_PREFIX, Store};
use webkit6::prelude::*;
use webkit6::{NavigationType, NetworkSession, PolicyDecisionType, WebView};

/// The colour the page shows along its top edge, as `#rrggbb`, or null: the first opaque
/// background from the element at the top centre up through its parents. It's what a frame that
/// blends in should be, in whatever scheme the page is in, which a manifest's one declared
/// colour isn't (a dark page declares a light colour).
const SAMPLE: &str = r#"(() => {
    const parse = c => {
        const m = c.match(/rgba?\(([^)]+)\)/);
        if (!m) return null;
        const p = m[1].split(/[ ,\/]+/).filter(Boolean).map(Number);
        return p.length >= 4 && p[3] < 0.5 ? null : p.slice(0, 3);
    };
    for (let el = document.elementFromPoint(Math.floor(innerWidth / 2), 4); el; el = el.parentElement) {
        const c = parse(getComputedStyle(el).backgroundColor);
        if (c) return '#' + c.map(v => Math.round(v).toString(16).padStart(2, '0')).join('');
    }
    return null;
})()"#;

/// What a page says about itself: the tags and manifest, and the colour it shows, posted back
/// as JSON for `probe`.
fn probe_script() -> String {
    format!(
        r#"(async () => {{
    const meta = document.querySelector('meta[name="theme-color"]');
    const icons = [...document.querySelectorAll('link[rel~="icon"], link[rel="apple-touch-icon"]')]
        .map(l => ({{ href: l.href, sizes: l.getAttribute('sizes') || '' }}));
    const link = document.querySelector('link[rel="manifest"]');
    let manifest = null;
    if (link) {{ try {{ manifest = await (await fetch(link.href)).json(); }} catch (e) {{}} }}
    window.webkit.messageHandlers.probe.postMessage(JSON.stringify({{
        color: meta ? meta.content : null, top: {SAMPLE},
        icons, manifestUrl: link ? link.href : null, manifest }}));
}})()"#
    )
}

/// Keeps the stored colour the page's as it is now: every few seconds the colour along the
/// page's top edge is read, and one that has held for two readings in a row (not a splash screen
/// or a theme flicker) is stored for the compositor's title bar and drawn round the page. A
/// page can change after login, or when the scheme does, and the colour read at install can't
/// know.
fn follow_the_page(
    view: &WebView,
    slug: &str,
    stored: Option<String>,
    apply: Rc<dyn Fn(Option<look::Rgb>)>,
) {
    let state = Rc::new(RefCell::new((stored, None::<String>)));
    let (view, slug) = (view.clone(), slug.to_string());
    gtk4::glib::timeout_add_seconds_local(4, move || {
        let (state, apply, slug) = (state.clone(), apply.clone(), slug.clone());
        view.evaluate_javascript(SAMPLE, None, None, gtk4::gio::Cancellable::NONE, move |r| {
            let Ok(value) = r else { return };
            if !value.is_string() {
                return;
            }
            let seen = value.to_str().to_string();
            let mut state = state.borrow_mut();
            if state.0.as_deref() == Some(&seen) {
                state.1 = None;
            } else if state.1.as_deref() == Some(&seen) {
                state.0 = Some(seen.clone());
                state.1 = None;
                if let Some(rgb) = look::parse(&seen) {
                    apply(Some(rgb));
                }
                if let Ok(store) = Store::user() {
                    let _ = store.set_color(&slug, &seen);
                }
            } else {
                state.1 = Some(seen);
            }
        });
        gtk4::glib::ControlFlow::Continue
    });
}

/// `patina-webview probe URL`: loads the page in a window that's never shown, and prints what
/// it says about itself (its colour, icons and manifest) as JSON, for a web app's install and
/// refresh when the site won't answer a plain fetch.
fn probe(url: String) -> gtk4::glib::ExitCode {
    let application = gtk4::Application::new(None::<&str>, Default::default());
    application.connect_activate(move |application| {
        let view = WebView::new();
        // The page's script can wait on its manifest, which a returned promise can't carry out
        // of `evaluate_javascript`, so it posts what it found.
        if let Some(manager) = view.user_content_manager() {
            manager.register_script_message_handler("probe", None);
            let application = application.clone();
            manager.connect_script_message_received(Some("probe"), move |_, value| {
                println!("{}", value.to_str());
                application.quit();
            });
        }
        let window = gtk4::ApplicationWindow::builder()
            .application(application)
            .child(&view)
            .build();
        view.connect_load_changed(move |view, event| {
            if event != webkit6::LoadEvent::Finished {
                return;
            }
            // Single-page sites finish loading before they've written their head.
            gtk4::glib::timeout_add_seconds_local_once(2, {
                let view = view.clone();
                move || {
                    view.evaluate_javascript(
                        &probe_script(),
                        None,
                        None,
                        gtk4::gio::Cancellable::NONE,
                        |_| {},
                    );
                }
            });
        });
        view.load_uri(&url);
        // Held, not shown: the window is only here so the page has somewhere to load.
        std::mem::forget(window);
    });
    // Held so the application runs without a window shown, until the probe quits it.
    let hold = application.hold();
    let code = application.run_with_args::<&str>(&[]);
    drop(hold);
    code
}

fn main() -> gtk4::glib::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let [command, url] = &args[..]
        && command == "probe"
    {
        return probe(url.clone());
    }
    let Some(slug) = std::env::args().nth(1) else {
        eprintln!("usage: patina-webview SLUG");
        return gtk4::glib::ExitCode::FAILURE;
    };
    let Some(app) = Store::user().ok().and_then(|s| s.find(&slug)) else {
        eprintln!("patina-webview: no web app called {slug}");
        return gtk4::glib::ExitCode::FAILURE;
    };
    // GTK 4 on Wayland takes the window's app-id from the program name.
    gtk4::glib::set_prgname(Some(&format!("{ID_PREFIX}{slug}")));

    let application = gtk4::Application::new(None::<&str>, Default::default());
    application.connect_activate(move |application| {
        let data = Store::user()
            .map(|s| s.profile_dir(&app.slug))
            .unwrap_or_default();
        let cache = dirs::cache_dir()
            .unwrap_or_default()
            .join("patina-webapp")
            .join(&app.slug);
        let session = NetworkSession::new(data.to_str(), cache.to_str());
        let view = WebView::builder().network_session(&session).build();

        let site = host(&app.url);
        view.connect_decide_policy(move |_, decision, kind| {
            if kind != PolicyDecisionType::NavigationAction
                && kind != PolicyDecisionType::NewWindowAction
            {
                return false;
            }
            let Some(decision) = decision.downcast_ref::<webkit6::NavigationPolicyDecision>()
            else {
                return false;
            };
            let Some(mut action) = decision.navigation_action() else {
                return false;
            };
            let Some(uri) = action.request().and_then(|r| r.uri()) else {
                return false;
            };
            let leaves = host(&uri) != site;
            let clicked = action.navigation_type() == NavigationType::LinkClicked;
            // A link that goes to another site opens in the user's browser.
            if leaves && (clicked || kind == PolicyDecisionType::NewWindowAction) {
                let _ = Command::new("xdg-open").arg(uri.as_str()).spawn();
                decision.ignore();
                return true;
            }
            false
        });

        // The frame's colour (stored: read when the web app was installed, and kept up to date
        // below): the compositor's title bar is this, and the page gets a border to match,
        // rounded at the bottom with the corners outside it left transparent, as Patina's own
        // apps do.
        let css = gtk4::CssProvider::new();
        if let Some(display) = gtk4::gdk::Display::default() {
            gtk4::style_context_add_provider_for_display(
                &display,
                &css,
                gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
        let apply = {
            let (css, view) = (css.clone(), view.clone());
            Rc::new(move |color: Option<look::Rgb>| {
                let border = color.map(look::border_for).unwrap_or([0x3a, 0x3f, 0x44]);
                if let Some([r, g, b]) = color {
                    view.set_background_color(&gtk4::gdk::RGBA::new(
                        f32::from(r) / 255.0,
                        f32::from(g) / 255.0,
                        f32::from(b) / 255.0,
                        1.0,
                    ));
                }
                css.load_from_string(&format!(
                    "window.patina-webapp, window.patina-webapp.background {{ background: transparent; }}\n\
                     .patina-page {{ border: 1px solid {border}; border-top-width: 0; \
                     border-radius: 0 0 {RADIUS}px {RADIUS}px; }}",
                    border = look::hex(border),
                ));
            })
        };
        apply(app.color.as_deref().and_then(look::parse));
        follow_the_page(&view, &app.slug, app.color.clone(), apply);
        let page = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        page.add_css_class("patina-page");
        view.set_vexpand(true);
        view.set_hexpand(true);
        page.append(&view);
        // What clips the page to the rounded corners; CSS has no property for it.
        page.set_overflow(gtk4::Overflow::Hidden);

        let window = gtk4::ApplicationWindow::builder()
            .application(application)
            .default_width(960)
            .default_height(640)
            .decorated(false)
            .title(&app.name)
            .child(&page)
            .build();
        window.add_css_class("patina-webapp");
        view.connect_title_notify({
            let window = window.clone();
            move |view| window.set_title(view.title().as_deref())
        });
        view.load_uri(&app.url);
        window.present();
    });
    application.run_with_args::<&str>(&[])
}

/// The host of an address: `web.whatsapp.com` for `https://web.whatsapp.com/x`.
fn host(uri: &str) -> String {
    uri.split("://")
        .nth(1)
        .unwrap_or(uri)
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .to_string()
}
