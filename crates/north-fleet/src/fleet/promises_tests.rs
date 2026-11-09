use std::os::unix::fs::PermissionsExt;

use super::*;

const SCANREUSE: &str = include_str!("fixtures/n842-scanreuse-2026-11-08.md");
const STOP_LINE: &str = include_str!("fixtures/proxy-stop-line-2026-11-07.md");

fn commands(claims: &[Claim]) -> Vec<Option<&str>> {
    claims.iter().map(|claim| claim.command.as_deref()).collect()
}

fn bin_with(programs: &[&str]) -> tempfile::TempDir {
    let bin = tempfile::tempdir().unwrap();
    for program in programs {
        let path = bin.path().join(program);
        fs::write(&path, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    bin
}

#[test]
fn a_report_quoting_a_path_before_its_promise_runs_only_the_promised_command_never_the_prose_between_spans() {
    let bin = bin_with(&["git"]);
    let claims = promised(SCANREUSE, &resolver(bin.path(), bin.path().as_os_str()));
    assert_eq!(commands(&claims), [Some("git -C ~/code/north/main fetch -q origin && git -C ~/code/north/main merge-base --is-ancestor 83f275f3981d08d5e4fd3b0f7e559a279753c27d origin/main")]);
    assert_eq!(claims[0].when, When::Wall { hour: 9, minute: 36, date: None });
    let prose = promised("It lands by 9:40 AM: `It should be on origin/main` soon.\n```\nby 9:41 AM `git status`\n```\nStop: by 9:42 AM `git status`\n", &resolver(bin.path(), bin.path().as_os_str()));
    assert_eq!(commands(&prose), [None], "a span whose first word is no program holds no promise, and fenced and Stop: lines are skipped");
}

#[test]
fn a_quoted_shell_builtin_is_no_program_so_a_reported_wait_until_holds_no_promise() {
    let bin = bin_with(&["git"]);
    let claims = promised(STOP_LINE, &resolver(bin.path(), bin.path().as_os_str()));
    assert!(claims.iter().any(|claim| claim.line.contains("`wait until 2026-11-08T03:30Z`")));
    assert!(commands(&claims).iter().all(Option::is_none), "{:?}", commands(&claims));
}

#[test]
fn a_box_line_yields_only_its_done_when_command_not_the_backticks_in_its_prose() {
    let reply = "- [ ] (not before 2026-11-09T13:00Z) The `agent text reads` fall from 74 lines to 10, each `north fleet need` replaced. Done when: `cargo test --test it agent_text_reads` passes.\nLanded by 2026-11-09T13:00Z, see `north fleet list` and `git log -1 origin/main`\n";
    assert_eq!(commands(&promised(reply, &|_| true)), [Some("cargo test --test it agent_text_reads"), Some("git log -1 origin/main")]);
}
