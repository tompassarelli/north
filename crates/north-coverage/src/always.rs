use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use crate::map::Test;

const BINARY: &str = "binary_id(=";
const NAME: &str = ") & test(=";

pub fn queue_lines(root: &Path, revision: &str) -> Vec<String> {
    let blob = format!("{revision}:.safe-push");
    let output = Command::new("git").arg("-C").arg(root).args(["config", "--blob", &blob, "--get-all", "land.queue"]).output();
    output.ok().filter(|output| output.status.success()).map(|output| String::from_utf8_lossy(&output.stdout).lines().map(str::to_owned).collect()).unwrap_or_default()
}

pub fn named(lines: &[String]) -> BTreeSet<Test> {
    let mut tests = BTreeSet::new();
    for line in lines {
        let mut rest = line.as_str();
        while let Some(start) = rest.find(BINARY) {
            rest = &rest[start + BINARY.len()..];
            let Some((binary, after)) = rest.split_once(NAME) else { break };
            let Some((name, after)) = after.split_once(')') else { break };
            tests.insert(Test { binary: binary.to_owned(), name: name.to_owned() });
            rest = after;
        }
    }
    tests
}
