use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use north_base::foundation::check_log::Failure;

fn quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

pub fn rerun_command(failed: &BTreeSet<Failure>, file_of: &dyn Fn(&Failure) -> Option<String>) -> Option<String> {
    compose(failed, None, file_of)
}

pub fn rerun_on(host: &str, failed: &BTreeSet<Failure>, file_of: &dyn Fn(&Failure) -> Option<String>) -> Option<String> {
    compose(failed, Some(host), file_of)
}

pub fn crate_root(file: &str) -> String {
    let mut directory = Path::new(file).parent();
    while let Some(candidate) = directory {
        if candidate.as_os_str().is_empty() {
            break;
        }
        if candidate.join("Cargo.toml").is_file() {
            return format!("{}/", candidate.display());
        }
        directory = candidate.parent();
    }
    "src/".to_owned()
}

fn cases_by_crate(failed: &BTreeSet<Failure>, file_of: &dyn Fn(&Failure) -> Option<String>) -> BTreeMap<String, Vec<String>> {
    let mut cases: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for failure in failed {
        let Failure::Cargo(name) = failure else { continue };
        let Some(file) = file_of(failure) else { continue };
        let directory = crate_root(&file);
        if directory != "src/" {
            cases.entry(directory).or_default().push(quote(name));
        }
    }
    cases
}

fn compose(failed: &BTreeSet<Failure>, host: Option<&str>, file_of: &dyn Fn(&Failure) -> Option<String>) -> Option<String> {
    let cargo: Vec<String> = failed.iter().filter_map(|failure| match failure {
        Failure::Cargo(name) => Some(quote(name)),
        _ => None,
    }).collect();
    let harness: Vec<&str> = failed.iter().filter_map(|failure| match failure {
        Failure::Harness(path) => Some(path.as_str()),
        _ => None,
    }).collect();
    if cargo.is_empty() && harness.is_empty() {
        return None;
    }
    let mut steps = vec!["status=0".to_owned()];
    let cargo_case = |name: &str| format!("CARGO_TARGET_DIR=target ./bin/land-shell --dev -- cargo test --locked --offline --workspace --no-fail-fast -- --exact {name} 2>&1 || status=1");
    let crates = cases_by_crate(failed, file_of);
    match host.and_then(|host| north_build_system::rerun_steps(host, &crates, &cargo_case)) {
        Some(buck2) => {
            steps.extend(buck2);
            let covered: BTreeSet<&String> = crates.values().flatten().collect();
            let uncovered: Vec<&String> = cargo.iter().filter(|name| !covered.contains(name)).collect();
            if !uncovered.is_empty() {
                steps.push(cargo_case(&uncovered.iter().map(|name| name.as_str()).collect::<Vec<_>>().join(" ")));
            }
        }
        None if !cargo.is_empty() => steps.push(cargo_case(&cargo.join(" "))),
        None => {}
    }
    if !harness.is_empty() {
        let paths: Vec<String> = harness.iter().map(|path| quote(path)).collect();
        steps.push(format!("env -u SAFE_PUSH_LAND_HEAD -u SAFE_PUSH_LAND_RUNTIME_HEAD ./bin/head-north land tests {} 2>&1 || status=1", paths.join(" ")));
    }
    steps.push("exit $status".to_owned());
    Some(steps.join("; "))
}
