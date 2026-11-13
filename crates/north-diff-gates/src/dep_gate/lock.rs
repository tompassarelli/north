use super::{Identity, LockCandidate, Parsed, Urgent, get, matches, optional_table, parse_json, parse_toml, required, string, table};
use regex::Regex;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const COOLDOWN: f64 = 604800.0;

fn jsonc(text: &str) -> Parsed<Value> {
    let re = Regex::new(r#""(?:[^"\\]|\\.)*"|//[^\n]*|/\*[\s\S]*?\*/"#).unwrap();
    let cleaned = re.replace_all(text, |c: &regex::Captures<'_>| {
        if c[0].starts_with('"') {
            c[0].to_owned()
        } else {
            " ".into()
        }
    });
    let re = Regex::new(r#""(?:[^"\\]|\\.)*"|,\s*([}\]])"#).unwrap();
    let cleaned = re.replace_all(&cleaned, |c: &regex::Captures<'_>| {
        if c[0].starts_with('"') {
            c[0].to_owned()
        } else {
            c[1].to_owned()
        }
    });
    let data = parse_json(&cleaned)?;
    table(&data)?;
    Ok(data)
}

pub(crate) fn locked_versions(path: &str, text: &str) -> Parsed<BTreeSet<(String, String)>> {
    let mut result = BTreeSet::new();
    if Path::new(path)
        .file_name()
        .is_some_and(|p| p == "Cargo.lock")
    {
        let data = parse_toml(text)?;
        if let Some(packages) = data.get("package") {
            for package in packages.as_array().ok_or("ValueError")? {
                table(package)?;
                if string(
                    package
                        .get("source")
                        .unwrap_or(&Value::String("local".into())),
                )?
                .starts_with("registry+")
                {
                    result.insert((
                        string(required(package, "name")?)?.to_owned(),
                        string(required(package, "version")?)?.to_owned(),
                    ));
                }
            }
        }
    } else {
        let data = jsonc(text)?;
        let re = Regex::new(r"\A((?:@[^/@\s]+/)?[^/@\s]+)@(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?)\z").unwrap();
        for (_, entry) in optional_table(&data, "packages")?.into_iter().flatten() {
            let entry = entry
                .as_array()
                .filter(|a| !a.is_empty())
                .ok_or("ValueError")?;
            let resolution = string(&entry[0])?;
            if let Some(c) = re.captures(resolution) {
                result.insert((c[1].to_owned(), c[2].to_owned()));
            }
        }
    }
    Ok(result)
}

fn locked_nodes(text: &str) -> Parsed<BTreeMap<String, Value>> {
    let data = parse_json(text)?;
    table(&data)?;
    let mut result = BTreeMap::new();
    for (name, node) in optional_table(&data, "nodes")?.into_iter().flatten() {
        table(node)?;
        if let Some(locked) = node.get("locked") {
            table(locked)?;
            result.insert(name.clone(), locked.clone());
        }
    }
    Ok(result)
}

fn first_party_flake(locked: &Value) -> bool {
    let kind = get(locked, "type").as_str().unwrap_or("");
    if kind == "github" {
        return get(locked, "owner")
            .as_str()
            .is_some_and(|s| s.eq_ignore_ascii_case("tompassarelli"));
    }
    let location = get(
        locked,
        if kind == "git" {
            "url"
        } else if kind == "path" {
            "path"
        } else {
            ""
        },
    )
    .as_str()
    .unwrap_or("");
    matches(
        r"(?i)\A(?:(?:git\+)?https://github\.com/tompassarelli/|(?:git\+)?ssh://git@github\.com/tompassarelli/|(?:git\+)?file:///home/[^/]+/code/|/home/[^/]+/code/)",
        location,
    )
}

fn github_owner(location: &str) -> Option<String> {
    let path = [
        "git+https://github.com/",
        "git+ssh://git@github.com/",
        "https://github.com/",
        "http://github.com/",
        "ssh://git@github.com/",
        "git@github.com:",
    ]
    .iter()
    .find_map(|prefix| location.strip_prefix(prefix))?;
    path.split('/').next().filter(|owner| !owner.is_empty()).map(str::to_ascii_lowercase)
}

fn flake_owner(locked: &Value) -> Option<String> {
    match get(locked, "type").as_str() {
        Some("github") => get(locked, "owner").as_str().map(str::to_ascii_lowercase),
        Some("git") => get(locked, "url").as_str().and_then(github_owner),
        _ => None,
    }
}

pub(super) fn flake_owners(settings: &str) -> BTreeSet<String> {
    let mut owners = BTreeSet::new();
    let mut section = "";
    for line in settings.lines().map(str::trim) {
        if let Some(header) = line.strip_prefix('[') {
            section = header.split(']').next().unwrap_or("").trim();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        if section == "flake" && key.trim() == "owner" {
            let owner = value.trim().trim_matches('"').trim();
            if !owner.is_empty() {
                owners.insert(owner.to_ascii_lowercase());
            }
        }
    }
    owners
}

pub(super) fn owner_violations(candidates: &[LockCandidate], owners: &BTreeSet<String>) -> Vec<String> {
    if owners.is_empty() {
        return Vec::new();
    }
    candidates
        .iter()
        .filter(|candidate| candidate.ecosystem == "flake")
        .filter_map(|candidate| {
            let owner = candidate.owner.as_ref()?;
            (!owners.contains(owner)).then(|| format!("dep-gate: {}: {}: owner {owner} is not in the .safe-push flake allowlist", candidate.path, candidate.name))
        })
        .collect()
}

fn input_node<'a>(nodes: &'a Value, root: &'a str, input: &'a Value, depth: usize) -> Option<&'a str> {
    if let Some(name) = input.as_str() {
        return Some(name);
    }
    let mut node = root;
    for step in input.as_array()? {
        let next = get(get(get(nodes, node), "inputs"), step.as_str()?);
        node = input_node(nodes, root, next, depth.checked_sub(1)?)?;
    }
    Some(node)
}

fn channel_nixpkgs(text: &str) -> Parsed<Option<String>> {
    let data = parse_json(text)?;
    let nodes = get(&data, "nodes");
    let Some(name) = get(&data, "root").as_str().and_then(|root| {
        input_node(nodes, root, get(get(get(nodes, root), "inputs"), "nixpkgs"), 16)
    }) else {
        return Ok(None);
    };
    let node = get(nodes, name);
    let official = |key: &str| {
        let source = get(node, key);
        get(source, "type").as_str() == Some("github")
            && get(source, "owner").as_str().is_some_and(|s| s.eq_ignore_ascii_case("nixos"))
            && get(source, "repo").as_str() == Some("nixpkgs")
    };
    let channel = get(get(node, "original"), "ref")
        .as_str()
        .is_some_and(|r| r.starts_with("nixos-"));
    Ok((official("locked") && official("original") && channel).then(|| name.to_owned()))
}

pub(super) fn lock_candidates(
    before: &BTreeMap<String, String>,
    after: &BTreeMap<String, String>,
) -> Result<Vec<LockCandidate>, String> {
    let mut result = Vec::new();
    for (path, text) in after {
        if before.get(path) == Some(text) {
            continue;
        }
        let mut parse = || -> Parsed<()> {
            if Path::new(path)
                .file_name()
                .is_some_and(|p| p == "flake.lock")
            {
                let old = before
                    .get(path)
                    .map(|t| locked_nodes(t))
                    .transpose()?
                    .unwrap_or_default();
                let channel = channel_nixpkgs(text)?;
                for (name, locked) in locked_nodes(text)? {
                    if old.get(&name) != Some(&locked)
                        && !first_party_flake(&locked)
                        && channel.as_deref() != Some(name.as_str())
                    {
                        let version = ["rev", "version", "narHash"]
                            .into_iter()
                            .find_map(|k| locked.get(k))
                            .map(string)
                            .transpose()?
                            .unwrap_or("unknown");
                        result.push(LockCandidate {
                            path: path.clone(),
                            ecosystem: "flake".into(),
                            name: string(&name.into())?.to_owned(),
                            version: version.to_owned(),
                            published: get(&locked, "lastModified").as_i64().map(|t| t as f64),
                            owner: flake_owner(&locked),
                        });
                    }
                }
            } else {
                let old = before
                    .get(path)
                    .map(|t| locked_versions(path, t))
                    .transpose()?
                    .unwrap_or_default();
                for (name, version) in locked_versions(path, text)?.difference(&old) {
                    result.push(LockCandidate {
                        path: path.clone(),
                        ecosystem: if path.ends_with("Cargo.lock") {
                            "cargo"
                        } else {
                            "npm"
                        }
                        .into(),
                        name: name.clone(),
                        version: version.clone(),
                        published: None,
                        owner: None,
                    });
                }
            }
            Ok(())
        };
        parse().map_err(|kind| format!("cannot parse {path}: {kind}"))?;
    }
    Ok(result)
}

pub(super) fn age_violations(
    candidates: &[LockCandidate],
    times: &BTreeMap<Identity, f64>,
    now: f64,
    urgent: &Urgent,
) -> Vec<String> {
    let mut result = Vec::new();
    for candidate in candidates {
        let flake = candidate.ecosystem == "flake";
        let published = if flake {
            candidate.published
        } else {
            times.get(&candidate.identity()).copied()
        };
        let urgent = urgent.contains(&(candidate.name.clone(), candidate.version.clone()));
        let identity = format!("{}@{}", candidate.name, candidate.version);
        if published.is_none() && (flake || !urgent) {
            result.push(format!(
                "dep-gate: {}: {identity}: missing {}",
                candidate.path,
                if flake {
                    "lastModified"
                } else {
                    "publish time"
                }
            ));
        } else if published.is_some_and(|t| t > now - COOLDOWN) && !urgent {
            result.push(format!(
                "dep-gate: {}: {identity}: {} less than 7 days ago",
                candidate.path,
                if flake { "lastModified" } else { "published" }
            ));
        }
    }
    result
}
