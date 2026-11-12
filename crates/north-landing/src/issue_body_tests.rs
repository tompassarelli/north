use std::cell::RefCell;

use super::*;

struct Tracked(RefCell<String>);

impl Tracker for Tracked {
    fn body(&self, _: &Issue) -> Result<String, String> {
        Ok(self.0.borrow().clone())
    }

    fn set_body(&self, _: &Issue, body: &str) -> Result<(), String> {
        *self.0.borrow_mut() = body.to_owned();
        Ok(())
    }

    fn close(&self, _: &Issue, _: &str) -> Result<(), String> {
        Ok(())
    }

    fn standing(&self, _: &Issue) -> Result<Standing, String> {
        Ok(Standing { open: true, open_children: 0 })
    }

    fn label(&self, _: &Issue, _: &str) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Default)]
struct Sent(RefCell<Vec<(String, String)>>);

impl Ledger for Sent {
    fn record(&self, item: &str, body: &str) -> Result<(), String> {
        self.0.borrow_mut().push((item.to_owned(), body.to_owned()));
        Ok(())
    }
}

#[test]
fn a_box_tick_sends_the_issue_boxes_row_the_split_settlement_reads() {
    let queue = tempfile::tempdir().unwrap();
    let issue = Issue::parse("north#872").unwrap();
    let tracker = Tracked(RefCell::new("- [ ] First box\n- [ ] Second box\n".to_owned()));
    let ledger = Sent::default();
    submit(queue.path(), &issue, &Edit::Land { box_text: "First box".into(), sha: "1a2b3c4d5e6f7".into() }).unwrap();
    submit(queue.path(), &issue, &Edit::Append { text: "a note".into() }).unwrap();

    let drained = drain(&directory(queue.path()), &tracker, &ledger, Duration::from_secs(5)).unwrap();

    let body = "- [x] First box (landed 1a2b3c4d5e6f)\n- [ ] Second box\na note\n";
    assert_eq!((drained.written, drained.recorded, drained.unrecorded), (2, 1, Vec::<String>::new()));
    assert_eq!(*ledger.0.borrow(), [("north#872".to_owned(), body.to_owned())]);
}
