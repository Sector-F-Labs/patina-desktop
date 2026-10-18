//! A message being written: a `Draft` for the compose window, started blank
//! or as a reply or forward, and turned into a lettre `Message` to send.

use lettre::message::{Mailbox, header::ContentType};

use crate::config::Account;
use crate::message::{self, Addr, Body};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Draft {
    pub to: String,
    pub cc: String,
    pub subject: String,
    pub text: String,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
}

/// Adds `prefix` ("Re:", "Fwd:") unless the subject already starts with it.
fn prefixed(prefix: &str, subject: &str) -> String {
    let has = subject.len() >= prefix.len() && subject[..prefix.len()].eq_ignore_ascii_case(prefix);
    if has { subject.to_string() } else { format!("{prefix} {subject}") }
}

fn date_line(date: i64) -> String {
    chrono::DateTime::from_timestamp(date, 0)
        .map(|d| d.with_timezone(&chrono::Local).format("%a, %-d %b %Y at %H:%M").to_string())
        .unwrap_or_default()
}

fn quote(text: &str) -> String {
    text.lines().map(|l| if l.is_empty() { ">".to_string() } else { format!("> {l}") }).collect::<Vec<_>>().join("\n")
}

impl Draft {
    /// A reply to `original`, to its sender, or with `all` to everyone on it
    /// but `me`.
    pub fn reply(original: &Body, me: &str, all: bool) -> Self {
        let sender = if original.reply_to.is_empty() { &original.from } else { &original.reply_to };
        let is_me = |a: &Addr| a.address.eq_ignore_ascii_case(me);
        let mut to: Vec<Addr> = sender.clone();
        if to.iter().all(is_me) && all {
            // Replying to your own message: it goes to whoever it went to.
            to = original.to.clone();
        }
        let mut cc = Vec::new();
        if all {
            for a in original.to.iter().chain(&original.cc) {
                let known = to.iter().chain(&cc).any(|b: &Addr| b.address.eq_ignore_ascii_case(&a.address));
                if !known && !is_me(a) {
                    cc.push(a.clone());
                }
            }
        }
        let who = original.from.first().map(Addr::to_string).unwrap_or_default();
        let mut references = original.references.clone();
        if let Some(id) = &original.message_id {
            references.push(id.clone());
        }
        Draft {
            to: message::join(&to),
            cc: message::join(&cc),
            subject: prefixed("Re:", &original.subject),
            text: format!("\n\nOn {}, {who} wrote:\n{}\n", date_line(original.date), quote(original.text.trim_end())),
            in_reply_to: original.message_id.clone(),
            references,
        }
    }

    pub fn forward(original: &Body) -> Self {
        let mut header = String::from("\n\n---------- Forwarded message ----------\n");
        header += &format!("From: {}\n", message::join(&original.from));
        header += &format!("Date: {}\n", date_line(original.date));
        header += &format!("Subject: {}\n", original.subject);
        header += &format!("To: {}\n", message::join(&original.to));
        if !original.cc.is_empty() {
            header += &format!("Cc: {}\n", message::join(&original.cc));
        }
        Draft {
            subject: prefixed("Fwd:", &original.subject),
            text: format!("{header}\n{}\n", original.text.trim_end()),
            ..Default::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.to.trim().is_empty()
            && self.cc.trim().is_empty()
            && self.subject.trim().is_empty()
            && self.text.trim().is_empty()
    }

    /// The message to send from `account`.
    pub fn build(&self, account: &Account) -> Result<lettre::Message, String> {
        let from_addr = account.username.parse().map_err(|e| format!("Your address {}: {e}", account.username))?;
        let name = (!account.name.trim().is_empty()).then(|| account.name.trim().to_string());
        let mut builder = lettre::Message::builder().from(Mailbox::new(name, from_addr)).subject(self.subject.trim());
        let to = mailboxes(&self.to)?;
        if to.is_empty() {
            return Err("Add someone to send it to".into());
        }
        for m in to {
            builder = builder.to(m);
        }
        for m in mailboxes(&self.cc)? {
            builder = builder.cc(m);
        }
        if let Some(id) = &self.in_reply_to {
            builder = builder.in_reply_to(format!("<{id}>"));
        }
        if !self.references.is_empty() {
            builder =
                builder.references(self.references.iter().map(|r| format!("<{r}>")).collect::<Vec<_>>().join(" "));
        }
        builder.header(ContentType::TEXT_PLAIN).body(self.text.clone()).map_err(|e| e.to_string())
    }
}

/// The addresses in a To or Cc field, split on commas outside quotes.
fn mailboxes(field: &str) -> Result<Vec<Mailbox>, String> {
    let mut parts = Vec::new();
    let (mut current, mut quoted) = (String::new(), false);
    for c in field.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                current.push(c);
            }
            ',' | ';' if !quoted => parts.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    parts.push(current);
    parts
        .iter()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .map(|p| p.parse::<Mailbox>().map_err(|_| format!("Not an email address: {p}")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(name: &str, address: &str) -> Addr {
        Addr { name: (!name.is_empty()).then(|| name.to_string()), address: address.into() }
    }

    fn original() -> Body {
        Body {
            from: vec![addr("Ada", "ada@example.com")],
            to: vec![addr("", "me@proton.me"), addr("Charles", "charles@example.com")],
            cc: vec![addr("", "ada@example.com"), addr("", "grace@example.com")],
            subject: "Engines".into(),
            date: 0,
            message_id: Some("m2@example.com".into()),
            references: vec!["m1@example.com".into()],
            text: "Line one\n\nLine two\n".into(),
            ..Default::default()
        }
    }

    #[test]
    fn reply_goes_to_the_sender() {
        let d = Draft::reply(&original(), "me@proton.me", false);
        assert_eq!(d.to, "Ada <ada@example.com>");
        assert_eq!(d.cc, "");
        assert_eq!(d.subject, "Re: Engines");
        assert!(d.text.contains("> Line one\n>\n> Line two"), "{}", d.text);
        assert_eq!(d.in_reply_to.as_deref(), Some("m2@example.com"));
        assert_eq!(d.references, vec!["m1@example.com", "m2@example.com"]);
    }

    #[test]
    fn reply_all_skips_me_and_repeats() {
        let d = Draft::reply(&original(), "ME@proton.me", true);
        assert_eq!(d.to, "Ada <ada@example.com>");
        assert_eq!(d.cc, "Charles <charles@example.com>, grace@example.com");
    }

    #[test]
    fn reply_honours_reply_to_and_keeps_re() {
        let mut o = original();
        o.reply_to = vec![addr("", "list@example.com")];
        o.subject = "RE: Engines".into();
        let d = Draft::reply(&o, "me@proton.me", false);
        assert_eq!(d.to, "list@example.com");
        assert_eq!(d.subject, "RE: Engines");
    }

    #[test]
    fn forward_has_the_original() {
        let d = Draft::forward(&original());
        assert_eq!(d.subject, "Fwd: Engines");
        assert!(d.to.is_empty() && d.in_reply_to.is_none());
        assert!(d.text.contains("From: Ada <ada@example.com>") && d.text.contains("Line two"));
    }

    #[test]
    fn builds_a_message() {
        let account = Account { username: "me@proton.me".into(), name: "Me".into(), ..Default::default() };
        let mut d = Draft::reply(&original(), "me@proton.me", true);
        d.to = "\"Lovelace, Ada\" <ada@example.com>; bob@example.com".into();
        let m = d.build(&account).unwrap();
        let text = String::from_utf8(m.formatted()).unwrap();
        assert!(text.contains("From: Me <me@proton.me>"), "{text}");
        // The quoted name keeps its comma (lettre encodes it), and the semicolon splits.
        assert!(text.contains("To: =?utf-8?b?TG92ZWxhY2UsIEFkYQ==?= <ada@example.com>, bob@example.com"), "{text}");
        assert!(text.contains("In-Reply-To: <m2@example.com>"), "{text}");
        assert!(text.contains("References: <m1@example.com> <m2@example.com>"), "{text}");
    }

    #[test]
    fn rejects_bad_or_missing_addresses() {
        let account = Account { username: "me@proton.me".into(), ..Default::default() };
        let d = Draft { to: "not an address".into(), ..Default::default() };
        assert!(d.build(&account).unwrap_err().contains("not an address"));
        assert!(Draft::default().build(&account).is_err());
    }
}
