use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use north_base::foundation::clock::Timestamp;
use north_base::verdict_cache::fingerprint::{self, Artifacts, Tree, Weak};
use north_base::verdict_cache::path_set::{self, Entry};
use north_base::verdict_cache::{Pass, Store, set_id};

use super::lines::Record;
use super::outcome::{Outcome, parse};
use super::{USAGE, execute, recorder};

const METADATA_LIMIT: std::time::Duration = std::time::Duration::from_secs(120);

struct Options {
    probe: String,
    record: bool,
    tree: String,
    command: String,
}

fn options(arguments: &[String]) -> Option<Options> {
    let mut parsed = Options { probe: String::new(), record: false, tree: "HEAD".to_owned(), command: String::new() };
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--probe" => parsed.probe = rest.next()?.clone(),
            "--record" => parsed.record = true,
            "--tree" => parsed.tree = rest.next()?.clone(),
            "--" => {
                parsed.command = rest.next()?.clone();
                return (rest.next().is_none() && !parsed.probe.is_empty()).then_some(parsed);
            }
            _ => return None,
        }
    }
    None
}

fn git(root: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git").arg("-C").arg(root).args(arguments).stdin(Stdio::null()).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn target_directory(root: &Path, command: &str) -> PathBuf {
    let named = command.split_whitespace().find_map(|word| word.strip_prefix("CARGO_TARGET_DIR=")).map(|value| value.trim_matches(['"', '\'']).to_owned());
    let chosen = named.or_else(|| std::env::var("CARGO_TARGET_DIR").ok()).unwrap_or_else(|| "target".to_owned());
    root.join(chosen)
}

fn rustc_identity(target: &Path) -> String {
    let text = fs::read_to_string(target.join(".rustc_info.json")).unwrap_or_default();
    let value: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    value["outputs"].as_object().into_iter().flat_map(|outputs| outputs.values()).filter_map(|output| output["stdout"].as_str()).find(|stdout| stdout.starts_with("rustc ") && stdout.contains("\nrelease:")).unwrap_or_default().to_owned()
}

fn metadata(root: &Path) -> Option<String> {
    let shell = root.join("bin/land-shell");
    let mut command = if shell.is_file() { Command::new(shell) } else { Command::new("cargo") };
    if command.get_program() != "cargo" {
        command.args(["--dev", "--", "cargo"]);
    }
    let mut child = command.args(["metadata", "--format-version", "1", "--locked", "--offline"]).current_dir(root).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        std::io::Read::read_to_string(&mut stdout, &mut text).ok().map(|_| text)
    });
    let deadline = std::time::Instant::now() + METADATA_LIMIT;
    let status = loop {
        if let Some(status) = child.try_wait().ok()? {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    status.success().then(|| reader.join().ok().flatten()).flatten()
}

fn hostname() -> String {
    fs::read_to_string("/proc/sys/kernel/hostname").map(|name| name.trim().replace(char::is_whitespace, "-")).ok().filter(|name| !name.is_empty()).unwrap_or_else(|| "unknown".to_owned())
}

struct Looked {
    weak: String,
    unit: String,
    reused: Option<(String, Vec<Entry>)>,
}

struct Lookup<'a> {
    store: &'a Store,
    tree: &'a Tree,
    artifacts: &'a mut Artifacts,
}

impl Lookup<'_> {
    fn entries(bytes: &[u8]) -> Option<Vec<Entry>> {
        path_set::parse(&String::from_utf8_lossy(bytes)).ok()
    }

    fn record(&mut self, looked: &Looked, outcome: &Outcome, bytes: &[u8], sha: &str, probe: &str) -> Option<Record> {
        let entries = Self::entries(bytes)?;
        let real = set_id(bytes);
        let _ = self.store.record_set(&looked.weak, bytes);
        if fingerprint::reusable(&entries)
            && let Some(strong) = fingerprint::strong(&looked.weak, &entries, self.tree, self.artifacts)
        {
            if outcome.passed {
                let _ = self.store.record_pass(&looked.weak, &strong, &Pass { at: Timestamp::now(), sha: sha.to_owned() }, &hostname());
            } else if self.store.pass(&looked.weak, &strong).ok().flatten().is_some() {
                let _ = self.store.clear(&looked.weak, &strong);
            }
        }
        let (reused, before) = looked.reused.as_ref()?;
        (*reused != real).then(|| Record::SetDiffers { probe: probe.to_owned(), unit: looked.unit.clone(), reused: reused.clone(), real, path: fingerprint::first_difference(before, &entries).unwrap_or_default() })
    }
}

fn shadow(options: &Options, root: &Path, inside: Option<&Path>, text: &str, records: Option<&Path>) -> Result<Vec<Record>, String> {
    let outcomes = parse(text);
    if outcomes.is_empty() {
        return Ok(Vec::new());
    }
    let tree = Tree::load(root, &options.tree).map_err(|error| error.to_string())?;
    let sha = git(root, &["rev-parse", &format!("{}^{{commit}}", options.tree)]).ok_or("the checked commit does not resolve")?;
    let target = target_directory(root, &options.command);
    let packages = metadata(root);
    let mut artifacts = Artifacts::open(root, inside, &target, packages.as_deref());
    let rustc = rustc_identity(&target);
    let environment = fingerprint::normalized(std::env::vars(), &[(&target, "<target>"), (root, "<tree>")]);
    let line = fingerprint::without_selection(&options.command);
    let store = Store::here();
    let mut found = Vec::new();
    let mut looked = Vec::new();
    for outcome in &outcomes {
        let unit = fingerprint::unit_id(&outcome.binary, &outcome.test);
        let weak = fingerprint::weak(&Weak { unit: &unit, line: &line, rustc: &rustc, environment: &environment }, &tree);
        let reused = fingerprint::reusable_set(&store, &weak, &tree, &mut artifacts);
        found.push(Record::Unit { probe: options.probe.clone(), unit: unit.clone(), set: reused.as_ref().map(|(id, _)| id.clone()), passed: outcome.passed, seconds: outcome.seconds });
        looked.push(Looked { weak, unit, reused });
    }
    if let Some(directory) = records {
        let mut lookup = Lookup { store: &store, tree: &tree, artifacts: &mut artifacts };
        for (outcome, looked) in outcomes.iter().zip(&looked) {
            let Ok(bytes) = fs::read(directory.join(fingerprint::record_name(&outcome.binary, &outcome.test))) else { continue };
            found.extend(lookup.record(looked, outcome, &bytes, &sha, &options.probe));
        }
    }
    Ok(found)
}

pub fn run(arguments: &[String]) -> u8 {
    let Some(options) = options(arguments) else {
        eprintln!("usage: {USAGE}");
        return 64;
    };
    let Some(root) = git(Path::new("."), &["rev-parse", "--show-toplevel"]).map(PathBuf::from) else {
        eprintln!("north land reuse observe: not inside a git tree");
        return 64;
    };
    let inside = recorder::inside_root(&root);
    let target = target_directory(&root, &options.command);
    let recording = options.record.then(|| recorder::stage(&root, inside.as_deref(), &target, &options.command)).flatten();
    let command = recording.as_ref().map_or(options.command.as_str(), |recording| recording.command.as_str());
    let (code, text) = match execute::run(command, &[]) {
        Ok(ran) => ran,
        Err(error) => {
            eprintln!("north land reuse observe: {error}");
            return 125;
        }
    };
    match shadow(&options, &root, inside.as_deref(), &text, recording.as_ref().map(|recording| recording.records.path())) {
        Ok(records) => {
            let reused = records.iter().filter(|record| matches!(record, Record::Unit { set: Some(_), .. })).count();
            println!("north land reuse: {}: {} unit(s) seen, {reused} would be reused{}", options.probe, records.iter().filter(|record| matches!(record, Record::Unit { .. })).count(), if recording.is_some() { ", recorded" } else { "" });
            for record in records {
                println!("{}", record.marked());
            }
        }
        Err(error) => eprintln!("north land reuse observe: {error}"),
    }
    u8::try_from(code & 0xff).unwrap_or(1)
}
