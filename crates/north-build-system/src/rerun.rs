use std::collections::BTreeMap;

use north_base::platform::orchestration;

/// The steps that rerun the failing `cases` (each crate directory, as `crates/NAME/`, to its quoted case names) on `host`
/// in the tree at the working directory, when that host's policy builds with Buck2. Each case's crate unit-test binary
/// is built on the workers and run here inside the dependency box with `--exact` naming the case, so the second run is
/// on another machine than the worker that went red. A case whose binary cannot be built, or whose run executed no
/// test, runs `cargo` instead, so the rerun never ends green on a case it did not execute. `None` on a Cargo host.
pub fn rerun_steps(host: &str, cases: &BTreeMap<String, Vec<String>>, cargo: &dyn Fn(&str) -> String) -> Option<Vec<String>> {
    let policy = orchestration::read().ok()?;
    if !orchestration::builds_with_buck2(&policy, host) {
        return None;
    }
    Some(cases.iter().flat_map(|(directory, names)| names.iter().map(move |name| case_step(directory, name, cargo))).collect())
}

fn case_step(directory: &str, name: &str, cargo: &dyn Fn(&str) -> String) -> String {
    let directory = directory.trim_end_matches('/');
    let krate = directory.rsplit('/').next().unwrap_or(directory);
    let label = shell_quote(&format!("//{directory}:{krate}-unittest-bin"));
    format!(
        "P=$(./bin/land-shell --dev -- buck2 build --remote-only --show-json-output {label} | tr -d '{{}}\"' | awk -F: '{{print $NF}}'); OUT=$(./bin/land-shell --dev -- ./\"$P\" --exact {name} --nocapture 2>&1); echo \"$OUT\"; RESULT=$(echo \"$OUT\" | grep '^test result:' | tail -n 1); case \"$RESULT\" in *'ok. 1 passed'*) ;; *'1 failed'*) status=1 ;; *) {fallback} ;; esac",
        fallback = cargo(name)
    )
}

fn shell_quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}
