use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{NorthError, NorthResult};
use crate::foundation::clock::Timestamp;

use super::model::Role;
use super::runtime::HAND_IN_CAP_SECS;

const TEMPLATE: &str = include_str!("prompt.md");
pub const TEMPLATE_PATH: &str = "crates/north-fleet/src/fleet/prompt.md";
const SEED_ORDERS: &str = include_str!("standing-orders.md");
const DEEPEN: &str = include_str!("deepen.md");
const ORDERS_SLOT: &str = "{standing_orders}";
const CAP_SLOT: &str = "{foreground_cap_secs}";
const HAND_IN_SLOT: &str = "{hand_in_timeout_ms}";
const REACH_SLOT: &str = "{reach}";
const REPORT_SLOT: &str = "{report_lead}";
const MEMBER_REPORT_LEAD: &str = " Begin it `Done:`, `Not done:`, `Split:` or `Blocked:`, since the ledger scores your run by that word, and end a Not done or Blocked report with one `Stop:` line: `Stop: decision`, `Stop: wait until YYYY-MM-DDTHH:MMZ` when it cannot go on before that UTC time, else `Stop: landing`, `capacity`, `client`, `leash` or `reasoning`. When only another host or need (laptop, native) can finish the box, run `north fleet need NAME REASON` first, which records it for dispatch to place the next try; write no `Stop: host` line.";
const PROXY_REACH: &str = "As Tom's proxy you hear Tom directly, typed in the fleet TUI; every message from anyone else starts with `[from NAME]`.";
const MEMBER_REACH: &str = "Tom and the proxy reach you only through fleet messages.";
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
    pub role: Role,
    pub mode: PromptMode,
    pub orders: &'a str,
    pub foreground_cap_secs: u64,
    pub extra: Option<&'a str>,
}

pub fn render(inputs: &PromptInputs) -> String {
    render_from(TEMPLATE, inputs)
}

/// The standing orders north ships in this repository, which a launch to a model outside the operator's own accounts carries instead of the operator's live file.
pub fn seed_orders() -> &'static str {
    SEED_ORDERS
}

pub fn template() -> &'static str {
    TEMPLATE
}

pub fn render_from(template: &str, inputs: &PromptInputs) -> String {
    let fleet = match inputs.mode {
        PromptMode::North => Some(
            template
                .replace(CAP_SLOT, &inputs.foreground_cap_secs.to_string())
                .replace(HAND_IN_SLOT, &(HAND_IN_CAP_SECS * 1000).to_string())
                .replace(REACH_SLOT, reach(inputs.role))
                .replace(REPORT_SLOT, report_lead(inputs.role))
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

fn reach(role: Role) -> &'static str {
    match role {
        Role::Proxy => PROXY_REACH,
        Role::Lead | Role::Worker => MEMBER_REACH,
    }
}

fn report_lead(role: Role) -> &'static str {
    match role {
        Role::Proxy => "",
        Role::Lead | Role::Worker => MEMBER_REPORT_LEAD,
    }
}

pub fn prompt_sha(text: &str) -> String {
    Sha256::digest(text.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn deepened(draft: &str) -> String {
    format!("{DEEPEN}{draft}")
}

pub fn is_deepened(prompt: &str) -> bool {
    prompt.contains(DEEPEN.trim_end())
}

