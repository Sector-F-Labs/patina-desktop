//! Made-up mail for `MAIL_DEMO=1`: screenshots and trying the UI without
//! Bridge. Nothing here connects anywhere.

use std::collections::HashMap;

use crate::config::Account;
use crate::message::{Addr, Attachment, Body, Summary};
use crate::sync::Folder;

/// A newsletter with a remote stylesheet, image and tracking pixel, all
/// of which the reader blocks.
const NEWSLETTER: &str = include_str!("../assets/demo/newsletter.html");

pub fn account() -> Account {
    Account { username: "me@proton.me".into(), password: "demo".into(), name: "Me".into(), ..Default::default() }
}

fn addr(name: &str, address: &str) -> Addr {
    Addr { name: Some(name.into()), address: address.into() }
}

pub fn mailbox() -> (Vec<Folder>, Vec<Summary>, Vec<(u32, Body)>) {
    let folder =
        |name: &str, label: &str, depth, unseen| Folder { name: name.into(), label: label.into(), depth, unseen };
    let folders = vec![
        folder("INBOX", "Inbox", 0, 3),
        folder("Drafts", "Drafts", 0, 0),
        folder("Sent", "Sent", 0, 0),
        folder("Starred", "Starred", 0, 0),
        folder("Archive", "Archive", 0, 0),
        folder("Spam", "Spam", 0, 0),
        folder("Trash", "Trash", 0, 0),
        folder("All Mail", "All Mail", 0, 0),
        folder("Folders/Receipts", "Receipts", 1, 0),
        folder("Folders/Conference Planning", "Conference Planning", 1, 12),
        folder("Folders/Travel", "Travel", 1, 1),
        folder("Labels/Family", "Family", 1, 0),
    ];
    let now = chrono::Local::now().timestamp();
    let hour = 3600;
    let mail = [
        (
            addr("Proton", "no-reply@proton.me"),
            "Your month in mail",
            now - hour / 3,
            false,
            false,
            "October at a glance: 1,284 messages received, 312 sent, 97 trackers blocked.",
        ),
        (
            addr("Ada Lovelace", "ada@example.com"),
            "Notes on the Analytical Engine",
            now - hour,
            false,
            true,
            "Hi,\n\nI've finished the notes on Menabrea's paper. Note G has the table for computing \
Bernoulli numbers; could you check the second column before I send it to Babbage?\n\n\
The engine might act upon other things besides number, were objects found whose mutual \
relations could be expressed by those of the abstract science of operations.\n\nAda",
        ),
        (
            addr("Grace Hopper", "grace@example.com"),
            "Re: The bug in relay 70",
            now - 3 * hour,
            false,
            false,
            "Found it. A moth, taped into the log book.\n\nGrace",
        ),
        (
            addr("Linus", "linus@example.com"),
            "Patch review for 6.18",
            now - 50 * hour,
            true,
            false,
            "Looks fine, applied.",
        ),
        (
            addr("Margaret Hamilton", "margaret@example.com"),
            "Priority displays",
            now - 80 * hour,
            true,
            true,
            "The 1202 alarms were the executive doing its job.",
        ),
        (
            addr("Dennis Ritchie", "dmr@example.com"),
            "C, the second edition",
            now - 200 * hour,
            true,
            false,
            "The proofs are attached.",
        ),
        (
            addr("Ken Thompson", "ken@example.com"),
            "UTF-8 on a placemat",
            now - 400 * hour,
            true,
            false,
            "Rob and I worked it out over dinner.",
        ),
        (
            addr("Barbara Liskov", "barbara@example.com"),
            "Substitution",
            now - 900 * hour,
            true,
            false,
            "Subtypes must be substitutable.",
        ),
    ];
    let mut summaries = Vec::new();
    let mut bodies = Vec::new();
    for (i, (from, subject, date, seen, flagged, text)) in mail.into_iter().enumerate() {
        let uid = 100 - i as u32;
        summaries.push(Summary { uid, from: Some(from.clone()), subject: subject.into(), date, seen, flagged });
        // The newsletter is HTML, with its logo as an inline part.
        let (html, parts) = if i == 0 {
            let logo = include_bytes!("../assets/mail-icon-128.png").to_vec();
            (Some(NEWSLETTER.to_string()), HashMap::from([("logo@demo".to_string(), logo)]))
        } else {
            (None, HashMap::new())
        };
        let attachments =
            if i == 1 { vec![Attachment { name: "note-g.pdf".into(), data: vec![0; 48 * 1024] }] } else { Vec::new() };
        bodies.push((
            uid,
            Body {
                from: vec![from],
                to: vec![addr("Me", "me@proton.me")],
                subject: subject.into(),
                date,
                message_id: Some(format!("{uid}@example.com")),
                text: text.into(),
                html,
                parts,
                attachments,
                ..Default::default()
            },
        ));
    }
    (folders, summaries, bodies)
}
