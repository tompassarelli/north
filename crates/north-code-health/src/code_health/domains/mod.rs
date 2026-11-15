mod islands;

use super::config::structure_of;
use super::rust_modules::Crate;
use super::snapshot::Snapshot;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use toml_edit::DocumentMut;

const USAGE: &str = "usage: north code-health domains [--repo DIR] [--rev REV]";
pub(super) const RULE: &str = "domains";
const PLAN: &str = "docs/rationale/north-ops-split.md";

pub(super) fn run(arguments: &[String]) -> u8 {
    let (mut repo, mut rev) = (PathBuf::from("."), "HEAD".to_owned());
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        match (argument.as_str(), rest.next()) {
            ("--repo", Some(dir)) => repo = PathBuf::from(dir),
            ("--rev", Some(name)) => rev.clone_from(name),
            _ => {
                eprintln!("{USAGE}");
                return 2;
            }
        }
    }
    match report(&repo, &rev) {
        Ok((lines, code)) => {
            for line in lines {
                println!("{line}");
            }
            code
        }
        Err(error) => {
            eprintln!("north code-health domains: {error}");
            2
        }
    }
}

struct Domains {
    name: String,
    src: String,
    islands: Vec<Vec<String>>,
}

struct Bound {
    max: usize,
    allowed: BTreeMap<String, String>,
}

impl Bound {
    fn parse(structure: &str) -> Result<Option<Self>, String> {
        let document: DocumentMut = structure.parse().map_err(|error: toml_edit::TomlError| format!("structure.toml: {error}"))?;
        let Some(item) = document.get("domains") else { return Ok(None) };
        let table = item.as_table_like().ok_or("structure.toml: domains must be a table")?;
        let max = table.get("max_islands").and_then(|item| item.as_integer()).and_then(|count| usize::try_from(count).ok()).ok_or("structure.toml: domains.max_islands must be a non-negative integer")?;
        let allowed = table
            .get("allow")
            .and_then(|item| item.as_array_of_tables())
            .into_iter()
            .flatten()
            .map(|entry| {
                let name = entry.get("crate").and_then(|item| item.as_str()).ok_or_else(|| "structure.toml: domains.allow needs a crate".to_owned())?;
                let reason = entry.get("reason").and_then(|item| item.as_str()).ok_or_else(|| "structure.toml: domains.allow needs a reason".to_owned())?;
                Ok((name.replace('-', "_"), reason.to_owned()))
            })
            .collect::<Result<BTreeMap<String, String>, String>>()?;
        Ok(Some(Self { max, allowed }))
    }

    fn over(&self, krate: &Domains) -> bool {
        krate.islands.len() > self.max
    }

    fn refuses(&self, krate: &Domains) -> bool {
        self.over(krate) && !self.allowed.contains_key(&krate.name.replace('-', "_"))
    }
}

fn report(repo: &Path, rev: &str) -> Result<(Vec<String>, u8), String> {
    let mut snapshot = Snapshot::new(repo, rev)?;
    let Some(structure) = structure_of(&snapshot) else {
        return Ok((vec!["domains: no structure.toml at rev; nothing to check".to_owned()], 0));
    };
    let Some(bound) = Bound::parse(&structure)? else {
        return Ok((vec!["domains: structure.toml has no [domains] table; nothing to check".to_owned()], 0));
    };
    let measured = measure(&mut snapshot)?;
    let mut lines = Vec::new();
    for krate in measured.iter().filter(|krate| bound.over(krate)) {
        let name = krate.name.replace('_', "-");
        match bound.allowed.get(&krate.name) {
            Some(reason) => lines.push(format!("domains: {name} spans {} islands, over its bound of {}; allowed: {reason}", krate.islands.len(), bound.max)),
            None => lines.push(refusal(krate, bound.max)),
        }
    }
    let refused = measured.iter().filter(|krate| bound.refuses(krate)).count();
    let summary = if refused == 0 { "domains: no crate spans more unrelated domains than its bound".to_owned() } else { format!("✗ domains: {refused} crate(s) span more unrelated domains than their bound") };
    lines.push(summary);
    Ok((lines, u8::from(refused > 0)))
}

pub(super) fn refusals(snapshot: &mut Snapshot, structure: &str, changed: &[String]) -> Result<Vec<(String, String)>, String> {
    let Some(bound) = Bound::parse(structure)? else { return Ok(Vec::new()) };
    let measured = measure(snapshot)?;
    let mut out = Vec::new();
    for krate in measured.iter().filter(|krate| bound.refuses(krate)) {
        let text = refusal(krate, bound.max);
        out.extend(changed.iter().filter(|path| path.strip_prefix(krate.src.as_str()).is_some_and(|rest| rest.starts_with('/'))).map(|path| (path.clone(), text.clone())));
    }
    Ok(out)
}

fn refusal(krate: &Domains, max: usize) -> String {
    let islands: Vec<String> = krate.islands.iter().map(|island| island.join("+")).collect();
    format!(
        "✗ domains: {} spans {} unrelated domains, over its bound of {max} ({}); split it into one crate per domain (plan {PLAN}), or record why in a [[domains.allow]] entry of structure.toml",
        krate.name.replace('_', "-"),
        krate.islands.len(),
        islands.join(" | ")
    )
}

fn measure(snapshot: &mut Snapshot) -> Result<Vec<Domains>, String> {
    let manifests: Vec<String> = snapshot.paths().iter().filter(|path| path.as_str() == "Cargo.toml" || path.ends_with("/Cargo.toml")).cloned().collect();
    snapshot.load(&manifests)?;
    let texts: Vec<(String, String)> = manifests.into_iter().map(|path| {
        let text = snapshot.text(&path).map(|text| text.to_string()).unwrap_or_default();
        (path, text)
    }).collect();
    let crates = Crate::from_manifests(texts.iter().map(|(path, text)| (path.as_str(), text.as_str())));
    let workspace: BTreeSet<String> = crates.iter().map(|krate| krate.name.clone()).collect();
    let srcs: Vec<String> = crates.iter().map(Crate::src).collect();
    let sources: Vec<String> = snapshot.paths().iter().filter(|path| path.ends_with(".rs")).cloned().collect();
    snapshot.load(&sources)?;
    let mut owned: Vec<Vec<(String, Rc<str>)>> = crates.iter().map(|_| Vec::new()).collect();
    for path in sources {
        let owner = srcs.iter().enumerate().filter_map(|(at, src)| path.strip_prefix(src.as_str()).and_then(|rest| rest.strip_prefix('/')).map(|rest| (at, src.len(), rest.to_owned()))).max_by_key(|(_, len, _)| *len);
        if let (Some((at, _, rest)), Some(text)) = (owner, snapshot.text(&path)) {
            owned[at].push((rest, text));
        }
    }
    Ok(crates
        .iter()
        .zip(&owned)
        .map(|(krate, files)| {
            let borrowed: Vec<(&str, &str)> = files.iter().map(|(path, text)| (path.as_str(), &**text)).collect();
            Domains { name: krate.name.clone(), src: krate.src(), islands: islands::islands(&borrowed, &workspace, &krate.name) }
        })
        .collect())
}
