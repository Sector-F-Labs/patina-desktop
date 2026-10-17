//! What the tray sees (ADR-0064): serves the StatusNotifierWatcher (or
//! follows the one there), waits a few seconds for apps to register, and
//! prints each item and its menu, clicking nothing.
//!
//! `cargo run -p patina-system --example tray`

use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use patina_system::sni::{self, MenuEntry};

fn print_menu(entries: &[MenuEntry], depth: usize) {
    for entry in entries {
        match entry {
            MenuEntry::Separator => println!("{}  ---", "  ".repeat(depth)),
            MenuEntry::Entry {
                id,
                label,
                enabled,
                checked,
                children,
            } => {
                println!(
                    "{}  [{id}] {label}{}{}",
                    "  ".repeat(depth),
                    if *enabled { "" } else { " (disabled)" },
                    match checked {
                        Some(true) => " ✔",
                        Some(false) => " ☐",
                        None => "",
                    }
                );
                print_menu(children, depth + 1);
            }
        }
    }
}

fn main() {
    let seen: Arc<Mutex<Vec<sni::Item>>> = Arc::default();
    let into = seen.clone();
    let tray = sni::start(22, move |items| {
        *into.lock().unwrap() = items;
        true
    })
    .expect("a session bus");
    std::thread::sleep(Duration::from_secs(4));
    let items = seen.lock().unwrap().clone();
    if items.is_empty() {
        println!("no items registered");
    }
    for item in items {
        let icon = match &item.icon {
            Some(sni::Icon::Pixels { size, .. }) => format!("{size}px pixels"),
            Some(sni::Icon::Named { name, theme_path }) => format!("{name:?} in {theme_path:?}"),
            None => "none".into(),
        };
        println!(
            "{} {:?} {:?} {:?} icon {icon}, menu {}",
            item.key, item.id, item.title, item.status, item.has_menu
        );
        let (tx, rx) = mpsc::channel();
        tray.menu(&item.key, 0, 0, move |entries| {
            let _ = tx.send(entries);
        });
        if let Ok(entries) = rx.recv_timeout(Duration::from_secs(3)) {
            print_menu(&entries, 0);
        }
    }
}
