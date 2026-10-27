use std::sync::LazyLock;

use regex::Regex;

const HEAD_CHARS: usize = 400;

enum Scope {
    Head,
    Report,
    FirstLineNotDone,
    FirstLineDone,
}

struct Rule {
    scope: Scope,
    quiet_only: bool,
    pattern: Regex,
    outcome: &'static str,
}

const STOP: &[&str] = &[r"^\W*(you.ve hit your|not logged in|you.ve reached your)"];
const NOTICE: &[&str] = &[
    r"\b(spawn hold|spawns are|spawns again|spawning (is|has)|agent spawning|agent hold|hold on (new|starting)|new hold|the hold|this hold|under the hold)\b",
    r"\b(usage hold|hold-spawns|resume-spawns|capacity notices|usage notices?|quota update|memory update|got the notice|received the pause|resume notice|these hold)\b",
    r"\b(nothing has changed|nothing new|no new task|not starting|nothing to restart|nothing (left )?to (do|resume|start|stop)|nothing for me|nothing of mine|no workers|this restart)\b",
    r"^\W*(noted|resume noted|no change|i.ve (seen|noted))\b",
];
const LINE: &[&str] = &[r"(?m)^\W*(?:(?P<not>(?:not|partly|mostly|half) done)|done\b[*\s]*[:.])"];
const REVIEW: &[&str] = &[
    r"^\W*(i )?(verdict|no defects|reviewed:|review of)\b",
    r"(?m)^\W*(\*\*)?([a-z0-9 ]{0,30}: )?(approve|approved|accept|reject)\b(\s{2,}|\s*[\n.:;,*—-])",
    r"\b(changes (needed|requested)|request changes)\b\W*(\n|$)",
];
const SCORED: &[&str] = &[r#"^\W*(i )?scored "#, r#"^\W*\{"look""#, r"\(a\) yes \(b\) no"];
const OPEN: &[&str] = &[
    r"(?m)^\W*stop:.*not done",
    r"\b(is not done|is still open|hasn.t (run|landed)|isn.t on|is not on|unticked|not landed|nothing landed|still fail(s|ing)?)\b",
];
const DECISION: &[&str] = &[r"\b(needs (tom|your)|need your call|waiting (on|for) (tom|your))\b"];
const STOP_OPEN: &[&str] = &[r"(?m)^\W*stop\W*:[^\n]*\b(not (done|landed)|nothing landed|unfinished)\b"];
const STOP_DONE: &[&str] = &[r"(?m)^\W*stop\W*:[^\n]{0,40}\b(done|goal met|landed)\b"];
const MAIN: &str = r"\bon (origin/)?main\b";
const CLOSED: &str = r"(#\d+\W{0,3}(is |was |now )?closed|\bclosed #\d|\bissue\b[^.\n]{0,40}\bclosed\b|\b(is|are|now|all|been|boxes) ticked\b)";
const FAILED: &[&str] = &[r"\b(fail(ed|s|ure)?|refused|error)\b"];
const WAITING: &[&str] = &[
    r"^\W*(waiting|still (running|deferred|waiting|queued)|i.ll (wait|pick|act|replace|start|score|run))",
    r"\b(will (report|notify)|is queued|are queued|holding the push|waiting (on|for|up to))\b",
];

static RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    let rule = |scope, quiet_only, patterns: &[&str], outcome| Rule {
        scope,
        quiet_only,
        pattern: Regex::new(&format!("(?i){}", patterns.join("|"))).unwrap(),
        outcome,
    };
    let landed = format!("(?s){MAIN}.*{CLOSED}|{CLOSED}.*{MAIN}");
    vec![
        rule(Scope::Head, false, STOP, "idle"),
        rule(Scope::Head, false, &["(?-i:HANDOFF)"], "handoff"),
        rule(Scope::Head, false, &[r"^\W*split\b[.:]"], "split"),
        rule(Scope::Head, false, &[r"^\W*(not|partly|mostly|half) done"], "not done"),
        rule(Scope::Head, false, &[r"^\W*blocked"], "escalated"),
        rule(Scope::Head, false, &[r"^\W*done\b"], "done"),
        rule(Scope::FirstLineNotDone, false, LINE, "not done"),
        rule(Scope::Head, true, NOTICE, "idle"),
        rule(Scope::FirstLineDone, false, LINE, "done"),
        rule(Scope::Report, false, STOP_OPEN, "not done"),
        rule(Scope::Report, false, STOP_DONE, "done"),
        rule(Scope::Head, false, OPEN, "not done"),
        rule(Scope::Report, false, &[&landed], "done"),
        rule(Scope::Head, false, FAILED, "unclear"),
        rule(Scope::Head, false, REVIEW, "done"),
        rule(Scope::Head, false, SCORED, "done"),
        rule(Scope::Head, false, DECISION, "not done"),
        rule(Scope::Head, false, WAITING, "idle"),
    ]
});

pub(super) fn classify(report: &str, quiet: bool) -> &'static str {
    let head: String = report.chars().take(HEAD_CHARS).collect();
    let matches = |rule: &Rule| match rule.scope {
        Scope::Head => rule.pattern.is_match(&head),
        Scope::Report => rule.pattern.is_match(report),
        Scope::FirstLineNotDone => rule.pattern.captures(report).is_some_and(|found| found.name("not").is_some()),
        Scope::FirstLineDone => rule.pattern.captures(report).is_some_and(|found| found.name("not").is_none()),
    };
    RULES.iter().filter(|rule| quiet || !rule.quiet_only).find(|rule| matches(rule)).map_or("unclear", |rule| rule.outcome)
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::classify;

    #[derive(Deserialize)]
    struct Labelled {
        report: String,
        quiet: bool,
        outcome: String,
    }

    #[test]
    fn at_least_eighteen_of_twenty_real_reports_from_2026_10_26_are_labelled_right() {
        let reports: Vec<Labelled> = serde_json::from_str(include_str!("reports.json")).unwrap();
        assert_eq!(reports.len(), 20);
        let wrong: Vec<(&str, &str)> =
            reports.iter().map(|labelled| (classify(&labelled.report, labelled.quiet), labelled.outcome.as_str())).filter(|(got, want)| got != want).collect();
        assert!(wrong.len() <= 2, "{wrong:?}");
    }

    #[test]
    fn a_prose_done_report_is_done_and_a_notice_acknowledgement_is_idle() {
        let labels = [
            ("The Cairne fix is on origin/main as 02865f8e6 and the issue is closed; it passes all five scorecard lines.", false, "done"),
            ("Noted the spawn hold. This session runs no workers, so nothing needs pausing.\n\nDone: smashcraft#239 is closed.", true, "idle"),
            ("You've hit your weekly limit · resets Oct 31, 12pm (UTC)", true, "idle"),
            ("Peak is 0.0152, just over the limit.", false, "unclear"),
            ("Done: the handoff hook fix landed on origin/main.", false, "done"),
            ("Pause-menu fix is on origin/main and #88 is closed; the pause menu now resumes input.", false, "done"),
            ("Desync fix landed on origin/main, #12 closed; replay shows no change in rollback count.", false, "done"),
            ("Waiting for Tom to approve the GPU spend; nothing landed.", false, "not done"),
            ("Acceptance on gex45 failed: cold read did not clear.", false, "unclear"),
            ("Your call: (a) retune the 10 fighters (b) leave them.", false, "unclear"),
            ("Reproduced the desync on main at a1ced; nothing landed, needs Tom's call.", false, "not done"),
            ("Scorecard lines 1–4 ticked on origin/main; cold judges still failing.", false, "not done"),
            ("Relay fix landed as abc.\n\nDone: closed #39.\nNot done: nothing.", false, "done"),
            ("Starting the pool failed after the rebase.", false, "unclear"),
            ("Noted.", true, "idle"),
            ("Tests are running; I will report.", false, "idle"),
            ("The fix is not on origin/main: safe-push refused it. I closed duplicate #90.", false, "not done"),
            ("Accept test on gex45 failed: cold read did not clear.", false, "unclear"),
            ("Need your call: (a) yes, rent gex45 for $40 (b) no.", false, "not done"),
            ("#497 is still open and nothing new landed; cold judges still fail.", false, "not done"),
            ("Tests fail, so I'm not starting the second pass.", false, "unclear"),
            ("Usage limit pacing is on origin/main as abc; #200 closed.", false, "done"),
            ("Waiting for Tom's call on the GPU spend.", false, "not done"),
            ("approve  patch-id reviewed: 9407563eec1a", false, "done"),
            ("Changes needed in wisp before this can land.", false, "unclear"),
            ("I re-ran the bench and nothing has changed: frame time is still 18 ms over budget.", false, "unclear"),
            ("cargo test panics on main, so I'm not starting the port.", false, "unclear"),
            ("Noted the hold.\n\nNot done: the build broke on the rebase.", true, "not done"),
            ("The server is running but the client cannot connect; the desync persists.", false, "unclear"),
            ("Could not reproduce the desync on main at a1ced; closed the two clients.", false, "unclear"),
            ("I won't rent gex45 until you approve.", false, "unclear"),
            ("Spawns are on hold again. The Cairne Shockwave fix (94d9193a0) is on origin/main, its issue (327) is closed.", false, "done"),
            ("Nothing new to do.\n\n**Done:** #255 is landed.", false, "done"),
            ("Nothing of mine is running.\n\nStop: batch landed; no work pending.", false, "done"),
            ("The relay fix landed as abc1234 after safe-push refused the first push on a stale lane.\n\nDone: #39 closed on origin/main.", false, "done"),
            ("The replay parser no longer fails on a zero-length frame. The fix is on origin/main as abc1234 and #412 is closed.", false, "done"),
            ("Landed on origin/main; #88 is closed. The follow-up needs your decision: retune or leave.", false, "done"),
            ("The rebase onto main conflicts in row.rs and I could not resolve it.\n\nStop: not done, the rebase conflict blocks landing.", false, "not done"),
            ("Fix written.\n\nStop: landing; the fix is not landed.", false, "not done"),
            ("The Shockwave fix failed its acceptance run on main at a1ced; both Warcraft clients are closed.", false, "unclear"),
            ("Box 1 is not ticked; the build failed on main.", false, "unclear"),
            ("Landed X.\n\n**Done**: closed #5.\n**Not done**: nothing.", false, "done"),
        ];
        let wrong: Vec<(&str, &str)> = labels.iter().map(|(report, quiet, want)| (classify(report, *quiet), *want)).filter(|(got, want)| got != want).collect();
        assert!(wrong.is_empty(), "{wrong:?}");
    }
}
