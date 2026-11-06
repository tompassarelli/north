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

}
