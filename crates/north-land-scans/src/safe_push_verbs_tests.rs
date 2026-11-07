use std::path::Path;

use super::*;

fn git_in(directory: &Path, arguments: &[&str]) {
    let status = std::process::Command::new("git")
        .current_dir(directory)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .args(arguments)
        .status()
        .unwrap();
    assert!(status.success(), "git {arguments:?}");
}

const TABLE: &str = "pub(super) const VERB_KINDS: VerbKinds = VerbKinds::new(&[\n    (\"\", Status),\n    (\"capacity\", Status),\n    (\"dev-shell\", Status),\n    (\"land scan\", Diagnosis),\n    (\"land\", Landing),\n]);\n";
const SETTINGS: &str = "[mutant]\n\trun = ./bin/head-north --code crates/dev/shell.rs,src/cli/ dev-shell run -- bash mutants.sh\n[land]\n\tcheck = src/ :: ./bin/head-north --code src/cli/ --no-build land scan nix-entry ${SAFE_PUSH_LAND_HEAD}\n\tcheck = x :: \"./bin/land-shell -- ./bin/head-north --code src/cli/\" ${BASE}\n\t# north gone-verb ignored\n\tcontract = file :: a format a past north writes or reads\n\tmachine-bound = reads what no lane tree holds, which north writes\n";

fn commit(directory: &Path, table: &str, message: &str) {
    std::fs::create_dir_all(directory.join("src/cli")).unwrap();
    std::fs::write(directory.join("src/cli/verbs.rs"), table).unwrap();
    std::fs::write(directory.join(".safe-push"), SETTINGS).unwrap();
    git_in(directory, &["add", "--all"]);
    git_in(directory, &["commit", "-qm", message, "--allow-empty"]);
}

fn scan(directory: &Path) -> (u8, String) {
    let tree = |path: &str| git::text(directory, &["show", &format!("HEAD:{path}")]).unwrap();
    let exists = |path: &str| git::text(directory, &["ls-tree", "--name-only", "HEAD", "--", path]).is_some_and(|listing| !listing.trim().is_empty());
    let found = findings(&tree(".safe-push"), &verbs(&tree("src/cli/verbs.rs")), &exists);
    let text = found.iter().map(|finding| format!("{}: {}", finding.line, finding.reason)).collect::<Vec<_>>().join("\n");
    (run(directory, &[]), text)
}

#[test]
fn a_lane_that_deletes_a_verb_a_safe_push_line_runs_or_a_code_path_it_names_is_refused_with_the_line() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path();
    git_in(directory, &["init", "-q", "-b", "main"]);
    std::fs::create_dir_all(directory.join("crates/dev")).unwrap();
    std::fs::write(directory.join("crates/dev/shell.rs"), "").unwrap();
    commit(directory, TABLE, "tree");
    assert_eq!(scan(directory), (0, String::new()));

    commit(directory, &TABLE.replace("    (\"dev-shell\", Status),\n", ""), "delete the dev-shell row");
    assert_eq!(scan(directory), (1, "2: calls north dev-shell, a verb src/cli/verbs.rs does not list".to_owned()));

    commit(directory, TABLE, "restore the row");
    git_in(directory, &["rm", "-q", "crates/dev/shell.rs"]);
    git_in(directory, &["commit", "-qm", "move the module"]);
    assert_eq!(scan(directory), (1, "2: --code names crates/dev/shell.rs, which the tree does not have".to_owned()));
}
