use std::os::unix::fs::PermissionsExt;

use super::*;

const SCANREUSE: &str = include_str!("fixtures/n842-scanreuse-2026-11-08.md");
const REUSE: &str = include_str!("fixtures/n842-reuse-2026-11-08.md");
const STOP_LINE: &str = include_str!("fixtures/proxy-stop-line-2026-11-07.md");

fn commands(claims: &[Claim]) -> Vec<Option<&str>> {
    claims.iter().map(|claim| claim.command.as_deref()).collect()
}

fn unix_ms(text: &str) -> u64 {
    u64::try_from(crate::foundation::clock::Timestamp::parse(text).unwrap().unix_millis()).unwrap()
}

struct UtcDay(u64);

impl Reckon for UtcDay {
    fn moment(&self, when: &When, _said_ms: u64) -> Option<u64> {
        match *when {
            When::Wall { hour, minute, .. } => Some(self.0 + (u64::from(hour) * 60 + u64::from(minute)) * 60_000),
            When::Utc { at_ms } => Some(at_ms),
        }
    }
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
fn a_report_keeps_only_the_promised_command_of_a_future_time_once_never_the_prose_between_spans_or_a_past_time() {
    let bin = bin_with(&["git"]);
    let claims = promised(SCANREUSE, 0, &Unreckoned, &resolver(bin.path(), bin.path().as_os_str()));
    assert_eq!(commands(&claims), [Some("git -C ~/code/north/main fetch -q origin && git -C ~/code/north/main merge-base --is-ancestor 83f275f3981d08d5e4fd3b0f7e559a279753c27d origin/main")]);
    assert_eq!(claims[0].when, When::Wall { hour: 9, minute: 36, date: None });
    let reuse = promised(REUSE, 0, &Unreckoned, &resolver(bin.path(), bin.path().as_os_str()));
    let landed = "git -C ~/code/north/main fetch -q origin && git -C ~/code/north/main log origin/main -n 60 --format=%s | rg -q 'Plan verdict reuse by recorded inputs'";
    assert_eq!(commands(&reuse), [Some(landed), Some(landed)], "on 2026-11-08 at 7:45 AM the tick ran this report's prose after `n842-reuse-plan` and sh stopped on the apostrophe in hosts'");
    let prose = promised("It lands by 9:40 AM: `It should be on origin/main` soon.\n```\nby 9:41 AM `git status`\n```\nStop: by 9:42 AM `git status`\n", 0, &Unreckoned, &resolver(bin.path(), bin.path().as_os_str()));
    assert_eq!(commands(&prose), [None], "a span whose first word is no program holds no promise, and fenced and Stop: lines are skipped");
    let said_ms = unix_ms("2026-11-13T09:30:00Z");
    let clock = UtcDay(unix_ms("2026-11-13T00:00:00Z"));
    let report = "Ran it over ssh at 8:56 AM, it exited 0: `git status`\nLanded at 2026-11-13T08:00Z: `git status`\nIt lands by 9:36 AM: `git log -1`\nDue 2026-11-13T09:50Z: `git diff`\n";
    let kept = promised(report, said_ms, &clock, &|_| true);
    assert_eq!(commands(&kept), [Some("git log -1"), Some("git diff")], "a time at or before the report was written is a statement of the past, not a promise");
    let bare = promised("Check `north` yourself by 9:50 AM.\nTurns left: 3. Focus: wait for the 9:40 AM `git status` to land, then rebase.\n", 0, &Unreckoned, &|_| true);
    assert_eq!(commands(&bare), [None], "a bare program name is no whole command, and a checkpoint's focus note is no report");
    let listing = format!("First sentence holds no time. It lands by 9:40 AM: `git log -1`; {} then more.", "x".repeat(2000));
    let quoted = promised(&listing, 0, &Unreckoned, &|_| true);
    assert_eq!(quoted[0].line, "It lands by 9:40 AM: `git log -1`;", "a note quotes the promise's own sentence, never the listing around it");
    let scratch = tempfile::tempdir().unwrap();
    let book = Book::at(scratch.path());
    let turns: Vec<Promise> = (1..=3).map(|turn| Promise { session: "n842-reuse".into(), turn, line: kept[0].line.clone(), command: "git log -1".into(), when: kept[0].when.clone(), said_ms, parent: "burn-lead".into() }).collect();
    book.keep(&turns).unwrap();
    let open = book.open();
    assert_eq!(open.len(), 1, "one promise repeated over three turns is paged once");
    book.checked(&[open[0].key()]).unwrap();
    assert!(book.open().is_empty());
    book.keep(&[Promise { parent: "proxy".into(), session: "other".into(), ..turns[0].clone() }]).unwrap();
    assert!(book.open().is_empty(), "a promise-due note goes to the reporting session's lead, never the coordinator");
}

#[test]
fn a_quoted_shell_builtin_is_no_program_so_a_reported_wait_until_holds_no_promise() {
    let bin = bin_with(&["git"]);
    let claims = promised(STOP_LINE, 0, &Unreckoned, &resolver(bin.path(), bin.path().as_os_str()));
    assert!(claims.iter().any(|claim| claim.line.contains("`wait until 2026-11-08T03:30Z`")));
    assert!(commands(&claims).iter().all(Option::is_none), "{:?}", commands(&claims));
}

#[test]
fn a_box_line_yields_only_its_done_when_command_not_the_backticks_in_its_prose() {
    let reply = "- [ ] (not before 2026-11-09T13:00Z) The `agent text reads` fall from 74 lines to 10, each `north fleet need` replaced. Done when: `cargo test --test it agent_text_reads` passes.\nLanded by 2026-11-09T13:00Z, see `north fleet list` and `git log -1 origin/main`\n";
    assert_eq!(commands(&promised(reply, 0, &Unreckoned, &|_| true)), [Some("cargo test --test it agent_text_reads"), Some("git log -1 origin/main")]);
}
