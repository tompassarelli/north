use super::*;
use std::fs;
use std::process::Command;

const PLAN: &str = "\
# Plan

## 1. Evidence

Counts only, so no item here.

## 2. Design

### 2.1 The sender

`Sender` is an enum.

Enforced by: types. A timer cannot pose as Tom.

### 2.2 The entry point

- Rejected, a hook adoption: messages arrive only when a hook fires.

Enforced by: code, plus config.

## 3. Rules

- `one` is placed. Enforced by the guard (hook).
- `two` is merged. Text only.

## 5. Boxes

1. **A1 the base.** feature; Files: `src/a.rs`, `src/b.rs`. Needs: nothing. Done when:
   it passes.
2. **B1 the follower.** feature; Files: `src/a.rs`. Needs: A1. Done when: it passes.
3. **C1 the stray.** feature; Files: `src/b.rs`,
   `src/c.rs`. Needs: nothing. Done when: it passes.
";

fn kinds(items: &[Item], kind: Kind) -> Vec<&Item> {
    items.iter().filter(|item| item.kind == kind).collect()
}

#[test]
fn scenarios_go_to_the_other_family_and_architecture_to_opus_at_a_tier_the_author_is_not() {
    assert_eq!(Tier::OpusMedium.scenario_reviewer(), Tier::SonnetHigh);
    assert_eq!(Tier::OpusMedium.architecture_reviewer(), Tier::OpusHigh);
    assert_eq!(Tier::OpusHigh.scenario_reviewer(), Tier::SonnetHigh);
    assert_eq!(Tier::OpusHigh.architecture_reviewer(), Tier::OpusMedium);
    assert_eq!(Tier::SonnetHigh.scenario_reviewer(), Tier::OpusMedium);
    assert_eq!(Tier::SonnetHigh.architecture_reviewer(), Tier::OpusMedium);
    assert_eq!(Tier::parse("claude:opus-high"), Some(Tier::OpusHigh));
    assert_eq!(Tier::parse("haiku"), None);
}

#[test]
fn each_enforced_by_claim_is_a_scenario_and_each_part_making_one_an_architecture_item_with_a_lens() {
    let items = design_items(PLAN, Tier::OpusMedium);
    let architecture: Vec<(&str, Handler)> = kinds(&items, Kind::Architecture).iter().map(|item| (item.subject.as_str(), item.handler)).collect();
    let decomposition = Handler::Agent { tier: Tier::OpusHigh, lens: Some(Lens::Decomposition) };
    let simpler = Handler::Agent { tier: Tier::OpusHigh, lens: Some(Lens::SimplerAlternative) };
    assert_eq!(architecture, [("2.1 The sender", decomposition), ("2.2 The entry point", simpler), ("3. Rules", decomposition)]);
    let scenarios: Vec<(usize, &str)> = kinds(&items, Kind::Scenario).iter().map(|item| (item.line, item.subject.as_str())).collect();
    assert_eq!(
        scenarios,
        [
            (13, "2.1 The sender: Enforced by: types. A timer cannot pose as Tom."),
            (19, "2.2 The entry point: Enforced by: code, plus config."),
            (23, "3. Rules: Enforced by the guard (hook)."),
        ]
    );
    assert!(kinds(&items, Kind::Scenario).iter().all(|item| item.handler == Handler::Agent { tier: Tier::SonnetHigh, lens: None }));
}

#[test]
fn boxes_sharing_a_file_hold_when_needs_orders_them_and_fail_when_they_may_run_in_parallel() {
    let facts: Vec<(Handler, String)> = overlap_facts(PLAN).into_iter().map(|item| (item.handler, item.subject)).collect();
    assert_eq!(
        facts,
        [
            (Handler::Holds, "boxes A1 and B1 both change src/a.rs; B1 needs A1".to_owned()),
            (Handler::Fails, "boxes A1 and C1 both change src/b.rs and no Needs: orders them".to_owned()),
        ]
    );
}

#[test]
fn a_cited_verb_holds_only_when_the_cli_defines_it() {
    let text = "Run `north fleet launch` and `north proxy`, then `north fleet show`.\n";
    let facts: Vec<(Handler, String)> = verb_facts("docs/plan.md", text, |command| command == "fleet").into_iter().map(|item| (item.handler, item.subject)).collect();
    assert_eq!(facts, [(Handler::Holds, "verb north fleet: defined".to_owned()), (Handler::Fails, "verb north proxy: not a defined subcommand".to_owned())]);
}

fn repository(files: &[(&str, &str)]) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    for (path, text) in files {
        let path = root.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    for args in [vec!["init", "-q"], vec!["add", "-A"]] {
        assert!(Command::new("git").arg("-C").arg(root.path()).args(args).status().unwrap().success());
    }
    root
}

#[test]
fn a_cited_path_holds_when_tracked_and_a_cited_line_only_when_the_file_reaches_it() {
    let plan = "See `src/a.rs:2`, `src/a.rs:9`, `src/gone.rs` and [b](b.md).\n";
    let root = repository(&[("docs/plan.md", plan), ("docs/b.md", "b\n"), ("src/a.rs", "one\ntwo\nthree\n")]);
    let facts: Vec<(Handler, String)> = path_facts(root.path(), "docs/plan.md", plan).unwrap().into_iter().map(|item| (item.handler, item.subject)).collect();
    assert_eq!(
        facts,
        [
            (Handler::Holds, "path b.md: tracked".to_owned()),
            (Handler::Holds, "path src/a.rs:2: line 2 of 3".to_owned()),
            (Handler::Fails, "path src/a.rs:9: line 9 of 3".to_owned()),
            (Handler::Fails, "path src/gone.rs: not tracked".to_owned()),
        ]
    );
}

#[test]
fn the_dispatch_prints_facts_first_with_results_then_the_residue_with_its_tier() {
    let root = repository(&[("docs/plan.md", PLAN), ("src/a.rs", "a\n"), ("src/b.rs", "b\n")]);
    let items = dispatch(root.path(), "docs/plan.md", Tier::OpusMedium, |_| true).unwrap();
    let lines = render("docs/plan.md", Tier::OpusMedium, &items);
    assert_eq!(lines[0], "review items docs/plan.md, author opus-medium: 5 fact (2 fail, checked here), 3 scenario -> sonnet-high, 3 architecture -> opus-high");
    let kinds: Vec<&str> = lines[1..].iter().map(|line| line.split(' ').next().unwrap()).collect();
    assert_eq!(kinds, ["fact", "fact", "fact", "fact", "fact", "scenario", "scenario", "scenario", "architecture", "architecture", "architecture"]);
    assert!(lines.contains(&"fact FAILS L32: path src/c.rs: not tracked".to_owned()), "{lines:#?}");
    assert!(lines.contains(&"architecture opus-high lens simpler-alternative L15: 2.2 The entry point".to_owned()), "{lines:#?}");
    assert!(lines.contains(&"scenario sonnet-high L13: 2.1 The sender: Enforced by: types. A timer cannot pose as Tom.".to_owned()), "{lines:#?}");
}

#[test]
fn the_command_needs_a_plan_and_a_known_author_tier() {
    assert_eq!(run(&[], |_| true), 2);
    assert_eq!(run(&["docs/plan.md".into(), "--author".into(), "haiku".into()], |_| true), 2);
    let root = repository(&[("docs/plan.md", PLAN)]);
    let arguments = ["docs/plan.md".to_owned(), "--root".into(), root.path().display().to_string(), "--author".into(), "sonnet-high".into()];
    assert_eq!(run(&arguments, |_| true), 0);
}

const SCHEMA: &str = "pub const SCHEMA: &str = \"CREATE TABLE IF NOT EXISTS runs(agent TEXT NOT NULL, task_ref TEXT) STRICT;\";\n";

#[test]
fn a_cited_metric_source_holds_only_when_a_tracked_schema_creates_its_table_and_names_its_column() {
    let plan = "\
The landed share reads `runs.task_ref` from the threads `runs` table.
Each finding lands in the `review_findings` table and `runs.reviewer` names who read it.
`flake.nix`, `runs.rs` and `land.check` are files, not metrics.
```
the `ghost` table inside a fence is not cited
```
";
    let root = repository(&[("docs/plan.md", plan), ("crates/ledger/src/schema.rs", SCHEMA)]);
    let facts: Vec<(Handler, String)> = metric_facts(root.path(), plan).unwrap().into_iter().map(|item| (item.handler, item.subject)).collect();
    assert_eq!(
        facts,
        [
            (Handler::Holds, "metric source table runs: created in crates/ledger/src/schema.rs".to_owned()),
            (Handler::Holds, "metric source runs.task_ref: column named in crates/ledger/src/schema.rs".to_owned()),
            (Handler::Fails, "metric source table review_findings: no tracked schema creates it".to_owned()),
            (Handler::Fails, "metric source runs.reviewer: no schema of runs names this column".to_owned()),
        ]
    );
}

#[test]
fn a_plan_citing_a_missing_metric_source_fails_that_fact_item_in_the_dispatch_and_an_existing_one_passes() {
    let missing = "## Measure\n\nThe gain is read from the `review_findings` table.\n";
    let present = "## Measure\n\nThe gain is read from the `runs` table.\n";
    for (plan, verdict, handler) in [(missing, "fact FAILS L3: metric source table review_findings: no tracked schema creates it", Handler::Fails), (present, "fact holds L3: metric source table runs: created in src/schema.rs", Handler::Holds)] {
        let root = repository(&[("docs/plan.md", plan), ("src/schema.rs", SCHEMA)]);
        let items = dispatch(root.path(), "docs/plan.md", Tier::OpusMedium, |_| true).unwrap();
        assert_eq!(items.iter().map(|item| item.handler).collect::<Vec<_>>(), [handler]);
        assert_eq!(render("docs/plan.md", Tier::OpusMedium, &items)[1], verdict);
    }
}
