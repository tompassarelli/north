use std::cell::RefCell;
use std::path::{Path, PathBuf};

use super::file::{self, Filed, Issues};
use super::read;
use crate::ledger::outcome::{Landing, Outcome};
use super::registry::{self, Day, Experiment, Issue, Metric};
use super::rules;
use super::verdict::{Reading, Recommend, Verdict, judge};
use super::judge_due;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

const ENTRY: &str = r#"
[[experiment]]
id = "probe"
change = "a probe change"
landed = "abcdef1"
metric = "rework_rate"
direction = "down"
bar = 0.05
min_n = 4
before = ["2026-10-01", "2026-10-11"]
after = ["2026-10-11", "2026-10-21"]
review = "2026-10-23"
owner = "tompassarelli/north#9"
"#;

fn entry(edit: &str) -> Experiment {
    let text = format!("{ENTRY}{edit}");
    let mut entries = registry::parse(&text).unwrap_or_else(|problems| panic!("{problems:?}"));
    entries.remove(0)
}

fn day(text: &str) -> i64 {
    Day::parse(text).unwrap().unix()
}

const NOW: i64 = 1_800_000_000;

fn measure(outcomes: &[Outcome], metric: &Metric, scope: &super::registry::Scope, window: super::registry::Window) -> Option<Reading> {
    read::measure(outcomes, metric, scope, window, NOW)
}

fn landed(at: &str, reworked: Option<bool>) -> Outcome {
    let landing = Landing { sha: "abcdef1234".into(), at: day(at) + 3600, first_commit: Some(day(at) - 1800), reworked };
    Outcome { agent: "w".into(), tier: "sonnet".into(), arm: "sonnet-high".into(), ended: day(at), usd: 2.0, landing: Some(landing) }
}

fn cohort(start: &str, reworked: usize, kept: usize) -> Vec<Outcome> {
    (0..reworked).map(|_| landed(start, Some(true))).chain((0..kept).map(|_| landed(start, Some(false)))).collect()
}

fn replay(rows: &[Outcome], entry: &Experiment) -> Verdict {
    let before = entry.before.and_then(|window| measure(rows, &entry.metric, &entry.scope, window));
    let after = measure(rows, &entry.metric, &entry.scope, entry.after);
    judge(entry, before, after).verdict
}

#[test]
fn the_registry_quotes_every_review_rule_in_the_rationale_notes() {
    assert_eq!(rules::check(&repo()), Vec::<String>::new());
    let entries = registry::parse(&std::fs::read_to_string(repo().join(registry::PATH)).unwrap()).unwrap();
    let ids: Vec<&str> = entries.iter().map(|entry| entry.id.as_str()).collect();
    for required in ["failing-first", "tier-arm-sonnet", "tier-arm-opus", "flake-gate", "enqueue-gates", "lander-default"] {
        assert!(ids.contains(&required), "{required} missing from {ids:?}");
    }
    let failing = entries.iter().find(|entry| entry.id == "failing-first").unwrap();
    assert_eq!((failing.review.text().as_str(), failing.owner.text().as_str()), ("2026-11-18", "tompassarelli/north#524"));
}

#[test]
fn rules_are_overturned_paragraphs_and_review_sections() {
    let note = "# Note\n\nOverturned by: one\nthing.\n\nprose\n\n## Review\n\nTwo weeks\nlater.\n\n## Next\n\nOverturned by is prose here.\n";
    let found = rules::rules("docs/rationale/n.md", note);
    let texts: Vec<(usize, &str)> = found.iter().map(|rule| (rule.line, rule.text.as_str())).collect();
    assert_eq!(texts, vec![(3, "Overturned by: one thing."), (8, "Two weeks later.")]);
}

#[test]
fn a_changed_or_new_rule_without_its_entry_is_refused_and_a_stale_quote_too() {
    let quoting = |rule: &str| entry(&format!("note = \"docs/rationale/n.md\"\nrule = \"{rule}\"\n"));
    let rule = |text: &str| rules::Rule { note: "docs/rationale/n.md".into(), line: 3, text: text.into() };
    assert!(rules::uncovered(&[rule("Overturned by: one thing.")], &[quoting("Overturned by: one  thing.")]).is_empty());
    let refused = rules::uncovered(&[rule("Overturned by: two things.")], &[quoting("Overturned by: one thing.")]);
    assert_eq!(refused.len(), 2, "{refused:?}");
    assert!(refused[0].contains("docs/rationale/n.md:3: review rule has no entry"), "{refused:?}");
    assert!(refused[1].contains("entry probe quotes a rule"), "{refused:?}");
    assert_eq!(rules::uncovered(&[rule("Overturned by: new.")], &[]).len(), 1);
}

#[test]
fn the_registry_refuses_entries_it_could_not_judge() {
    let bad = |from: &str, to: &str| registry::parse(&ENTRY.replace(from, to)).unwrap_err().join("\n");
    assert!(bad("rework_rate", "rework").contains("unknown metric rework"));
    assert!(bad("metric = \"rework_rate\"", "metric = \"none\"").contains("needs a measure"));
    assert!(bad("2026-10-23", "2026-10-22").contains("at least 48 h after"));
    assert!(bad("abcdef1", "HEAD").contains("not a commit sha"));
    assert!(bad("north#9", "north").contains("owner/repo#N"));
    assert!(bad("0.05", "-1.0").contains("bar must be"));
    assert!(bad("[\"2026-10-01\", \"2026-10-11\"]", "[\"2026-10-01\", \"2026-10-12\"]").contains("before must end"));
    assert!(registry::parse(&format!("{ENTRY}{ENTRY}")).unwrap_err()[0].contains("appears twice"));
}

#[test]
fn replay_keeps_a_change_that_moved_past_the_bar() {
    let rows = [cohort("2026-10-02", 4, 6), cohort("2026-10-12", 1, 9)].concat();
    assert_eq!(replay(&rows, &entry("")), Verdict::Keep);
}

#[test]
fn replay_reverts_a_change_that_moved_the_other_way_past_the_bar() {
    let rows = [cohort("2026-10-02", 1, 9), cohort("2026-10-12", 4, 6)].concat();
    assert_eq!(replay(&rows, &entry("")), Verdict::Revert);
}

#[test]
fn replay_brings_a_move_inside_the_bar_to_tom_with_a_recommendation() {
    let toward = [cohort("2026-10-02", 3, 7), cohort("2026-10-12", 3, 8)].concat();
    assert_eq!(replay(&toward, &entry("")), Verdict::Decide(Recommend::Keep));
    let flat = [cohort("2026-10-02", 2, 8), cohort("2026-10-12", 2, 8)].concat();
    assert_eq!(replay(&flat, &entry("")), Verdict::Decide(Recommend::Revert));
}

#[test]
fn replay_re_measures_a_missing_or_thin_metric_and_never_guesses() {
    let unlanded = [cohort("2026-10-02", 2, 8), vec![Outcome { landing: None, ..landed("2026-10-12", None) }; 6]].concat();
    let Verdict::ReMeasure(why) = replay(&unlanded, &entry("")) else { panic!("judged without an after reading") };
    assert!(why.contains("rework_rate has no value for the after window 2026-10-11 to 2026-10-21 (n=0)"), "{why}");
    let thin = [cohort("2026-10-02", 2, 8), cohort("2026-10-12", 0, 3)].concat();
    let Verdict::ReMeasure(why) = replay(&thin, &entry("")) else { panic!("judged on n=3") };
    assert!(why.contains("n=3") && why.contains("min_n 4"), "{why}");
    let unread = registry::parse(&ENTRY.replace("metric = \"rework_rate\"", "metric = \"none\"\nmeasure = \"a blinded A/B\"")).unwrap().remove(0);
    let Verdict::ReMeasure(why) = replay(&thin, &unread) else { panic!("judged an unread metric") };
    assert!(why.contains("judged by a blinded A/B"), "{why}");
    assert_eq!(judge(&entry(""), None, Some(Reading { value: Some(0.1), n: 9 })).verdict.word(), "re-measure");
}

#[test]
fn a_count_entry_without_a_before_window_is_judged_on_its_after_count_and_scope() {
    let arm = ENTRY.replace("metric = \"rework_rate\"", "metric = \"runs\"\narm = \"sonnet-high\"").replace("direction = \"down\"", "direction = \"up\"").replace("bar = 0.05", "bar = 15.0").replace("min_n = 4", "min_n = 0").replace("before = [\"2026-10-01\", \"2026-10-11\"]\n", "");
    let arm = registry::parse(&arm).unwrap().remove(0);
    let other = Outcome { arm: "worker".into(), ..landed("2026-10-12", Some(false)) };
    let rows: Vec<Outcome> = std::iter::repeat_n(landed("2026-10-12", Some(false)), 15).chain(std::iter::repeat_n(other, 20)).collect();
    assert_eq!(replay(&rows, &arm), Verdict::Keep);
    assert_eq!(replay(&rows[..9], &arm), Verdict::Decide(Recommend::Keep));
    assert_eq!(measure(&rows, &Metric::FirstCommitP50Minutes, &arm.scope, arm.after), Some(Reading { value: Some(90.0), n: 15 }));
    assert_eq!(measure(&rows, &Metric::UsdPerSurviving, &arm.scope, arm.after), Some(Reading { value: Some(2.0), n: 15 }));
}

#[derive(Default)]
struct Fake {
    body: RefCell<String>,
    open: RefCell<bool>,
    edits: RefCell<usize>,
    reopened: RefCell<usize>,
}

impl Issues for Fake {
    fn body(&self, _: &Issue) -> Result<(String, bool), String> {
        Ok((self.body.borrow().clone(), *self.open.borrow()))
    }

    fn set_body(&self, _: &Issue, body: &str) -> Result<(), String> {
        *self.body.borrow_mut() = body.to_owned();
        *self.edits.borrow_mut() += 1;
        Ok(())
    }

    fn reopen(&self, _: &Issue) -> Result<(), String> {
        *self.open.borrow_mut() = true;
        *self.reopened.borrow_mut() += 1;
        Ok(())
    }
}

fn scratch(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("north-experiments-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    directory.join("filed")
}

#[test]
fn each_verdict_is_filed_once_and_a_left_open_box_reopens_its_issue() {
    let issues = Fake { body: RefCell::new("Done when:\n- [x] adopt".into()), ..Fake::default() };
    let rows = [cohort("2026-10-02", 1, 9), cohort("2026-10-12", 4, 6)].concat();
    let entries = [entry("")];
    let path = scratch("once");
    let mut filed = Filed::load(path.clone());
    let (lines, failed) = judge_due(&entries, Day::parse("2026-10-23").unwrap(), &rows, Some((&issues as &dyn Issues, &mut filed)));
    assert!(!failed, "{lines:?}");
    let body = issues.body.borrow().clone();
    assert!(body.starts_with("Done when:\n- [x] adopt\n- [ ] Revert abcdef1 (a probe change): rework_rate over every worker run: 0.100 (n=10, 2026-10-01 to 2026-10-11) -> 0.400 (n=10, 2026-10-11 to 2026-10-21), moved -0.300"), "{body}");
    assert!(body.contains("origin experiment probe <!-- experiment probe@2026-10-23 -->"), "{body}");
    assert_eq!((*issues.edits.borrow(), *issues.reopened.borrow()), (1, 1));
    assert!(Filed::load(path).has(&entries[0]));
    let mut again = Filed::load(scratch("again"));
    let (lines, _) = judge_due(&entries, Day::parse("2026-10-24").unwrap(), &rows, Some((&issues as &dyn Issues, &mut again)));
    assert!(lines.iter().any(|line| line == "probe: already on tompassarelli/north#9"), "{lines:?}");
    assert_eq!(*issues.edits.borrow(), 1);
    let (lines, _) = judge_due(&entries, Day::parse("2026-10-22").unwrap(), &rows, None);
    assert!(lines.is_empty(), "judged before its review day: {lines:?}");
}

#[test]
fn keep_is_a_ticked_box_and_decide_is_one_line_for_tom() {
    let issues = Fake { open: RefCell::new(false), ..Fake::default() };
    let keep = [cohort("2026-10-02", 4, 6), cohort("2026-10-12", 1, 9)].concat();
    let mut filed = Filed::load(scratch("keep"));
    judge_due(&[entry("")], Day::parse("2026-10-23").unwrap(), &keep, Some((&issues as &dyn Issues, &mut filed)));
    assert!(issues.body.borrow().contains("- [x] Keep probe (abcdef1)"), "{}", issues.body.borrow());
    assert_eq!(*issues.reopened.borrow(), 0);
    let flat = [cohort("2026-10-02", 2, 8), cohort("2026-10-12", 2, 8)].concat();
    let decided = judge(&entry(""), measure(&flat, &Metric::ReworkRate, &Default::default(), entry("").before.unwrap()), measure(&flat, &Metric::ReworkRate, &Default::default(), entry("").after));
    let line = file::line(&entry(""), None, None, &decided);
    assert!(line.starts_with("- [ ] Tom decides probe (a probe change):") && line.contains("recommend revert"), "{line}");
}

