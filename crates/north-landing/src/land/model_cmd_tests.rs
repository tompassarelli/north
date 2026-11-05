use super::*;
use crate::land_model::Class;

fn lane(branch: &str, era: Era, class: Class, shared_files: u32) -> Lane {
    Lane { branch: branch.into(), era, class, queued: 0, decided: 0, shared_files, resubmit: false, lander_paths: false, history_refused: false, docs_only: false }
}

fn lanes() -> Vec<Lane> {
    vec![lane("a", Era::Precheck, Class::Conflict, 3), lane("b", Era::Precheck, Class::Landed, 0), lane("c", Era::Batch, Class::Landed, 1), lane("d", Era::Batch, Class::Failed, 0)]
}

fn parsed(arguments: &[&str]) -> Option<Options> {
    parse(&arguments.iter().map(|argument| (*argument).to_owned()).collect::<Vec<_>>())
}

#[test]
fn arguments_name_the_repo_era_base_and_json_and_refuse_anything_else() {
    let options = parsed(&["--repo", "/r", "--era", "precheck", "--base", "main", "--json"]).unwrap();
    assert_eq!((options.repo, options.era, options.base.as_str(), options.json), (PathBuf::from("/r"), Some(Era::Precheck), "main", true));
    let default = parsed(&[]).unwrap();
    assert_eq!((default.era, default.base.as_str(), default.json), (None, "origin/main", false));
    assert!(parsed(&["--era", "weekly"]).is_none());
    assert!(parsed(&["--repo"]).is_none());
    assert!(parsed(&["extra"]).is_none());
}

#[test]
fn one_report_per_target_era_and_scoring_in_a_fixed_order_and_an_era_flag_keeps_its_own() {
    let all = reports(&lanes(), None);
    assert_eq!(all.len(), 8);
    assert_eq!(all.iter().map(|report| (report.target.name(), report.era.name(), report.scoring.name())).take(3).collect::<Vec<_>>(), [("conflict", "batch", "leave-one-branch-out"), ("conflict", "batch", "prequential"), ("conflict", "precheck", "leave-one-branch-out")]);
    let precheck = reports(&lanes(), Some(Era::Precheck));
    assert_eq!(precheck.len(), 4);
    assert!(precheck.iter().all(|report| report.era == Era::Precheck));
}

#[test]
fn the_text_and_json_carry_the_counts_scores_and_cells_of_every_report() {
    let loaded = Loaded { lanes: lanes(), tickets: 5, skipped: 1, ledger_lines: 2 };
    let all = reports(&loaded.lanes, Some(Era::Precheck));
    let text = render(&loaded, &all);
    assert!(text.starts_with("north land model: 5 done tickets, 1 skipped without a resolvable head, 2 ledger lines\n"), "{text}");
    assert!(text.contains("conflict precheck leave-one-branch-out: n=2 base rate=0.500"), "{text}");
    assert!(text.contains("brier model=") && text.contains("log loss model=") && text.contains("cell shared 3+, first:"), "{text}");
    assert_eq!(text, render(&loaded, &reports(&loaded.lanes, Some(Era::Precheck))));
    let value = render_json(&loaded, &all);
    assert_eq!((value["tickets"].as_u64(), value["skipped"].as_u64()), (Some(5), Some(1)));
    assert_eq!(value["reports"].as_array().unwrap().len(), 4);
    assert_eq!(value["reports"][0]["rows"].as_u64(), Some(2));
}

#[test]
fn load_counts_a_done_ticket_whose_head_the_repository_lacks_as_skipped() {
    let repository = tempfile::tempdir().unwrap();
    let queue = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(queue.path().join("done")).unwrap();
    let ticket = queue::Ticket { branch: "lane".into(), head: "a".repeat(40), verdict: Some("landed abc".into()), ..Default::default() };
    queue::write(&queue.path().join("done").join(queue::file_name(1_700_000_000_000_000, "lane")), &ticket).unwrap();
    let loaded = load(queue.path(), repository.path(), "main").unwrap();
    assert_eq!((loaded.tickets, loaded.skipped, loaded.ledger_lines, loaded.lanes.len()), (1, 1, 0, 0));
}
