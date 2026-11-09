//! The lander's advisory gate: `cargo deny check advisories bans sources` over each Cargo.lock a lane changes, against an advisory database fetched at most once a day, refusing only the locked versions the lane adds.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use regex::Regex;
use serde_json::{Value, json};

use crate::dep_gate::{self, Snapshot};

const ACCEPTED_FORMAT: &str = "Dependency-Advisory: <advisory-id> — <reason> (ASCII - also accepted)";
const FETCHED_STAMP: &str = "fetched";
const DAY: Duration = Duration::from_secs(86_400);

type Locked = BTreeSet<(String, String)>;

/// Where a host keeps cargo-deny's advisory database under its state home.
pub fn database(state_home: &Path) -> std::path::PathBuf {
    state_home.join("north/advisory-db")
}

/// Whether the lane adds a locked crate version cargo-deny flags with an open advisory, a banned crate or an unknown source, and that no `Dependency-Advisory:` trailer accepts; `database` is cargo-deny's db-path.
pub fn refuse(root: &Path, base: &str, head: &str, database: &Path, out: &mut impl Write) -> bool {
    match refusals(root, base, head, database) {
        Ok(found) if found.is_empty() => false,
        Ok(found) => {
            let _ = writeln!(out, "{}\n{ACCEPTED_FORMAT}", found.join("\n"));
            true
        }
        Err(error) => {
            let _ = writeln!(out, "advisory-gate: {error}");
            true
        }
    }
}

fn refusals(root: &Path, base: &str, head: &str, database: &Path) -> Result<Vec<String>, String> {
    let (before, after) = (dep_gate::read_snapshot(root, base)?, dep_gate::read_snapshot(root, head)?);
    let changed = added_versions(&before, &after)?;
    if changed.is_empty() {
        return Ok(Vec::new());
    }
    let messages: Vec<String> = dep_gate::git(root, &["log", "--format=%B%x00", &format!("{base}..{head}")])?.split('\0').map(str::to_owned).collect();
    let accepted = accepted(&messages);
    refresh(database)?;
    let mut found = Vec::new();
    for (path, added) in changed {
        for (name, version, id, message) in flagged(&after.locks[&path], database)? {
            if added.contains(&(name.clone(), version.clone())) && !accepted.contains(&id) {
                found.push(format!("advisory-gate: {path}: {name}@{version}: {id} {message}"));
            }
        }
    }
    Ok(found)
}

fn added_versions(before: &Snapshot, after: &Snapshot) -> Result<BTreeMap<String, Locked>, String> {
    let mut changed = BTreeMap::new();
    for (path, text) in after.locks.iter().filter(|(path, _)| path.rsplit('/').next() == Some("Cargo.lock")) {
        if before.locks.get(path) == Some(text) {
            continue;
        }
        let parse = |text: &str| dep_gate::locked_versions(path, text).map_err(|kind| format!("cannot parse {path}: {kind}"));
        let old = before.locks.get(path).map(|text| parse(text)).transpose()?.unwrap_or_default();
        let added: Locked = parse(text)?.difference(&old).cloned().collect();
        if !added.is_empty() {
            changed.insert(path.clone(), added);
        }
    }
    Ok(changed)
}

fn accepted(messages: &[String]) -> BTreeSet<String> {
    let re = Regex::new(r"\ADependency-Advisory:\s+(\S+)\s+(?:—|-)\s+\S.*\z").unwrap();
    dep_gate::trailer_lines(messages).into_iter().filter_map(|line| re.captures(line).map(|c| c[1].to_owned())).collect()
}

fn refresh(database: &Path) -> Result<(), String> {
    let stamp = database.join(FETCHED_STAMP);
    let fresh = fs::metadata(&stamp).and_then(|meta| meta.modified()).is_ok_and(|at| SystemTime::now().duration_since(at).is_ok_and(|age| age < DAY));
    if fresh {
        return Ok(());
    }
    fs::create_dir_all(database).map_err(|error| format!("{}: {error}", database.display()))?;
    let scratch = tempfile::tempdir().map_err(|error| error.to_string())?;
    let config = write_config(scratch.path(), database)?;
    fs::write(scratch.path().join("Cargo.toml"), "[workspace]\n").map_err(|error| error.to_string())?;
    let output = deny(scratch.path()).args(["fetch", "--config"]).arg(&config).arg("db").output().map_err(|error| format!("cargo-deny: {error}"))?;
    if !output.status.success() {
        return Err(format!("cannot fetch the advisory database into {}: {}", database.display(), String::from_utf8_lossy(&output.stderr).trim_end()));
    }
    fs::write(&stamp, "").map_err(|error| format!("{}: {error}", stamp.display()))
}

fn deny(workspace: &Path) -> Command {
    let mut command = Command::new("cargo-deny");
    command.arg("--manifest-path").arg(workspace.join("Cargo.toml")).stdin(Stdio::null());
    command
}

fn write_config(directory: &Path, database: &Path) -> Result<std::path::PathBuf, String> {
    let config = directory.join("deny.toml");
    let text = format!("[advisories]\ndb-path = {}\nyanked = \"allow\"\n\n[sources]\nunknown-registry = \"deny\"\nunknown-git = \"deny\"\n\n[sources.allow-org]\ngithub = [\"tompassarelli\"]\n", json!(database.to_string_lossy()));
    fs::write(&config, text).map_err(|error| format!("{}: {error}", config.display()))?;
    Ok(config)
}

fn flagged(lock: &str, database: &Path) -> Result<Vec<(String, String, String, String)>, String> {
    let scratch = tempfile::tempdir().map_err(|error| error.to_string())?;
    let workspace = scratch.path();
    let config = write_config(workspace, database)?;
    let metadata = workspace.join("metadata.json");
    let write = |path: &Path, text: &str| fs::write(path, text).map_err(|error| format!("{}: {error}", path.display()));
    write(&workspace.join("Cargo.toml"), "[workspace]\n")?;
    write(&workspace.join("Cargo.lock"), lock)?;
    write(&metadata, &metadata_from_lock(lock, workspace)?.to_string())?;
    let output = deny(workspace)
        .args(["--offline", "--format", "json", "check", "--disable-fetch", "--config"])
        .arg(&config)
        .arg("--metadata-path")
        .arg(&metadata)
        .args(["advisories", "bans", "sources"])
        .output()
        .map_err(|error| format!("cargo-deny: {error}; it runs from the lander's PATH"))?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut found = Vec::new();
    let mut summarized = false;
    for line in stderr.lines() {
        let Ok(row) = serde_json::from_str::<Value>(line) else { continue };
        summarized |= row["type"] == "summary";
        let fields = &row["fields"];
        if row["type"] != "diagnostic" || fields["severity"] != "error" {
            continue;
        }
        let krate = &fields["graphs"][0]["Krate"];
        let (Some(name), Some(version)) = (krate["name"].as_str(), krate["version"].as_str()) else { continue };
        let id = fields["advisory"]["id"].as_str().or(fields["code"].as_str()).unwrap_or("error");
        found.push((name.to_owned(), version.to_owned(), id.to_owned(), fields["message"].as_str().unwrap_or("").to_owned()));
    }
    if !summarized {
        return Err(format!("cargo-deny gave no verdict: {}", stderr.lines().rev().take(3).collect::<Vec<_>>().join(" | ")));
    }
    Ok(found)
}

fn metadata_from_lock(lock: &str, workspace: &Path) -> Result<Value, String> {
    let data = dep_gate::parse_toml(lock).map_err(|kind| format!("cannot parse Cargo.lock: {kind}"))?;
    let packages = data.get("package").and_then(Value::as_array).cloned().unwrap_or_default();
    let local = format!("path+file://{}", workspace.display());
    let field = |package: &Value, key: &str| package.get(key).and_then(Value::as_str).unwrap_or("").to_owned();
    let id = |package: &Value| format!("{}#{}@{}", package.get("source").and_then(Value::as_str).unwrap_or(&local), field(package, "name"), field(package, "version"));
    let resolve = |entry: &str| -> Option<&Value> {
        let mut words = entry.split(' ');
        let (name, version) = (words.next()?, words.next());
        packages.iter().find(|package| field(package, "name") == name && version.is_none_or(|version| field(package, "version") == version))
    };
    let (mut listed, mut nodes, mut members) = (Vec::new(), Vec::new(), Vec::new());
    for package in &packages {
        let dependencies: Vec<&Value> = package.get("dependencies").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).filter_map(resolve).collect();
        let declared: Vec<Value> = dependencies.iter().map(|dependency| json!({"name": field(dependency, "name"), "source": dependency.get("source"), "req": format!("={}", field(dependency, "version")), "kind": null, "optional": false, "uses_default_features": true, "features": [], "target": null, "rename": null, "registry": null})).collect();
        let edges: Vec<Value> = dependencies.iter().map(|dependency| json!({"name": field(dependency, "name").replace('-', "_"), "pkg": id(dependency), "dep_kinds": [{"kind": null, "target": null}]})).collect();
        let manifest = workspace.join(format!("{}-{}", field(package, "name"), field(package, "version"))).join("Cargo.toml");
        listed.push(json!({"name": field(package, "name"), "version": field(package, "version"), "id": id(package), "source": package.get("source"), "dependencies": declared, "targets": [], "features": {}, "manifest_path": manifest}));
        nodes.push(json!({"id": id(package), "dependencies": dependencies.iter().map(|dependency| id(dependency)).collect::<Vec<_>>(), "deps": edges, "features": []}));
        if package.get("source").is_none() {
            members.push(id(package));
        }
    }
    Ok(json!({"packages": listed, "workspace_members": members, "workspace_default_members": members, "resolve": {"nodes": nodes, "root": null}, "target_directory": workspace.join("target"), "version": 1, "workspace_root": workspace, "metadata": null}))
}
