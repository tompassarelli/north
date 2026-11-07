use super::rework_labels::score;

const PRECISION_FLOOR: f64 = 0.33;
const RECALL_FLOOR: f64 = 0.50;

#[test]
fn a_rule_change_that_lowers_the_judges_precision_or_recall_on_the_blind_labels_fails() {
    let score = score();
    assert!(score.n >= 60, "the sample holds {} landings", score.n);
    assert!(score.precision >= PRECISION_FLOOR, "precision fell to {:.3} from {PRECISION_FLOOR}", score.precision);
    assert!(score.recall >= RECALL_FLOOR, "recall fell to {:.3} from {RECALL_FLOOR}", score.recall);
}
