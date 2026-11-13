//! `north config operator init DIR`: writes a working operator repository of synthetic values, with the catalog generated from north's `structure.toml`.
use std::fs;
use std::path::Path;

use north_core::agent_catalog::initial_catalog;

use crate::error::{NorthError, NorthResult};
use crate::platform::harness;

pub const USAGE: &str = "usage: north config operator init DIR | init --check DIR";

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
    match arguments {
        [flag, directory] if flag == "--check" => check(Path::new(directory)),
        [directory] if directory != "--check" => write(Path::new(directory)),
        _ => Err(NorthError::Usage(USAGE.into())),
    }
}

fn render() -> NorthResult<Vec<(String, String)>> {
    let north = harness::harness_root().map_err(NorthError::Configuration)?;
    let structure = fs::read_to_string(north.join("structure.toml"))?;
    let catalog = serde_json::to_string_pretty(&initial_catalog(&structure)?)? + "\n";
    Ok(TEMPLATE.iter().copied().chain([("agents/catalog-config.json", catalog.as_str())]).map(|(relative, content)| (relative.to_string(), content.to_string())).collect())
}

fn write(directory: &Path) -> NorthResult<()> {
    if directory.exists() && (!directory.is_dir() || fs::read_dir(directory)?.next().is_some()) {
        return Err(NorthError::Configuration(format!("{} exists and is not empty", directory.display())));
    }
    let files = render()?;
    for (relative, content) in &files {
        let path = directory.join(relative);
        fs::create_dir_all(path.parent().unwrap_or(directory))?;
        fs::write(&path, content)?;
    }
    println!("wrote {} files under {}", files.len(), directory.display());
    Ok(())
}

fn check(directory: &Path) -> NorthResult<()> {
    let files = render()?;
    let mut findings = Vec::new();
    for (relative, content) in &files {
        match fs::read_to_string(directory.join(relative)) {
            Ok(found) if found == *content => {}
            Ok(_) => findings.push(format!("{relative}: differs from init's output")),
            Err(_) => findings.push(format!("{relative}: missing")),
        }
    }
    let mut present = Vec::new();
    list_files(directory, directory, &mut present)?;
    findings.extend(present.into_iter().filter(|path| files.iter().all(|(relative, _)| relative != path)).map(|path| format!("{path}: not in init's output")));
    if findings.is_empty() {
        println!("{} equals init's output", directory.display());
        return Ok(());
    }
    Err(NorthError::Configuration(findings.join("\n")))
}

fn list_files(root: &Path, directory: &Path, into: &mut Vec<String>) -> NorthResult<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            list_files(root, &path, into)?;
        } else if let Ok(relative) = path.strip_prefix(root) {
            into.push(relative.to_string_lossy().into_owned());
        }
    }
    Ok(())
}
