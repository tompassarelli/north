use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{NorthError, NorthResult};
use crate::foundation::clock::Timestamp;

use super::model::Role;

const TEMPLATE: &str = include_str!("prompt.md");
const SEED_ORDERS: &str = include_str!("standing-orders.md");
const DEEPEN: &str = include_str!("deepen.md");
const ORDERS_SLOT: &str = "{standing_orders}";
const CAP_SLOT: &str = "{foreground_cap_secs}";
const SESSION_SLOT: &str = "{session}";
const REACH_SLOT: &str = "{reach}";
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
    pub session: &'a str,
    pub role: Role,
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
                .replace(REACH_SLOT, reach(inputs.role))
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

