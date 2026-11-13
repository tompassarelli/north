//! `north config operator init DIR`: writes a working operator repository of synthetic values, with the catalog generated from north's `structure.toml`.
use std::fs;
use std::path::Path;

use north_core::agent_catalog::initial_catalog;

use crate::error::{NorthError, NorthResult};
use crate::platform::harness;

pub const USAGE: &str = "usage: north config operator init DIR";

const TEMPLATE: &[(&str, &str)] = &[
    ("agents/orchestration.toml", include_str!("template/agents/orchestration.toml")),
    ("agents/costs.toml", include_str!("template/agents/costs.toml")),
    ("agents/claude-accounts.toml", include_str!("template/agents/claude-accounts.toml")),
    ("agents/credentials.toml", include_str!("template/agents/credentials.toml")),
    ("agents/frozen-topics.toml", include_str!("template/agents/frozen-topics.toml")),
    ("agents/operating-point.toml", include_str!("template/agents/operating-point.toml")),
    ("agents/watch.toml", include_str!("template/agents/watch.toml")),
    ("agents/rules.toml", include_str!("template/agents/rules.toml")),
    ("agents/incidents.jsonl", include_str!("template/agents/incidents.jsonl")),
    ("infra/manifest.toml", include_str!("template/infra/manifest.toml")),
    ("privacy/private-kinds.toml", include_str!("template/privacy/private-kinds.toml")),
    ("privacy/public-denylist.toml", include_str!("template/privacy/public-denylist.toml")),
    ("secrets/NOTE.md", include_str!("template/secrets/NOTE.md")),
    (".north-operator-schema", include_str!("template/schema-version")),
    (".safe-push", include_str!("template/safe-push")),
];

pub fn run(arguments: &[String]) -> NorthResult<()> {
    let [directory] = arguments else {
        return Err(NorthError::Usage(USAGE.into()));
    };
    let directory = Path::new(directory);
    if directory.exists() && (!directory.is_dir() || fs::read_dir(directory)?.next().is_some()) {
        return Err(NorthError::Configuration(format!("{} exists and is not empty", directory.display())));
    }
    let north = harness::harness_root().map_err(NorthError::Configuration)?;
    let structure = fs::read_to_string(north.join("structure.toml"))?;
    let catalog = serde_json::to_string_pretty(&initial_catalog(&structure)?)? + "\n";
    for (relative, content) in TEMPLATE.iter().copied().chain([("agents/catalog-config.json", catalog.as_str())]) {
        let path = directory.join(relative);
        fs::create_dir_all(path.parent().unwrap_or(directory))?;
        fs::write(&path, content)?;
    }
    println!("wrote {} files under {}", TEMPLATE.len() + 1, directory.display());
    Ok(())
}
