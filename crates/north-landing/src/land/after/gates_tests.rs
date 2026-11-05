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
    fs::write(tree.join("AGENTS.md"), "rule\n".repeat(101)).unwrap();
    git(&tree, &["add", "AGENTS.md"]);
    git(&tree, &["commit", "-q", "-m", "head"]);
    let head = git(&tree, &["rev-parse", "HEAD"]);
    let tools = root.path().join("bin");
    stub(&tools.join("safe-push"), "exit 9");
    stub(&tools.join("code-health"), "echo 'code-health found debt'; exit 1");
    let failures = run(&Landed { tree: &tree, base: &base, head: &head, safe_push: &tools.join("safe-push") });
    let gates: Vec<&str> = failures.iter().map(|failure| failure.gate.as_str()).collect();
    assert_eq!(gates, ["code-health", "instruction-limits"]);
    assert!(failures[0].output.contains("code-health found debt") && failures[0].output.contains("✗ code-health gate refused"), "{}", failures[0].output);
    assert!(failures[1].output.contains(&format!("agent-instruction-check: AGENTS.md@{head}: 101 lines")), "{}", failures[1].output);
}

#[test]
fn the_crate_lines_gate_runs_north_beside_safe_push_only_where_structure_toml_enrolls_it() {
    let root = tempfile::tempdir().unwrap();
    let tree = root.path().join("tree");
    fs::create_dir_all(&tree).unwrap();
    git(&tree, &["init", "-q", "-b", "main"]);
    fs::write(tree.join("structure.toml"), "[thresholds]\nfile_lines = 500\n").unwrap();
    git(&tree, &["add", "-A"]);
    git(&tree, &["commit", "-q", "-m", "base"]);
    let base = git(&tree, &["rev-parse", "HEAD"]);
    let tools = root.path().join("bin");
    let trace = root.path().join("trace");
    stub(&tools.join("safe-push"), "exit 9");
    stub(&tools.join("code-health"), "exit 0");
    stub(&tools.join("agent-instruction-check"), "exit 0");
    stub(&tools.join("north"), &format!("echo \"north $*\" >>{}; echo 'north-core grew 10 -> 11'; exit 1", trace.display()));
    let gates = |head: &str| run(&Landed { tree: &tree, base: &base, head, safe_push: &tools.join("safe-push") });
    assert!(gates(&base).iter().all(|failure| failure.gate != "crate-lines"));
    assert!(!trace.exists());
    fs::write(tree.join("structure.toml"), "[crate_lines]\ndefault = 25000\n").unwrap();
    git(&tree, &["commit", "-q", "-am", "enroll"]);
    let head = git(&tree, &["rev-parse", "HEAD"]);
    let failures = gates(&head);
    let refused = failures.iter().find(|failure| failure.gate == "crate-lines").expect("crate-lines refused");
    assert!(refused.output.contains("north-core grew 10 -> 11") && refused.output.contains("✗ crate-lines gate refused"), "{}", refused.output);
    assert_eq!(fs::read_to_string(trace).unwrap(), format!("north code-health crate-lines {base} {head}\n"));
}
