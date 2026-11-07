use super::super::{AFTER, COVERED_BY, Category, PHASE, Span};
use super::Trace;

fn landing(name: &str, landed: &str, start: f64, end: f64) -> Span {
    Span::new(&format!("ticket-{name}"), name, Category::Landing, start, end).with("landed", landed)
}

fn after(trace: &str, name: &str, category: Category, start: f64, end: f64) -> Span {
    Span::new(trace, name, category, start, end).with(PHASE, AFTER)
}

#[test]
fn a_landing_covered_by_a_later_run_counts_its_wait_and_that_run_as_its_verification() {
    let spans = vec![
        landing("older", "b", 0.0, 10.0),
        landing("newer", "c", 20.0, 30.0),
        after("b", "waiting for the after-landing run in flight", Category::Queue, 10.0, 60.0),
        after("b", "after-landing run of c", Category::Lander, 60.0, 100.0).with(COVERED_BY, "c"),
        after("c", "post-land checks", Category::Check, 60.0, 100.0),
        after("c", "post-land checks", Category::Check, 200.0, 240.0),
    ];
    let trace = Trace::of(&spans);
    let landings = trace.landings(0.0);
    let older = trace.profile(landings[0]);
    assert!((older.after_wall - 90.0).abs() < 1e-9, "older verified for {}", older.after_wall);
    assert!((older.after.get(Category::Queue) - 50.0).abs() < 1e-9);
    assert!((older.after.get(Category::Check) - 40.0).abs() < 1e-9);
    assert!(older.after.get(Category::Lander).abs() < 1e-9);
}
