use std::path::Path;
use std::process::Command;

use super::*;

struct Observed {
    status: i32,
    stdout: String,
    stderr: String,
}

fn shell_functions() -> String {
    let script = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bin/safe-push")).unwrap();
    let mut functions = String::new();
    for name in ["usage", "argument_error", "plan_push", "default_branch"] {
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
    let stub = tempfile::tempdir().unwrap();
    let usage = stub.path().join("usage.txt");
    std::fs::write(&usage, USAGE).unwrap();
    let north = stub.path().join("north");
    north_base::foundation::shim::write_script(&north, &format!("[ \"$*\" = \"safe-push --help\" ] && exec cat '{}'\nexit 64\n", usage.display()));
    let path = format!("{}:{}", stub.path().display(), std::env::var("PATH").unwrap_or_default());
    let output = Command::new("bash")
        .env("PATH", path)
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
        "{}|{}|{}|{}|{}|{}|{}|{}|{}\n",
        flag(arguments.dry_run),
        flag(arguments.keep_lane),
        flag(arguments.direct),
        flag(arguments.tag.is_some()),
        arguments.tag.as_deref().unwrap_or(""),
        flag(arguments.to.is_some()),
        arguments.to.as_deref().unwrap_or(""),
        flag(arguments.delete_tag.is_some()),
        arguments.delete_tag.as_deref().unwrap_or(""),
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
    printf '%s|%s|%s|%s|%s|%s|%s|%s|%s\\n' \"$dry\" \"$keep_lane\" \"$direct\" \"$tag_seen\" \"$tag\" \"$to_seen\" \"$to\" \"$delete_tag_seen\" \"$delete_tag\"\n";

#[test]
fn safe_push_refuses_destination_names_git_would_misread() {
    for invalid in ["HEAD", "refs/heads/main", "origin/main", "../main", "bad name", "bad..name", "a@{1}", "a~1", "a^", "a:b", "a?", "a*", "a[b", "a\\b"] {
        let arguments = ["--to", invalid];
        let new = rust_arguments(&arguments);
        assert_eq!(new.status, 2, "{invalid} was accepted");
        assert_same(invalid, &shell(PARSED_STATE, &arguments), &new);
    }
}
