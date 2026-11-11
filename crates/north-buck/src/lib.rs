//! `north buck targets`: writes the Buck2 target of every workspace crate, binary, test and example from the
//! Cargo manifests, so the two build graphs cannot drift while both exist (docs/rationale/compile-reuse.md).
//! Reads `cargo metadata` and the manifests' lint tables; depends on no other north crate.
mod env;
mod lints;
mod plan;
mod scan;
mod sources;
mod starlark;
mod workspace;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const USAGE: &str = "usage: north buck targets [--check] [--root DIR]";

pub fn run(arguments: &[String]) -> u8 {
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
    match generate(&root).and_then(|files| if check { report_drift(&files) } else { write(&files) }) {
        Ok(code) => code,
        Err(why) => {
            eprintln!("north buck targets: {why}");
            1
        }
    }
}

pub struct Generated {
    pub path: PathBuf,
    pub content: Option<String>,
}

pub fn generate(dir: &Path) -> Result<Vec<Generated>, String> {
    let workspace = workspace::parse(&workspace::cargo_metadata(dir)?)?;
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
    let mut generated: Vec<Generated> = files.iter().map(|file| Generated { path: workspace.root.join(&file.dir).join("BUCK"), content: Some(starlark::render(file)) }).collect();
    for stale in stale_buckfiles(&workspace.root, &generated)? {
        generated.push(Generated { path: stale, content: None });
    }
    Ok(generated)
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
