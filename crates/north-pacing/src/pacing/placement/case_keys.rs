use super::{config, document};
use toml_edit::{DocumentMut, Item, Value};

/// An orchestration document holding every key `gate.rs` reads while it decides a launch.
const EVERY_KEY: &str = r#"
[placement]
order = ["gex45"]
public_repos = ["north"]
private_repos = ["south"]
cloud_credit = "anthropic"

[placement.surfaces]
north = "rust"

[hosts.gex45]
ssh = "tom@gex45"
agent_ssh = "agent@gex45"
serves = ["rust"]
pushes = ["north"]
reaches = ["laptop"]
resident_cap = 24
launches = true

[accounts.claude]
reserve = ["api:ops"]
api_beside_plans = true

[accounts.claude.plans]
a = "max-5x"

[accounts.claude.api_hold]
since = "2026-11-01"
reason = "a spend review"
"#;

/// Every key `decide()` reads from the config, whose loss changes the decision it replays.
const DECIDED: &[&str] = &[
    "placement.order",
    "placement.public_repos",
    "placement.private_repos",
    "placement.cloud_credit",
    "placement.surfaces.north",
    "hosts.gex45.ssh",
    "hosts.gex45.agent_ssh",
    "hosts.gex45.serves",
    "hosts.gex45.pushes",
    "hosts.gex45.reaches",
    "hosts.gex45.resident_cap",
    "hosts.gex45.launches",
    "accounts.claude.plans.a",
    "accounts.claude.api_hold.since",
    "accounts.claude.api_hold.reason",
    "accounts.claude.reserve",
    "accounts.claude.api_beside_plans",
];

fn at<'a>(document: &'a DocumentMut, path: &str) -> Option<&'a Item> {
    let mut parts = path.split('.');
    let mut item = document.get(parts.next()?)?;
    for part in parts {
        item = item.get(part)?;
    }
    Some(item)
}

/// The leaf's value as text, since toml_edit without its display feature cannot print an item.
fn leaf(item: &Item) -> Option<String> {
    match item.as_value()? {
        Value::String(text) => Some(format!("string:{}", text.value())),
        Value::Integer(number) => Some(format!("integer:{}", number.value())),
        Value::Boolean(flag) => Some(format!("boolean:{}", flag.value())),
        Value::Array(items) => {
            let words: Option<Vec<&str>> = items.iter().map(|item| item.as_str()).collect();
            Some(format!("array:{:?}", words?))
        }
        _ => None,
    }
}

/// Every key `decide()` reads that `document()` does not give back from the recorded config.
pub(super) fn decided_keys_lost() -> Vec<&'static str> {
    let source: DocumentMut = EVERY_KEY.parse().unwrap();
    let recorded = config(&source, "sha256".to_owned());
    let rebuilt = document(&recorded);

    let shown = |document: &DocumentMut, path: &str| at(document, path).and_then(leaf);
    DECIDED.iter().copied().filter(|path| shown(&rebuilt, path) != shown(&source, path)).collect()
}
