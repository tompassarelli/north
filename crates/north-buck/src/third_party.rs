use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const FILE: &str = "third-party/BUCK";

type Features = BTreeMap<(String, String), BTreeSet<String>>;

#[derive(Default)]
struct Targets {
    names: BTreeSet<String>,
    crates: Features,
}

#[derive(Default)]
struct Rule {
    name: Option<String>,
    package: Option<String>,
    version: Option<String>,
    features: BTreeSet<String>,
}

impl Rule {
    fn attribute(&mut self, line: &str) {
        if let Some(rest) = line.strip_prefix("    name = ") {
            self.name = quoted(rest).next();
        } else if let Some(rest) = line.trim_start().strip_prefix("\"CARGO_PKG_NAME\":") {
            self.package = quoted(rest).next();
        } else if let Some(rest) = line.trim_start().strip_prefix("\"CARGO_PKG_VERSION\":") {
            self.version = quoted(rest).next();
        }
    }

    fn finish(self, targets: &mut Targets) {
        targets.names.extend(self.name);
        if let (Some(package), Some(version)) = (self.package, self.version) {
            targets.crates.entry((package, version)).or_default().extend(self.features);
        }
    }
}

#[derive(Default)]
struct Reader {
    targets: Targets,
    rule: Rule,
    in_features: bool,
}

impl Reader {
    fn line(&mut self, line: &str) {
        if self.in_features {
            self.rule.features.extend(quoted(line));
            self.in_features = !line.contains(']');
        } else if line.starts_with(')') {
            std::mem::take(&mut self.rule).finish(&mut self.targets);
        } else if let Some(rest) = line.trim_start().strip_prefix("features = [") {
            self.rule.features.extend(quoted(rest));
            self.in_features = !rest.contains(']');
        } else {
            self.rule.attribute(line);
        }
    }
}

fn quoted(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split('"').skip(1).step_by(2).map(str::to_owned)
}

/// The rules reindeer writes: each rule's name, and the features it builds the package named by its `CARGO_PKG_NAME` and `CARGO_PKG_VERSION` with, a platform's own features included.
fn read(buck: &str) -> Targets {
    let mut reader = Reader::default();
    for line in buck.lines() {
        reader.line(line);
    }
    reader.targets
}

/// The features Cargo's resolver turns on for each package outside the workspace, from `cargo tree --workspace --prefix none --format "{p}|{f}"`, merged over every place a package appears.
fn cargo_features(tree: &str) -> Features {
    let mut found = Features::new();
    for line in tree.lines() {
        let Some((package, features)) = line.split_once('|') else {
            continue;
        };
        let mut words = package.split(' ');
        let (Some(name), Some(version)) = (words.next(), words.next().and_then(|word| word.strip_prefix('v'))) else {
            continue;
        };
        if words.any(|word| word.starts_with("(/")) {
            continue;
        }
        found.entry((name.to_owned(), version.to_owned())).or_default().extend(features.trim_end_matches(" (*)").split(',').filter(|feature| !feature.is_empty()).map(str::to_owned));
    }
    found
}

fn drift(buck: &Targets, cargo: &Features, labels: &BTreeSet<String>) -> Vec<String> {
    let mut found = Vec::new();
    for (key, wanted) in cargo {
        let (name, version) = key;
        match buck.crates.get(key) {
            None => found.push(format!("{FILE} has no target for {name} {version}, which the Cargo build uses")),
            Some(built) => {
                let lacking: Vec<&str> = wanted.difference(built).map(String::as_str).collect();
                if !lacking.is_empty() {
                    found.push(format!("{FILE} builds {name} {version} without {}, which the Cargo build turns on", lacking.join(", ")));
                }
            }
        }
    }
    for (name, version) in buck.crates.keys().filter(|key| !cargo.contains_key(*key)) {
        found.push(format!("{FILE} builds {name} {version}, which the Cargo build no longer uses"));
    }
    for label in labels {
        if let Some(name) = label.strip_prefix("//third-party:").filter(|name| !buck.names.contains(*name)) {
            found.push(format!("a workspace target depends on {label}, but {FILE} has no target named {name}"));
        }
    }
    found
}

/// Each way `third-party/BUCK` under `root` lags the Cargo build that `tree` describes: a package without a target, a target built without a feature Cargo turns on, a target Cargo no longer builds, and a label a workspace target depends on that the file does not define. A feature only the Buck target has is not one, since reindeer may add it.
pub fn lag(root: &Path, tree: &str, labels: &BTreeSet<String>) -> Vec<String> {
    let buck = std::fs::read_to_string(root.join(FILE)).unwrap_or_default();
    drift(&read(&buck), &cargo_features(tree), labels)
}
