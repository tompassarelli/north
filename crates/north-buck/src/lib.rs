//! `north buck targets`: writes the Buck2 target of every workspace crate, binary, test and example from the
//! Cargo manifests, so the two build graphs cannot drift while both exist (docs/rationale/compile-reuse.md);
//! `--check` also holds `third-party/BUCK` to the packages and features Cargo resolves.
//! Reads the resolved manifests and features through the readers its caller passes and the manifests' lint tables; depends on no other north crate.
mod env;
mod gate;
mod host_class;
mod lints;
mod plan;
mod rule;
mod scan;
mod sources;
mod starlark;
mod third_party;
mod workspace;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const USAGE: &str = "usage: north buck targets [--check] [--root DIR]";

/// Reads a workspace's packages with their dependency graph resolved for one platform, in `cargo metadata --format-version 1` form.
pub type Manifests = fn(&Path, &str) -> Result<String, String>;

/// Reads the features Cargo's resolver turns on for each package of a workspace for one platform, in `cargo tree --workspace --prefix none --format "{p}|{f}"` form.
pub type FeatureGraph = fn(&Path, &str) -> Result<String, String>;

/// What Cargo says about a workspace; the Buck2 targets must agree with it.
pub struct Cargo {
    pub manifests: Manifests,
    pub features: FeatureGraph,
}

const PLATFORM: &str = "x86_64-unknown-linux-gnu";
// The mark north-build-system and the capacity shim print when cargo itself could not run.
const INFRASTRUCTURE: &str = "(infrastructure, exit 75)";
const INFRASTRUCTURE_STATUS: u8 = 75;

/// `state` is the state home a gate in shadow logs its decisions under.
pub fn run(arguments: &[String], cargo: &Cargo, state: &Path) -> u8 {
    let Some(("targets", rest)) = arguments.split_first().map(|(verb, rest)| (verb.as_str(), rest)) else {
        eprintln!("{USAGE}");
        return 2;
    };
    let mut check = false;
    let mut root = PathBuf::from(".");
    let mut rest = rest.iter();
    while let Some(argument) = rest.next() {
        match (argument.as_str(), argument == "--root") {
            ("--check", _) => check = true,
            (_, true) if let Some(dir) = rest.next() => root = PathBuf::from(dir),
            _ => {
                eprintln!("{USAGE}");
                return 2;
            }
        }
    }
    let outcome = if check { compare(&root, cargo, state) } else { generate(&root, cargo.manifests).and_then(|planned| write(&planned.files)) };
    match outcome {
        Ok(code) => code,
        Err(why) => {
            eprintln!("north buck targets: {why}");
            if why.contains(INFRASTRUCTURE) { INFRASTRUCTURE_STATUS } else { 1 }
        }
    }
}

fn compare(root: &Path, cargo: &Cargo, state: &Path) -> Result<u8, String> {
    let (planned, features) = std::thread::scope(|scope| {
        let features = scope.spawn(|| (cargo.features)(root, PLATFORM));
        (generate(root, cargo.manifests), features.join())
    });
    let planned = planned?;
    let features = features.map_err(|_| "the read of Cargo's resolved features panicked".to_owned())??;
    let lagging = third_party::lag(&planned.root, &features, &planned.labels);
    Ok(report_drift(&planned.files)?.max(gate::decide(&planned.root, state, &lagging)))
}

pub struct Generated {
    pub path: PathBuf,
    pub content: Option<String>,
}

pub struct Planned {
    pub root: PathBuf,
    pub files: Vec<Generated>,
    /// Every `//third-party:` label a workspace target depends on.
    pub labels: BTreeSet<String>,
}

pub fn generate(dir: &Path, manifests: Manifests) -> Result<Planned, String> {
    let workspace = workspace::parse(&manifests(dir, PLATFORM)?)?;
    let workspace_lints = lints::workspace_table(&workspace.root)?;
    let mut lint_flags = BTreeMap::new();
    for package in &workspace.packages {
        lint_flags.insert(package.dir.clone(), lints::flags(&workspace.root, &package.dir, workspace_lints.as_ref())?);
    }
    for package in &workspace.packages {
        if let Some(script) = &package.build_script {
            eprintln!("north buck targets: {}: {script} is not run under Buck2 yet, so its targets compile the files it writes as committed", package.name);
        }
    }
    let files = plan::plan(&workspace, &lint_flags)?;
    let labels = files.iter().flat_map(|file| &file.rules).flat_map(|rule| rule.deps.iter().chain(rule.named_deps.values())).filter(|label| label.starts_with("//third-party:")).cloned().collect();
    let mut generated: Vec<Generated> = files.iter().map(|file| Generated { path: workspace.root.join(&file.dir).join("BUCK"), content: Some(starlark::render(file)) }).collect();
    for stale in stale_buckfiles(&workspace.root, &generated)? {
        generated.push(Generated { path: stale, content: None });
    }
    Ok(Planned { root: workspace.root, files: generated, labels })
}

fn stale_buckfiles(root: &Path, generated: &[Generated]) -> Result<Vec<PathBuf>, String> {
    let crates = root.join("crates");
    let mut stale = Vec::new();
    let Ok(entries) = std::fs::read_dir(&crates) else {
        return Ok(stale);
    };
    for entry in entries {
        let path = entry.map_err(|why| format!("{}: {why}", crates.display()))?.path().join("BUCK");
        let ours = std::fs::read_to_string(&path).is_ok_and(|text| text.starts_with(starlark::HEADER));
        if ours && !generated.iter().any(|file| file.path == path) {
            stale.push(path);
        }
    }
    stale.sort();
    Ok(stale)
}

fn differs(file: &Generated) -> bool {
    std::fs::read_to_string(&file.path).ok() != file.content
}

fn report_drift(files: &[Generated]) -> Result<u8, String> {
    let drifted: Vec<&Generated> = files.iter().filter(|file| differs(file)).collect();
    for file in &drifted {
        eprintln!("{} is not what north buck targets writes from the Cargo manifests; run it and commit the result", file.path.display());
    }
    Ok(u8::from(!drifted.is_empty()))
}

fn write(files: &[Generated]) -> Result<u8, String> {
    for file in files.iter().filter(|file| differs(file)) {
        match &file.content {
            Some(content) => std::fs::write(&file.path, content),
            None => std::fs::remove_file(&file.path),
        }
        .map_err(|why| format!("{}: {why}", file.path.display()))?;
        println!("{} {}", if file.content.is_some() { "wrote" } else { "removed" }, file.path.display());
    }
    Ok(0)
}
