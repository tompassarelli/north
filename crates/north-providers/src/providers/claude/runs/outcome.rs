use std::sync::LazyLock;

use regex::Regex;

const HEAD_CHARS: usize = 400;

enum Scope {
    Head,
    Opener,
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
const MAIN: &str = r"\b(on|ancestor of) `?(origin/)?main\b";
const FINISHED: &str = r"(#\d+\W{0,3}(is |was |now )?closed|\bclosed #\d|\bissue\b[^.\n]{0,40}\bclosed\b|\b(is|are|now|all|been|boxes) ticked\b|\bticked (its|the|every|all|both|each)\b|\b(is|are) (now )?(closed|done|fixed)\b)";
const LANDED_PROSE: &str = r"\b(landed|is|are|now|sits|stays) (on|as|in)\b[^.\n]{0,40}\b(origin/)?main\b|\blanded (it |them )?as `?[0-9a-f]{7,}|\bis an ancestor of `?(origin/)?main";
const OPEN_PROSE: &str = r"\b(still not met|not met\b|left open|remains? open|still open|is unticked|not ticked|couldn.t|could not (land|finish)|did not land|didn.t land|still fails?|isn.t done|not yet|only just started|still (missing|pending))\b";
const FAILED: &[&str] = &[r"\b(fail(ed|s|ure)?|refused|error)\b"];
const WAITING: &[&str] = &[
    r"^\W*(waiting|still (running|deferred|waiting|queued)|i.ll (wait|pick|act|replace|start|score|run))",
    r"\b(will (report|notify)|is queued|are queued|holding the push|waiting (on|for|up to))\b",
];

static WAITS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\bdated checks?\b|\bdeferred (it |them |the box |its box |both boxes )?to .{0,60}\bcheck\b|\(waits: \d{4}-|stop:\s*wait until|\b(needs?|waits? (on|for)|waiting (on|for)) (a |an |the )?(full |clean |another )?(day|hour|week|\d+ (more )?(hours|days|landings|runs))\b|\b(day|hour|window) (isn.t|is not|hasn.t|has not) (up|over|passed|closed)|\blive (traffic|data|measurement)\b|\bwaits on the real (proxy|traffic)\b").unwrap()
});

static RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    let rule = |scope, quiet_only, patterns: &[&str], outcome| Rule {
        scope,
        quiet_only,
        pattern: Regex::new(&format!("(?i){}", patterns.join("|"))).unwrap(),
        outcome,
    };
    let landed = format!("(?s){MAIN}.*{FINISHED}|{FINISHED}.*{MAIN}");
    let landed_open = format!("(?s)(?:{LANDED_PROSE}).*(?:{OPEN_PROSE})|(?:{OPEN_PROSE}).*(?:{LANDED_PROSE})");
    vec![
        rule(Scope::Head, false, STOP, "idle"),
        rule(Scope::Head, false, &["(?-i:HANDOFF)"], "handoff"),
        rule(Scope::Head, false, &[r"^\W*split\b[.:]"], "split"),
        rule(Scope::Opener, false, &[r"(?-i:\bSplit\b)\W{0,2}:"], "split"),
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
        rule(Scope::Report, false, &[&landed_open], "not done"),
        rule(Scope::Report, false, &[LANDED_PROSE], "done"),
    ]
});

static STOP_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?im)^\W*stop\s*:[^\n]*").unwrap());

/// The report's last `Stop:` line, which the 600-character report the ledger keeps loses when the report is long.
pub fn stop_line(report: &str) -> Option<String> {
    STOP_LINE.find_iter(report).last().map(|found| found.as_str().trim().chars().take(200).collect())
}

/// Whether a report stops on a box deferred to a dated check (`north check experiments defer`) or waiting on stated elapsed time or live traffic.
pub fn waits_on_time(report: &str) -> bool {
    WAITS.is_match(report)
}

pub(super) fn classify(report: &str, quiet: bool) -> &'static str {
    let head: String = report.chars().take(HEAD_CHARS).collect();
    let matches = |rule: &Rule| match rule.scope {
        Scope::Head => rule.pattern.is_match(&head),
        Scope::Opener => rule.pattern.is_match(report.split("\n\n").next().unwrap_or_default()),
        Scope::Report => rule.pattern.is_match(report),
        Scope::FirstLineNotDone => rule.pattern.captures(report).is_some_and(|found| found.name("not").is_some()),
        Scope::FirstLineDone => rule.pattern.captures(report).is_some_and(|found| found.name("not").is_none()),
    };
    RULES.iter().filter(|rule| quiet || !rule.quiet_only).find(|rule| matches(rule)).map_or("unclear", |rule| rule.outcome)
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::{classify, waits_on_time};
    use crate::platform::run_roles::ingest_role;

    #[derive(Deserialize)]
    struct Labelled {
        report: String,
        quiet: bool,
        outcome: String,
        brief: Option<String>,
    }

    /// How the first-try bar reads the run: a reviewer is no worker, and a failed box waiting on time or traffic is deferred.
    fn first_try(labelled: &Labelled) -> &'static str {
        if labelled.brief.as_deref().is_some_and(|brief| ingest_role("research", brief, "unclear", 5) == "reviewer") {
            return "reviewer";
        }
        match classify(&labelled.report, labelled.quiet) {
            "not done" | "unclear" | "escalated" if waits_on_time(&labelled.report) => "deferred",
            outcome => outcome,
        }
    }

    #[test]
    fn at_most_two_of_thirty_seven_real_reports_from_2026_10_26_to_2026_11_06_are_labelled_wrong_and_none_that_the_first_try_bar_leaves_out() {
        let reports: Vec<Labelled> = serde_json::from_str(include_str!("reports.json")).unwrap();
        assert_eq!(reports.len(), 37);
        let wrong: Vec<(&str, &str)> = reports.iter().map(|labelled| (first_try(labelled), labelled.outcome.as_str())).filter(|(got, want)| got != want).collect();
        assert!(wrong.len() <= 2, "{wrong:?}");
        assert!(wrong.iter().all(|(got, want)| ![*got, *want].iter().any(|outcome| ["reviewer", "deferred", "split"].contains(outcome))), "{wrong:?}");
    }
}
