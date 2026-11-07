use super::*;
use std::cell::RefCell;

struct Fake {
    closed: bool,
    plan: &'static str,
    calls: RefCell<Vec<String>>,
}

impl Desk for Fake {
    fn landed(&self, _: &Path, _: &str) -> Result<bool, String> {
        Ok(true)
    }
    fn item_closed(&self, _: &Path, _: &str) -> Result<bool, String> {
        Ok(self.closed)
    }
    fn step(&self, verb: &str, _: &Value) -> Result<bool, String> {
        self.calls.borrow_mut().push(verb.to_owned());
        Ok(true)
    }
    fn plan(&self, _: &Value) -> Option<String> {
        planned_account(self.plan)
    }
    fn gate(&self, _: &Value, _: &Path, account: Option<&str>) -> Result<Option<String>, String> {
        self.calls.borrow_mut().push(format!("gate {}", account.unwrap_or("default")));
        Ok(Some("Hold: claude:c exhausted".into()))
    }
    fn start(&self, _: &Value, _: Option<&str>) -> Result<bool, String> {
        self.calls.borrow_mut().push("start".into());
        Ok(true)
    }
    fn alert(&self, _: &Value, why: &str, _: f64) -> Result<(), String> {
        self.calls.borrow_mut().push(format!("alert {why}"));
        Ok(())
    }
}

#[test]
fn a_released_follows_release_box_on_a_closed_issue_finishes_without_the_gate_and_the_gate_judges_the_planned_account() {
    let planned = "  claude:b unavailable: exhausted: provider week 100%\n  claude:c available: 5h 56%, week 71% (29% headroom)\nnorth#653 box: target gex45 claude:c (code-only box on public north)\n";
    let queued = || json!({"id": 358, "state": "queued", "item": "north#653", "follows": "north#652", "tier": "worker", "brief": "Item: north#653"});
    let closed = Fake { closed: true, plan: planned, calls: RefCell::default() };
    tend(&closed, &mut queued(), Path::new("."), 0.0).unwrap();
    assert_eq!(closed.calls.into_inner(), ["follow-release", "follow-started"], "a box whose own issue closed before release is done with no gate call and no alert");
    let open = Fake { closed: false, plan: planned, calls: RefCell::default() };
    tend(&open, &mut queued(), Path::new("."), 0.0).unwrap();
    assert_eq!(open.calls.into_inner()[..2], ["follow-release".to_owned(), "gate c".to_owned()], "the gate judges the account agents plan picks, not the exhausted default");
}
