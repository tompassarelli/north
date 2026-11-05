use super::evidence::{IssueState, LaneClass, LaneFacts, lane_class};
use super::report::class_summary;

fn facts(equivalent: bool, issues: &[IssueState]) -> LaneFacts<'_> {
    LaneFacts { dirty: false, equivalent, issues, idle_hours: 0.0, held: false }
}

#[test]
fn a_branch_is_landed_superseded_or_lost_by_its_tree_and_issues() {
    assert_eq!(lane_class(&facts(true, &[])), LaneClass::LandedElsewhere);
    assert_eq!(lane_class(&facts(true, &[IssueState::Open])), LaneClass::LandedElsewhere);
    assert_eq!(lane_class(&facts(false, &[IssueState::Closed, IssueState::Closed])), LaneClass::Superseded);
    assert_eq!(lane_class(&facts(false, &[IssueState::Closed, IssueState::Open])), LaneClass::Owned);
    assert_eq!(lane_class(&facts(false, &[IssueState::Closed, IssueState::Unknown])), LaneClass::Owned);
    assert_eq!(lane_class(&facts(false, &[])), LaneClass::Owned);
}

#[test]
fn report_labels_name_the_three_classes() {
    assert_eq!(LaneClass::LandedElsewhere.report_label(), "landed-by-tree");
    assert_eq!(LaneClass::Superseded.report_label(), "superseded");
    assert_eq!(LaneClass::Owned.report_label(), "lost");
    assert_eq!(LaneClass::StaleDirty.report_label(), "lost");
}

#[test]
fn the_summary_counts_each_class_per_repo_and_treats_unclassed_rows_as_lost() {
    let row = |repo: &str, class: &str| format!("30h\t{repo}\t(branch)\tb\t+1\t-\tsubject\t-\t{class}");
    let rows = vec![
        "# header".to_string(),
        row("makgora", "lost"),
        row("makgora", "lost"),
        row("makgora", "superseded"),
        row("north", "landed-by-tree"),
        "40h\tnorth\t(branch)\told\t+2\t-\tsubject\t-".to_string(),
    ];
    assert_eq!(
        class_summary(&rows),
        vec![
            "# classes makgora: landed-by-tree=0 superseded=1 lost=2".to_string(),
            "# classes north: landed-by-tree=1 superseded=0 lost=1".to_string(),
        ]
    );
}
