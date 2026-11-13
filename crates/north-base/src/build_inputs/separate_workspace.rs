use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use toml_edit::DocumentMut;

const DIRECTIVES: &str = r"include\w*!|#\[path";
const MANIFEST: &str = "Cargo.toml";
const BUILD_SCRIPT: &str = "build.rs";

pub fn read(root: &Path, head: &str, paths: &[&str]) -> BTreeSet<String> {
    let mut unnamed: BTreeMap<String, bool> = BTreeMap::new();
    let mut apart = BTreeSet::new();
    for path in paths.iter().copied().filter(|path| path.ends_with(".rs")) {
        let Some(directory) = own_workspace(root, head, path) else { continue };
        if *unnamed.entry(directory.clone()).or_insert_with(|| inputs(root, head, &directory).is_some_and(|inputs| !inputs.name(&directory))) {
            apart.insert(path.to_owned());
        }
    }
    apart
}

fn own_workspace(root: &Path, head: &str, path: &str) -> Option<String> {
    let mut directory = path;
    while let Some((parent, _)) = directory.rsplit_once('/') {
        directory = parent;
        if let Some(text) = git(root, &["show", &format!("{head}:{directory}/{MANIFEST}")]) {
            let manifest: DocumentMut = text.parse().ok()?;
            return manifest.contains_key("workspace").then(|| directory.to_owned());
        }
    }
    None
}

struct Inputs {
    directives: String,
    manifests: String,
}

impl Inputs {
    fn name(&self, directory: &str) -> bool {
        let own: Vec<&str> = directory.split('/').collect();
        let named = |literal: &str| literal.split(['/', '\\']).any(|part| own.contains(&part));
        let text = self.directives.as_str();
        let starts = text.match_indices(['i', '#']).map(|(at, _)| at).filter(|at| text[*at..].starts_with("include") || text[*at..].starts_with("#[path"));
        let spans = starts.map(|at| &text[at..text[at..].find(';').map_or(text.len(), |end| at + end)]);
        spans.flat_map(|span| span.split('"')).any(named) || self.manifests.split('"').any(named)
    }
}

fn inputs(root: &Path, head: &str, directory: &str) -> Option<Inputs> {
    let outside = format!(":(exclude){directory}/");
    let sources = grep(root, &["-l", "-E", DIRECTIVES, head, "--", "*.rs", &outside])?;
    let listed = git(root, &["ls-tree", "-r", "--name-only", head])?;
    let inside = format!("{directory}/");
    let manifests = listed.lines().filter(|path| !path.starts_with(&inside) && [MANIFEST, BUILD_SCRIPT].contains(&path.rsplit('/').next().unwrap_or(path))).map(|path| format!("{head}:{path}")).collect();
    Some(Inputs { directives: shown(root, sources.lines().map(str::to_owned).collect())?, manifests: shown(root, manifests)? })
}

fn shown(root: &Path, blobs: Vec<String>) -> Option<String> {
    if blobs.is_empty() {
        return Some(String::new());
    }
    let arguments: Vec<&str> = ["show", "--no-color"].into_iter().chain(blobs.iter().map(String::as_str)).collect();
    git(root, &arguments)
}

fn grep(root: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git").arg("-C").arg(root).arg("grep").args(arguments).output().ok()?;
    match output.status.code() {
        Some(0) => String::from_utf8(output.stdout).ok(),
        Some(1) if output.stderr.is_empty() => Some(String::new()),
        _ => None,
    }
}

fn git(root: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git").arg("-C").arg(root).args(arguments).output().ok()?;
    output.status.success().then(|| String::from_utf8(output.stdout).ok()).flatten()
}
