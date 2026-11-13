//! The files the launcher finds as you type (ADR-0137): the folders Settings'
//! Search page turns on, searched with fd and ripgrep on a thread of their
//! own, a moment after the typing stops, so keys never wait on the disk. Only
//! the newest question is answered.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use patina_search::Config;
use patina_search::engine::{self, Hit, Tools};

/// How long typing must pause before the search starts.
const PAUSE: Duration = Duration::from_millis(150);

/// Where answers go: the question's number and the files found.
type Notify = Arc<dyn Fn(u64, Vec<Hit>) -> bool + Send + Sync>;

pub struct Finder {
    config: Arc<Config>,
    tools: Arc<Tools>,
    /// The number of the newest question.
    newest: Arc<AtomicU64>,
    notify: Notify,
    /// The files found for the newest question that's been answered.
    pub hits: Vec<Hit>,
}

impl Finder {
    pub fn new(
        config: Config,
        tools: Tools,
        notify: impl Fn(u64, Vec<Hit>) -> bool + Send + Sync + 'static,
    ) -> Self {
        Finder {
            config: Arc::new(config),
            tools: Arc::new(tools),
            newest: Arc::new(AtomicU64::new(0)),
            notify: Arc::new(notify),
            hits: Vec::new(),
        }
    }

    /// Whether there's anything to search with: it's on, and fd or ripgrep
    /// is installed.
    pub fn usable(&self) -> bool {
        self.config.enabled && (self.tools.fd.is_some() || self.tools.rg.is_some())
    }

    /// Looks for `query` once the typing pauses; a query too short for it
    /// clears what was found.
    pub fn ask(&mut self, query: &str) {
        let number = self.newest.fetch_add(1, Ordering::SeqCst) + 1;
        if !self.usable() || query.trim().chars().count() < engine::LEAST {
            self.hits.clear();
            return;
        }
        let (query, config, tools) = (query.to_string(), self.config.clone(), self.tools.clone());
        let (newest, notify) = (self.newest.clone(), self.notify.clone());
        std::thread::spawn(move || {
            std::thread::sleep(PAUSE);
            if newest.load(Ordering::SeqCst) != number {
                return;
            }
            let hits = engine::search(&query, &config, &tools);
            // Typed on while it ran: stale.
            if newest.load(Ordering::SeqCst) == number {
                notify(number, hits);
            }
        });
    }

    /// An answer: kept if it's for the newest question.
    pub fn found(&mut self, number: u64, hits: Vec<Hit>) {
        if number == self.newest.load(Ordering::SeqCst) {
            self.hits = hits;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn finder(config: Config, tools: Tools) -> (Finder, mpsc::Receiver<(u64, Vec<Hit>)>) {
        let (send, answers) = mpsc::channel();
        let send = std::sync::Mutex::new(send);
        let finder = Finder::new(config, tools, move |n, hits| {
            send.lock().unwrap().send((n, hits)).is_ok()
        });
        (finder, answers)
    }

    #[test]
    fn nothing_is_searched_with_it_off_or_no_tools() {
        let (mut off, answers) = finder(
            Config {
                enabled: false,
                ..Config::default()
            },
            Tools {
                fd: Some("/bin/true".into()),
                rg: None,
            },
        );
        assert!(!off.usable());
        off.ask("report");
        assert!(answers.recv_timeout(Duration::from_millis(400)).is_err());
        let (none, _) = finder(Config::default(), Tools::default());
        assert!(!none.usable());
    }

    #[test]
    fn a_short_query_clears_and_only_the_newest_is_kept() {
        let (mut f, _) = finder(
            Config::default(),
            Tools {
                fd: Some("/bin/true".into()),
                rg: None,
            },
        );
        f.hits = vec![];
        f.ask("report");
        let first = f.newest.load(Ordering::SeqCst);
        f.ask("reports");
        f.found(
            first,
            vec![Hit {
                path: "/old".into(),
                name: "old".into(),
                dir: String::new(),
                line: None,
            }],
        );
        assert!(
            f.hits.is_empty(),
            "an answer to an older question is dropped"
        );
        f.ask("r");
        assert!(f.hits.is_empty());
    }
}
