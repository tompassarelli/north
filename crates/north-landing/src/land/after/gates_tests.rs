use std::fs;
use std::path::Path;
use std::process::Command;

use super::{Landed, run};

fn stub(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    crate::foundation::shim::write_script(path, &format!("{body}\n"));
}

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").current_dir(directory).args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"]).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[test]
fn every_gate_runs_and_only_the_failing_ones_are_reported_with_their_output() {
    let root = tempfile::tempdir().unwrap();
    let tree = root.path().join("tree");
    fs::create_dir_all(&tree).unwrap();
    git(&tree, &["init", "-q", "-b", "main"]);
    git(&tree, &["commit", "-q", "--allow-empty", "-m", "base"]);
    let base = git(&tree, &["rev-parse", "HEAD"]);
    git(&tree, &["commit", "-q", "--allow-empty", "-m", "head"]);
    let head = git(&tree, &["rev-parse", "HEAD"]);
    let tools = root.path().join("bin");
    let trace = root.path().join("trace");
    stub(&tools.join("safe-push"), "exit 9");
    stub(&tools.join("code-health"), "echo 'code-health found debt'; exit 1");
    stub(&tools.join("agent-instruction-check"), &format!("echo \"instructions $*\" >>{}; echo 'AGENTS.md is over its limit' >&2; exit 3", trace.display()));
    let failures = run(&Landed { tree: &tree, base: &base, head: &head, safe_push: &tools.join("safe-push") });
    let gates: Vec<&str> = failures.iter().map(|failure| failure.gate.as_str()).collect();
    assert_eq!(gates, ["code-health", "instruction-limits"]);
    assert!(failures[0].output.contains("code-health found debt") && failures[0].output.contains("✗ code-health gate refused"), "{}", failures[0].output);
    assert!(failures[1].output.contains("AGENTS.md is over its limit"), "{}", failures[1].output);
    let trace = fs::read_to_string(trace).unwrap();
    let root_path = fs::canonicalize(&tree).unwrap();
    assert!(trace.contains(&format!("instructions --repo {} --ref {head}", root_path.display())), "{trace}");
}
