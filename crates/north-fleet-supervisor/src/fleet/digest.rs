use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use toml_edit::DocumentMut;

use super::model::Delivery;
use super::registry::Fleet;
use super::sender::Sender;

const INTERVAL_KEY: &str = "fleet_digest_minutes";
const FILE: &str = ".retirement-digests.json";

static DEFAULTS: LazyLock<&DocumentMut> = LazyLock::new(|| north_base::platform::harness::document(north_base::platform::harness::DEFAULTS));

pub fn interval() -> Duration {
    let minutes = DEFAULTS[INTERVAL_KEY]["value"].as_integer().and_then(|minutes| u64::try_from(minutes).ok()).unwrap_or_else(|| panic!("harness/defaults.toml has no whole minutes for {INTERVAL_KEY}"));
    Duration::from_secs(minutes * 60)
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct Parent {
    sent_ms: Option<u64>,
    lines: Vec<String>,
    held_through_ms: Option<u64>,
    held_sent_ms: Option<u64>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct Book(BTreeMap<String, Parent>);

impl Fleet {
    fn digest_file(&self) -> PathBuf {
        self.root.join(FILE)
    }

    fn digest_book(&self) -> Book {
        std::fs::read(self.digest_file()).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
    }

    fn keep_digest_book(&self, book: &Book) -> std::io::Result<()> {
        let path = self.digest_file();
        let staged = path.with_extension(format!("json.{}", std::process::id()));
        std::fs::write(&staged, serde_json::to_vec(book)?)?;
        std::fs::rename(staged, path)
    }

    /// Adds each parent's new lane lines to what is pending for it, then sends every parent whose half hour since its last digest has passed one message.
    pub fn tell_retirement_digests(&self, fresh: BTreeMap<String, Vec<String>>, now_ms: u64) {
        let window = u64::try_from(interval().as_millis()).unwrap_or(u64::MAX);
        let mut book = self.digest_book();
        for (parent, lines) in fresh {
            let pending = &mut book.0.entry(parent).or_default().lines;
            for line in lines {
                if !pending.contains(&line) {
                    pending.push(line);
                }
            }
        }
        for (parent, pending) in &mut book.0 {
            let open = pending.sent_ms.is_none_or(|sent| now_ms.saturating_sub(sent) >= window);
            if pending.lines.is_empty() || !open {
                continue;
            }
            let text = format!("[north: {} sessions' lanes hold commits not on origin/main or could not be read; land or restaff each]{}", pending.lines.len(), pending.lines.concat());
            match self.send_signed(&Sender::fleet(), parent, &text, Delivery::Queued) {
                Ok(_) => {
                    pending.sent_ms = Some(now_ms);
                    pending.lines.clear();
                }
                Err(error) => eprintln!("fleet reaper: {parent} was not told of {} unlanded lanes: {}", pending.lines.len(), error.user_message()),
            }
        }
        book.0.retain(|_, pending| !pending.lines.is_empty() || pending.held_through_ms.is_some() || pending.sent_ms.is_some_and(|sent| now_ms.saturating_sub(sent) < window));
        if let Err(error) = self.keep_digest_book(&book) {
            eprintln!("fleet reaper: the pending retirement digests were not kept: {error}");
        }
    }

    pub fn tell_held_digests(&self, now_ms: u64) {
        let window = u64::try_from(interval().as_millis()).unwrap_or(u64::MAX);
        let mut book = self.digest_book();
        let mut by_recipient: BTreeMap<String, Vec<crate::fleet::held::Held>> = BTreeMap::new();
        for row in self.held_since(0) {
            if book.0.get(&row.to).and_then(|parent| parent.held_through_ms).is_none_or(|at| row.at_ms > at) {
                by_recipient.entry(row.to.clone()).or_default().push(row);
            }
        }
        for (recipient, rows) in by_recipient {
            let parent = book.0.entry(recipient.clone()).or_default();
            if parent.held_sent_ms.is_some_and(|sent| now_ms.saturating_sub(sent) < window) {
                continue;
            }
            let texts: Vec<String> = rows.iter().map(|row| row.text.chars().take(300).collect()).collect();
            let causes = crate::fleet::alike::causes(&texts);
            let lines: String = causes.iter().map(|cause| format!("\n- {cause}")).collect();
            let text = format!("[north: {} held messages of {} causes since the last digest]{lines}", rows.len(), causes.len());
            match self.send_signed(&Sender::fleet(), &recipient, &text, Delivery::Queued) {
                Ok(_) => {
                    parent.held_through_ms = rows.iter().map(|row| row.at_ms).max();
                    parent.held_sent_ms = Some(now_ms);
                }
                Err(error) => eprintln!("fleet reaper: {recipient} was not sent its {} held messages: {}", rows.len(), error.user_message()),
            }
        }
        if let Err(error) = self.keep_digest_book(&book) {
            eprintln!("fleet reaper: the held digests were not kept: {error}");
        }
    }
}
