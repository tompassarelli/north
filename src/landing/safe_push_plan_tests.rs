use std::path::Path;
use std::process::Command;

use super::*;

const OID_1: &str = "1111111111111111111111111111111111111111";
const OID_2: &str = "2222222222222222222222222222222222222222";
const OID_3: &str = "3333333333333333333333333333333333333333";

const ARGUMENT_ROWS: &str = "\
zero|-h
zero|--help
nonzero|--keep-lane
nonzero|--wat
nonzero|main
nonzero|--force
nonzero|-f
nonzero|--force-with-lease
nonzero|--mirror
nonzero|--delete
nonzero|--prune
nonzero|--tag
nonzero|--tag --dry-run
nonzero|--dry-run --dry-run
nonzero|--tag release-a --tag release-b
nonzero|--to
nonzero|--to main --to other
nonzero|--to main --tag release-a
nonzero|--help --dry-run
nonzero|--keep-lane --keep-lane --to main
nonzero|--direct --direct
nonzero|--to --dry-run
zero|--dry-run
zero|--to other
zero|--dry-run --to other
zero|--tag release-a
zero|--dry-run --tag release-a
zero|--tag release-a --dry-run
zero|--keep-lane --to main
zero|--direct --to main
zero|--keep-lane -h";

struct Observed {
    status: i32,
    stdout: String,
    stderr: String,
}

fn shell_functions() -> String {
    let script = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/safe-push")).unwrap();
    let mut functions = String::new();
    for name in ["durable_harness_paths", "usage", "argument_error", "plan_push", "default_branch", "enforce_one_branch_main"] {
        let start = format!("{name}() {{");
        let body: Vec<&str> = script.lines().skip_while(|line| *line != start).collect();
        let end = body.iter().position(|line| *line == "}").unwrap_or_else(|| panic!("bin/safe-push lost {name}"));
        functions.extend(body[..=end].iter().map(|line| format!("{line}\n")));
    }
    functions
}

fn shell(body: &str, arguments: &[&str]) -> Observed {
    shell_in(Path::new("."), body, arguments)
}

fn shell_in(directory: &Path, body: &str, arguments: &[&str]) -> Observed {
    let output = Command::new("bash")
        .arg("-c")
        .arg(format!("set -euo pipefail\n{}{body}", shell_functions()))
        .arg("safe-push")
        .current_dir(directory)
        .args(arguments)
        .output()
        .unwrap();
    Observed {
        status: output.status.code().unwrap(),
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

fn parsed_state(arguments: &Arguments) -> String {
    let flag = |set: bool| if set { "1" } else { "0" };
    format!(
        "{}|{}|{}|{}|{}|{}|{}\n",
        flag(arguments.dry_run),
        flag(arguments.keep_lane),
        flag(arguments.direct),
        flag(arguments.tag.is_some()),
        arguments.tag.as_deref().unwrap_or(""),
        flag(arguments.to.is_some()),
        arguments.to.as_deref().unwrap_or(""),
    )
}

fn rust_arguments(arguments: &[&str]) -> Observed {
    let owned: Vec<String> = arguments.iter().map(|argument| (*argument).to_owned()).collect();
    match super::arguments(&owned) {
        Ok(Plan::Help) => Observed { status: 0, stdout: USAGE.to_owned(), stderr: String::new() },
        Ok(Plan::Push(parsed)) => Observed { status: 0, stdout: parsed_state(&parsed), stderr: String::new() },
        Err(refusal) => Observed { status: refusal.exit_code().into(), stdout: String::new(), stderr: refusal.stderr() },
    }
}

fn assert_same(case: &str, old: &Observed, new: &Observed) {
    assert_eq!(old.status, new.status, "exit code differs for {case}");
    assert_eq!(old.stdout, new.stdout, "stdout differs for {case}");
    assert_eq!(old.stderr, new.stderr, "stderr differs for {case}");
}

const PARSED_STATE: &str = "plan_push arguments \"$@\"\n\
    printf '%s|%s|%s|%s|%s|%s|%s\\n' \"$dry\" \"$keep_lane\" \"$direct\" \"$tag_seen\" \"$tag\" \"$to_seen\" \"$to\"\n";

#[test]
fn safe_push_arguments_match_the_shell_planner_row_by_row() {
    for row in ARGUMENT_ROWS.lines() {
        let (verdict, arguments) = row.split_once('|').unwrap();
        let arguments: Vec<&str> = arguments.split_whitespace().collect();
        let new = rust_arguments(&arguments);
        assert_eq!(new.status == 0, verdict == "zero", "verdict differs for {row}");
        assert_same(row, &shell(PARSED_STATE, &arguments), &new);
    }
}

#[test]
fn safe_push_refuses_destination_names_git_would_misread() {
    for invalid in ["HEAD", "refs/heads/main", "origin/main", "../main", "bad name", "bad..name", "a@{1}", "a~1", "a^", "a:b", "a?", "a*", "a[b", "a\\b"] {
        let arguments = ["--to", invalid];
        let new = rust_arguments(&arguments);
        assert_eq!(new.status, 2, "{invalid} was accepted");
        assert_same(invalid, &shell(PARSED_STATE, &arguments), &new);
    }
}

fn snapshot_shapes() -> Vec<(&'static str, String)> {
    let destination = "refs/heads/main";
    vec![
        ("absent", String::new()),
        ("object", format!("{OID_1}\t{destination}")),
        ("sha256", format!("{}\t{destination}", "a".repeat(64))),
        ("symbolic", format!("ref: refs/heads/other\t{destination}")),
        ("ambiguous", format!("{OID_1}\t{destination}\n{OID_1}\t{destination}")),
        ("malformed", format!("not-an-oid\t{destination}")),
        ("uppercase", format!("{}\t{destination}", OID_1.replace('1', "A"))),
        ("wrong-ref", format!("{OID_1}\trefs/heads/other")),
        ("extra-field", format!("{OID_1}\t{destination}\textra")),
        ("trailing-newline", format!("{OID_1}\t{destination}\n")),
        ("doubled-tab", format!("{OID_1}\t\t{destination}")),
    ]
}

#[test]
fn safe_push_destination_snapshot_matches_the_shell_planner() {
    let destination = "refs/heads/main";
    for (shape, observed) in snapshot_shapes() {
        let new = match snapshot(&observed, destination) {
            Ok(found) => Observed { status: 0, stdout: format!("{}\n", found.render()), stderr: String::new() },
            Err(reason) => Observed { status: 1, stdout: String::new(), stderr: reason },
        };
        let expected_zero = matches!(shape, "absent" | "object" | "sha256" | "doubled-tab");
        assert_eq!(new.status == 0, expected_zero, "verdict differs for {shape}");
        let old = shell("plan_push snapshot \"$1\" \"$2\" || exit $?\n", &[&observed, destination]);
        assert_same(shape, &old, &new);
    }
    assert_eq!(snapshot("", destination), Ok(Snapshot::Absent));
    assert_eq!(snapshot(&format!("{OID_1}\t{destination}"), destination), Ok(Snapshot::Object(OID_1.to_owned())));
}

fn rust_source(current: &str) -> Observed {
    match source(current, "main") {
        Ok(()) => Observed { status: 0, stdout: String::new(), stderr: String::new() },
        Err(reason) => Observed { status: 1, stdout: String::new(), stderr: reason },
    }
}

fn assert_publication_matches_the_shell(destination_ref: &str, observed: &str) {
    let plan = publication(OID_3, destination_ref, observed);
    assert_eq!(plan.refspec, format!("{OID_3}:{destination_ref}"));
    assert_eq!(plan.lease, format!("--force-with-lease={destination_ref}:{observed}"));
    let new = Observed { status: 0, stdout: format!("{}\n{}\n", plan.refspec, plan.lease), stderr: String::new() };
    let old = shell(
        "plan_push publication \"$1\" \"$2\" \"$3\"\nprintf '%s\\n' \"$publication\" \"${publication_flags[@]}\"\n",
        &[OID_3, destination_ref, observed],
    );
    assert_same(&format!("{destination_ref} at {observed:?}"), &old, &new);
}

#[test]
fn safe_push_publishes_only_from_the_default_branch_unless_a_destination_is_named() {
    let mut rows = 0;
    for current in ["main", "feature", "farm/topic"] {
        assert_same(current, &shell("plan_push source \"$1\" \"$2\"\n", &[current, "main"]), &rust_source(current));
        for destination in ["", "main", "farm/output"] {
            for dry_run in [false, true] {
                let mut arguments = Vec::new();
                if dry_run {
                    arguments.push("--dry-run".to_owned());
                }
                if !destination.is_empty() {
                    arguments.extend(["--to".to_owned(), destination.to_owned()]);
                }
                let Ok(Plan::Push(parsed)) = super::arguments(&arguments) else { panic!("{arguments:?} refused") };
                let refused = parsed.to.is_none() && source(current, "main").is_err();
                assert_eq!(refused, current != "main" && destination.is_empty(), "{current} -> {destination:?}");
                if !refused {
                    let destination_ref = format!("refs/heads/{}", if destination.is_empty() { current } else { destination });
                    for observed in ["", OID_1, OID_2] {
                        assert_publication_matches_the_shell(&destination_ref, observed);
                    }
                }
                rows += 1;
            }
        }
    }
    assert_eq!(rows, 18);
}

#[test]
fn safe_push_durable_harness_paths_match_the_shell_list() {
    for repository in ["north", "south", "firn", "nixos-config", "smashcraft"] {
        let paths = durable_harness_paths(repository);
        let new = Observed {
            status: 0,
            stdout: paths.iter().map(|path| format!("{path}\n")).collect(),
            stderr: String::new(),
        };
        assert_same(repository, &shell("durable_harness_paths \"$1\"\n", &[repository]), &new);
    }
    assert!(durable_harness_paths("nixos-config").contains(&"modules/worker-ledger/".to_owned()));
}

fn git(repository: &Path, arguments: &[&str]) {
    let status = Command::new("git").arg("-C").arg(repository).args(arguments).status().unwrap();
    assert!(status.success(), "git {arguments:?} failed");
}

fn branch_fixture(scratch: &Path) -> std::path::PathBuf {
    let remote = scratch.join("remote.git");
    let repository = scratch.join("repo");
    git(scratch, &["init", "-q", "--bare", remote.to_str().unwrap()]);
    git(scratch, &["init", "-q", "-b", "main", repository.to_str().unwrap()]);
    git(&repository, &["-c", "user.name=t", "-c", "user.email=t@example.invalid", "commit", "-q", "--allow-empty", "-m", "initial"]);
    git(&repository, &["remote", "add", "origin", remote.to_str().unwrap()]);
    git(&repository, &["push", "-q", "-u", "origin", "main"]);
    repository
}

const ONE_BRANCH_MAIN_STEPS: [(&str, &[&str], bool); 6] = [
    ("main without origin HEAD", &["status", "--short"], true),
    ("new branch with no upstream", &["switch", "-q", "-c", "topic"], false),
    ("branch inheriting origin/main", &["branch", "-q", "--set-upstream-to=origin/main", "topic"], false),
    ("detached HEAD", &["switch", "-q", "--detach"], true),
    ("detached HEAD after origin HEAD moves", &["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/topic"], true),
    ("main when origin HEAD names another", &["switch", "-q", "main"], false),
];

#[test]
fn safe_push_one_branch_main_matches_the_shell_guard_on_real_repositories() {
    let scratch = tempfile::tempdir().unwrap();
    let repository = branch_fixture(scratch.path());
    for (shape, step, allowed) in ONE_BRANCH_MAIN_STEPS {
        git(&repository, step);
        let new = match one_branch_main(&repository) {
            Ok(()) => Observed { status: 0, stdout: String::new(), stderr: String::new() },
            Err(reason) => Observed { status: 1, stdout: String::new(), stderr: reason },
        };
        assert_eq!(new.status == 0, allowed, "verdict differs for {shape}");
        assert_same(shape, &shell_in(&repository, "enforce_one_branch_main\n", &[]), &new);
    }
    git(&repository, &["push", "-q", "origin", "main:topic"]);
    git(&repository, &["switch", "-q", "topic"]);
    assert_eq!(default_branch(&repository), "topic");
    assert_eq!(one_branch_main(&repository), Ok(()));
    assert_same("branch origin HEAD names", &shell_in(&repository, "enforce_one_branch_main\n", &[]), &Observed { status: 0, stdout: String::new(), stderr: String::new() });
}
