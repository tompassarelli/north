use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{NorthError, NorthResult};
use crate::foundation::clock::Timestamp;

const TEMPLATE: &str = include_str!("prompt.md");
const SEED_ORDERS: &str = include_str!("standing-orders.md");
const DEEPEN: &str = include_str!("deepen.md");
const ORDERS_SLOT: &str = "{standing_orders}";
const CAP_SLOT: &str = "{foreground_cap_secs}";
const SESSION_SLOT: &str = "{session}";
const ORDERS_FILE: &str = "standing-orders.md";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PromptMode {
    #[default]
    North,
    Vanilla,
}

impl PromptMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "north" => Some(PromptMode::North),
            "vanilla" => Some(PromptMode::Vanilla),
            _ => None,
        }
    }
}

pub struct StandingOrders {
    path: PathBuf,
}

impl StandingOrders {
    pub fn in_state(state: &Path) -> Self {
        Self { path: state.join(ORDERS_FILE) }
    }

    pub fn read(&self) -> NorthResult<String> {
        if !self.path.exists() {
            if let Some(parent) = self.path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&self.path, SEED_ORDERS)?;
        }
        Ok(fs::read_to_string(&self.path)?)
    }

    pub fn add(&self, text: &str) -> NorthResult<String> {
        let order = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if order.is_empty() {
            return Err(NorthError::Usage("a standing order needs text".into()));
        }
        let current = self.read()?;
        let line = format!("- {order} ({})", Timestamp::now().iso_date());
        let separator = if current.is_empty() || current.ends_with('\n') { "" } else { "\n" };
        OpenOptions::new().append(true).open(&self.path)?.write_all(format!("{separator}{line}\n").as_bytes())?;
        Ok(line)
    }
}

#[derive(Clone, Copy)]
pub struct PromptInputs<'a> {
    pub session: &'a str,
    pub mode: PromptMode,
    pub orders: &'a str,
    pub foreground_cap_secs: u64,
    pub extra: Option<&'a str>,
}

pub fn render(inputs: &PromptInputs) -> String {
    let fleet = match inputs.mode {
        PromptMode::North => Some(
            TEMPLATE
                .replace(CAP_SLOT, &inputs.foreground_cap_secs.to_string())
                .replace(SESSION_SLOT, inputs.session)
                .replace(ORDERS_SLOT, inputs.orders.trim_end()),
        ),
        PromptMode::Vanilla => None,
    };
    [fleet.as_deref(), inputs.extra]
        .into_iter()
        .flatten()
        .map(str::trim_end)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub fn prompt_sha(text: &str) -> String {
    Sha256::digest(text.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn variant_sha(inputs: &PromptInputs) -> String {
    prompt_sha(&render(&PromptInputs { session: SESSION_SLOT, ..*inputs }))
}

pub fn deepened(draft: &str) -> String {
    format!("{DEEPEN}{draft}")
}

pub fn is_deepened(prompt: &str) -> bool {
    prompt.contains(DEEPEN.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn north_prompt_carries_the_cap_every_standing_order_and_the_extra_file() {
        let orders = "- first order (2026-10-15)\n- second order (2026-10-16)\n";
        let inputs = PromptInputs { session: "worker", mode: PromptMode::North, orders, foreground_cap_secs: 45, extra: Some("project rules\n") };
        let north = render(&inputs);
        assert!(north.contains("longer than 45 seconds"));
        assert!(north.contains(orders.trim_end()));
        assert!(north.ends_with("\n\nproject rules"));
        assert!(!north.contains('{'), "an unfilled slot reached the session: {north}");
        let vanilla = render(&PromptInputs { mode: PromptMode::Vanilla, ..inputs });
        assert_eq!(vanilla, "project rules");
        assert_ne!(prompt_sha(&north), prompt_sha(&vanilla));
    }

    #[test]
    fn questions_to_tom_name_the_session_gate_and_one_variant_spans_every_session() {
        let inputs = PromptInputs { session: "worker", mode: PromptMode::North, orders: "", foreground_cap_secs: 30, extra: None };
        let north = render(&inputs);
        let questions = north.split("Questions to Tom").nth(1).unwrap().split("Standing orders").next().unwrap();
        assert!(questions.contains("north gate open --session worker "));
        assert_eq!(variant_sha(&inputs), variant_sha(&PromptInputs { session: "other", ..inputs }));
        assert_ne!(variant_sha(&inputs), variant_sha(&PromptInputs { foreground_cap_secs: 31, ..inputs }));
    }

    #[test]
    fn an_added_order_is_appended_after_the_seed() {
        let state = tempfile::tempdir().unwrap();
        let orders = StandingOrders::in_state(state.path());
        let line = orders.add("  keep   it short ").unwrap();
        let text = orders.read().unwrap();
        assert!(text.starts_with(SEED_ORDERS));
        assert!(line.starts_with("- keep it short ("));
        assert_eq!(text.lines().last(), Some(line.as_str()));
    }

    #[test]
    fn deepen_prepends_the_questions_and_keeps_the_draft_verbatim() {
        let draft = "fix the parser\n  exactly as said";
        let sent = deepened(draft);
        assert!(sent.ends_with(draft));
        assert!(is_deepened(&sent));
        assert!(is_deepened(&format!("[urgent]\n{sent}")));
        assert!(!is_deepened(draft));
    }
}
