use super::buckets::{fit, Target, Weights};
use super::replay::{replay, Scoring};
use super::{Class, Era, Lane};

fn lane(branch: &str, era: Era, class: Class) -> Lane {
    Lane { branch: branch.into(), era, class, queued: 0, decided: 0, shared_files: 0, resubmit: false, lander_paths: false, history_refused: false, docs_only: false }
}

fn contested(branch: &str, class: Class) -> Lane {
    Lane { shared_files: 3, ..lane(branch, Era::Batch, class) }
}

fn close(left: f64, right: f64) {
    assert!((left - right).abs() < 1e-12, "{left} != {right}");
}

#[test]
fn conflict_cells_split_shared_files_into_zero_one_two_and_three_plus_by_first_or_resubmit() {
    let shared = |count, resubmit| Target::Conflict.cell(&Lane { shared_files: count, resubmit, ..lane("a", Era::Batch, Class::Landed) });
    assert_eq!([shared(0, false), shared(0, true), shared(1, false), shared(2, true), shared(3, false), shared(40, true)], [0, 1, 2, 3, 4, 5]);
    assert_eq!(Target::Conflict.cell_labels().len(), 6);
}

#[test]
fn pass_cells_hold_a_docs_only_cell_before_lander_paths_by_refused_history() {
    let cell = |docs_only, lander_paths, history_refused| Target::Pass.cell(&Lane { docs_only, lander_paths, history_refused, ..lane("a", Era::Batch, Class::Landed) });
    assert_eq!([cell(true, true, true), cell(false, false, false), cell(false, false, true), cell(false, true, false), cell(false, true, true)], [0, 1, 2, 3, 4]);
    assert_eq!(Target::Pass.cell_labels().len(), 5);
}

#[test]
fn replaced_tickets_are_not_conflict_rows_and_infra_tickets_are_not_pass_rows() {
    let replaced = lane("a", Era::Batch, Class::Replaced);
    let infra = lane("a", Era::Batch, Class::Infra);
    assert_eq!(Target::Conflict.outcome(&replaced), None);
    assert_eq!(Target::Conflict.outcome(&infra), Some(false));
    assert_eq!(Target::Pass.outcome(&infra), None);
    assert_eq!(Target::Pass.outcome(&replaced), None);
    assert_eq!(Target::Pass.outcome(&lane("a", Era::Batch, Class::Failed)), Some(false));
    assert_eq!(Target::Conflict.outcome(&lane("a", Era::Batch, Class::Conflict)), Some(true));
}

#[test]
fn a_cell_posterior_adds_two_pseudo_rows_at_the_pooled_base_rate() {
    let lanes = [contested("a", Class::Conflict), contested("b", Class::Landed), lane("c", Era::Batch, Class::Landed), lane("d", Era::Batch, Class::Landed)];
    let model = fit(Target::Conflict, Era::Batch, Weights::default(), lanes.iter());
    close(model.base, 2.0 / 6.0);
    close(model.cells[4].posterior, (1.0 + 2.0 / 3.0) / (2.0 + 2.0));
    close(model.cells[0].posterior, (0.0 + 2.0 / 3.0) / (2.0 + 2.0));
    close(model.cells[5].posterior, model.base);
}

#[test]
fn the_other_era_counts_a_quarter_in_the_cell_and_in_the_base_rate() {
    let lanes = [lane("a", Era::Precheck, Class::Conflict), lane("b", Era::Batch, Class::Landed), lane("c", Era::Batch, Class::Landed)];
    let model = fit(Target::Conflict, Era::Precheck, Weights::default(), lanes.iter());
    close(model.weight, 1.5);
    close(model.base, 2.0 / 3.5);
    let cell = &model.cells[0];
    close(cell.weight, 1.5);
    close(cell.events, 1.0);
    close(cell.posterior, (1.0 + 2.0 * 2.0 / 3.5) / (1.5 + 2.0));
}

#[test]
fn with_no_rows_every_cell_is_the_even_prior() {
    let model = fit(Target::Pass, Era::Precheck, Weights::default(), [].iter());
    assert!(model.cells.iter().all(|cell| (cell.posterior - 0.5).abs() < 1e-12));
}

#[test]
fn leave_one_branch_out_scores_each_of_two_twin_branches_without_its_twin() {
    let lanes = [contested("twin-a", Class::Conflict), contested("twin-b", Class::Conflict), lane("quiet", Era::Batch, Class::Landed)];
    let report = replay(&lanes, Target::Conflict, Era::Batch, Scoring::LeaveOneBranchOut, Weights::default());
    assert_eq!(report.rows, 3);
    let twin = (1.0 + 2.0 * 0.5) / (1.0 + 2.0);
    let quiet = 0.75;
    close(report.brier_model, (2.0 * (1.0_f64 - twin).powi(2) + quiet * quiet) / 3.0);
    close(report.brier_base, (2.0 * 0.25 + quiet * quiet) / 3.0);
    close(report.log_loss_model, (-2.0 * f64::ln(twin) - f64::ln(1.0 - quiet)) / 3.0);
    close(report.log_loss_base, (-2.0 * f64::ln(0.5) - f64::ln(1.0 - quiet)) / 3.0);
    let with_twin = fit(Target::Conflict, Era::Batch, Weights::default(), lanes.iter()).cells[4].posterior;
    assert!((with_twin - twin).abs() > 0.05);
}

#[test]
fn leave_one_branch_out_drops_every_ticket_of_the_scored_branch() {
    let lanes = [contested("again", Class::Conflict), contested("again", Class::Conflict), contested("again", Class::Landed), lane("other", Era::Batch, Class::Landed)];
    let report = replay(&lanes, Target::Conflict, Era::Batch, Scoring::LeaveOneBranchOut, Weights::default());
    let again = 1.0 / 3.0;
    let other = 3.0 / 5.0;
    close(report.brier_model, (2.0 * (1.0_f64 - again).powi(2) + again * again + other * other) / 4.0);
}

#[test]
fn prequential_trains_only_on_tickets_decided_before_the_scored_one_was_queued() {
    let mut early = contested("early", Class::Conflict);
    early.decided = 100;
    let mut late = contested("late", Class::Landed);
    late.queued = 150;
    late.decided = 200;
    let mut last = contested("last", Class::Landed);
    last.queued = 120;
    last.decided = 300;
    let lanes = [early, late, last];
    let report = replay(&lanes, Target::Conflict, Era::Batch, Scoring::Prequential, Weights::default());
    let after_early = (1.0 + 2.0 * (2.0 / 3.0)) / (1.0 + 2.0);
    close(report.brier_model, (0.25 + 2.0 * after_early * after_early) / 3.0);
}

#[test]
fn deciles_keep_rows_with_equal_predictions_together_so_few_cells_make_few_groups() {
    let mut lanes = Vec::new();
    for index in 0..30 {
        let class = if index % 3 == 0 { Class::Conflict } else { Class::Landed };
        lanes.push(Lane { shared_files: [0, 1, 3][index % 3], ..lane(&format!("branch-{index}"), Era::Batch, class) });
    }
    let report = replay(&lanes, Target::Conflict, Era::Batch, Scoring::LeaveOneBranchOut, Weights::default());
    assert!(report.deciles.len() <= 3, "{:?}", report.deciles);
    assert_eq!(report.deciles.iter().map(|decile| decile.rows).sum::<usize>(), 30);
    assert!(report.deciles.windows(2).all(|pair| pair[0].mean_predicted <= pair[1].mean_predicted));
}

#[test]
fn deciles_split_distinct_predictions_into_ten_near_equal_groups() {
    let lanes: Vec<Lane> = (0..100)
        .map(|index| Lane { queued: index, ..Lane { shared_files: 0, resubmit: index % 2 == 0, ..lane(&format!("b{index}"), Era::Batch, if index % 7 == 0 { Class::Conflict } else { Class::Landed }) } })
        .collect();
    let report = replay(&lanes, Target::Conflict, Era::Batch, Scoring::LeaveOneBranchOut, Weights::default());
    assert!(report.deciles.iter().all(|decile| decile.rows >= 1));
    assert_eq!(report.deciles.iter().map(|decile| decile.rows).sum::<usize>(), 100);
}

#[test]
fn a_report_has_the_same_numbers_on_every_run_and_counts_only_its_era() {
    let lanes = [contested("a", Class::Conflict), lane("b", Era::Precheck, Class::Landed), contested("c", Class::Landed)];
    let first = replay(&lanes, Target::Conflict, Era::Batch, Scoring::LeaveOneBranchOut, Weights::default());
    let second = replay(&lanes, Target::Conflict, Era::Batch, Scoring::LeaveOneBranchOut, Weights::default());
    assert_eq!(first, second);
    assert_eq!(first.rows, 2);
}
