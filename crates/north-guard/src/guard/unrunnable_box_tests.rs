use super::*;

#[derive(Default)]
struct Recorder {
    enforcing: bool,
    rows: Vec<serde_json::Value>,
}

impl Log for Recorder {
    fn enforcing(&self) -> bool {
        self.enforcing
    }

    fn record(&mut self, row: serde_json::Value) {
        self.rows.push(row);
    }
}

#[test]
fn an_edit_that_rewrites_a_held_box_is_not_a_new_box_and_an_enforcing_gate_refuses_the_new_one() {
    let held = "- [ ] Each refusing guard has a measured false-refusal rate from a sample of its real refusals, and one over 10% is narrowed (from north#715)";
    let edited = format!("{held} Measured 2026-11-08: the sample narrows poll-loop.\n- [ ] A new box a worker can only tick itself\n- [ ] A new box with its check. Done when: `cargo test -p north-guard unrunnable` passes\n- [ ] A decision for Tom. Needs: Tom\n- [x] A ticked box with no command");
    let filings = [Filing { issue: Some("706"), held: Some(held) }];
    let mut log = Recorder::default();
    judge("tompassarelli/north", &filings, &edited, &mut log).unwrap();
    let logged: Vec<(&str, &str, &str)> = log.rows.iter().map(|row| (row["verb"].as_str().unwrap(), row["issue"].as_str().unwrap(), row["box"].as_str().unwrap())).collect();
    assert_eq!(logged, [("edit", "706", "A new box a worker can only tick itself")]);
    let mut log = Recorder { enforcing: true, ..Recorder::default() };
    let refused = judge("tompassarelli/north", &filings, &edited, &mut log).unwrap_err();
    assert!(log.rows.is_empty(), "an enforcing gate logs nothing");
    assert!(refused.contains("A new box a worker can only tick itself") && refused.contains("Done when:") && !refused.contains("Each refusing guard"));
}
