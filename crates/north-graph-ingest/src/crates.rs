use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;
use std::sync::LazyLock;

use regex::Regex;
use toml_edit::{DocumentMut, Item, TableLike};

use crate::facts::{Facts, Sourced, text};
use crate::places::Places;

const WRITER: &str = "cargo";
const REPLACED: [&str; 4] = ["root", "depends-on", "embeds", "crates-commit"];
const KINDS: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

static EMBED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"include_(?:str|bytes)!\(\s*(?:"([^"\\]*)"|concat!\(\s*env!\(\s*"CARGO_MANIFEST_DIR"\s*\)\s*,\s*"([^"\\]*)"\s*,?\s*\))\s*,?\s*\)"#).expect("the embed pattern compiles")
});

struct Member {
    name: String,
    dir: String,
    manifest: DocumentMut,
}

pub fn read(places: &Places) -> Result<Sourced, String> {
    let checkout = places.code.join("north/main");
    let rev = git(&checkout, &["rev-parse", "--verify", "origin/main^{commit}"])?.trim().to_owned();
    let show = |path: &str| git(&checkout, &["show", &format!("{rev}:{path}")]);
    let workspace = manifest("Cargo.toml", &show("Cargo.toml")?)?;
    let mut members = Vec::new();
    if let Some(name) = workspace.get("package").and_then(|package| package.get("name")).and_then(Item::as_str) {
        members.push(Member { name: name.to_owned(), dir: ".".to_owned(), manifest: workspace.clone() });
    }
    let listed = workspace.get("workspace").and_then(|table| table.get("members")).and_then(Item::as_array).ok_or("Cargo.toml has no [workspace] members")?;
    for dir in listed.iter().filter_map(|value| value.as_str()) {
        let dir = normalize(dir).ok_or_else(|| format!("member {dir} leaves the workspace"))?;
        let path = format!("{dir}/Cargo.toml");
        let manifest = manifest(&path, &show(&path)?)?;
        let name = manifest.get("package").and_then(|package| package.get("name")).and_then(Item::as_str).ok_or_else(|| format!("{path} names no package"))?.to_owned();
        members.push(Member { name, dir, manifest });
    }
    let mut facts = Facts::new(&REPLACED);
    facts.tell(WRITER, text("crates:north"), "crates-commit", text(&rev));
    let by_dir: BTreeMap<&str, &str> = members.iter().map(|member| (member.dir.as_str(), member.name.as_str())).collect();
    let shared = workspace.get("workspace").and_then(|table| table.get("dependencies")).and_then(Item::as_table_like);
    for member in &members {
        facts.tell(WRITER, text(format!("crate:{}", member.name)), "root", text(&member.dir));
        let mut depends = BTreeSet::new();
        for (name, dependency) in dependencies(&member.manifest) {
            let from_workspace = dependency.get("workspace").and_then(Item::as_bool) == Some(true);
            let (base, entry) = if from_workspace { (".", shared.and_then(|table| table.get(name))) } else { (member.dir.as_str(), Some(dependency)) };
            let Some(path) = entry.and_then(|entry| entry.get("path")).and_then(Item::as_str) else { continue };
            if let Some(dep) = normalize(&format!("{base}/{path}")).and_then(|dir| by_dir.get(dir.as_str()).copied()) {
                depends.insert(dep);
            }
        }
        for dep in depends {
            facts.tell(WRITER, text(format!("crate:{}", member.name)), "depends-on", text(format!("crate:{dep}")));
        }
    }
    let sources = git(&checkout, &["grep", "-l", "-E", r"include_(str|bytes)!", &rev, "--", "*.rs"])?;
    for source in sources.lines().filter_map(|line| line.strip_prefix(&format!("{rev}:"))) {
        let root = members.iter().map(|member| member.dir.as_str()).filter(|dir| *dir == "." || source.starts_with(&format!("{dir}/"))).max_by_key(|dir| dir.len()).unwrap_or(".");
        let here = Path::new(source).parent().and_then(Path::to_str).filter(|dir| !dir.is_empty()).unwrap_or(".");
        for embed in EMBED.captures_iter(&show(source)?) {
            let (base, path) = match (embed.get(1), embed.get(2)) {
                (Some(literal), _) => (here, literal.as_str()),
                (None, Some(rooted)) => (root, rooted.as_str().trim_start_matches('/')),
                (None, None) => continue,
            };
            if let Some(file) = (!path.starts_with('/')).then(|| normalize(&format!("{base}/{path}"))).flatten() {
                facts.tell(WRITER, text(source), "embeds", text(file));
            }
        }
    }
    Ok(facts.finish_noted())
}

fn dependencies(manifest: &DocumentMut) -> Vec<(&str, &Item)> {
    let targets = manifest.get("target").and_then(Item::as_table_like).into_iter().flat_map(TableLike::iter).map(|(_, target)| target);
    let mut tables: Vec<&Item> = KINDS.iter().filter_map(|kind| manifest.get(kind)).collect();
    tables.extend(targets.flat_map(|target| KINDS.iter().filter_map(|kind| target.get(kind))));
    tables.into_iter().filter_map(Item::as_table_like).flat_map(TableLike::iter).collect()
}

fn manifest(path: &str, body: &str) -> Result<DocumentMut, String> {
    body.parse().map_err(|why| format!("{path}: {why}"))
}

fn normalize(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(if parts.is_empty() { ".".to_owned() } else { parts.join("/") })
}

fn git(checkout: &Path, arguments: &[&str]) -> Result<String, String> {
    let output = Command::new("git").arg("-C").arg(checkout).args(arguments).output().map_err(|why| format!("git: {why}"))?;
    let missed = arguments.first() == Some(&"grep") && output.status.code() == Some(1) && output.stderr.is_empty();
    if !output.status.success() && !missed {
        return Err(format!("{}: git {}: {}", checkout.display(), arguments.first().copied().unwrap_or_default(), String::from_utf8_lossy(&output.stderr).trim()));
    }
    String::from_utf8(output.stdout).map_err(|why| format!("{}: git {}: {why}", checkout.display(), arguments.first().copied().unwrap_or_default()))
}
