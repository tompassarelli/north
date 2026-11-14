use super::{COMMENT, Hands, Lanes, Owners, Record, Stale, sweep};

#[derive(Default)]
struct Spy {
    messages: Vec<(String, String)>,
    closed: Vec<(u64, String)>,
    filed: Vec<(String, String)>,
    rewrites: Vec<(String, String)>,
}

impl Hands for Spy {
    fn message(&mut self, session: &str, text: &str) -> Result<(), String> {
        self.messages.push((session.to_owned(), text.to_owned()));
        Ok(())
    }

    fn close(&mut self, number: u64, comment: &str) -> Result<(), String> {
        self.closed.push((number, comment.to_owned()));
        Ok(())
    }

    fn file(&mut self, title: &str, body: &str) -> Result<u64, String> {
        self.filed.push((title.to_owned(), body.to_owned()));
        Ok(100 + self.filed.len() as u64)
    }

    fn recorded_successor(&mut self, closed: &str) -> Result<Option<String>, String> {
        Ok(self.rewrites.iter().find(|(from, _)| from == closed).map(|(_, to)| to.clone()))
    }

    fn record_successor(&mut self, closed: &str, successor: &str) -> Result<(), String> {
        self.rewrites.push((closed.to_owned(), successor.to_owned()));
        Ok(())
    }
}

#[test]
fn an_open_issue_25_hours_old_is_closed_with_its_open_boxes_and_lanes_rewritten_as_a_recorded_successor_and_its_holder_messaged() {
    let directory = tempfile::tempdir().unwrap();
    let now = 1_000_000;
    let lanes = Lanes::parse("\u{1}refs/heads/n7-lane\nA change\n\nItem: tompassarelli/north#7\nBox: one\n\u{1}refs/heads/other\nB\n\nItem: wisp#7\n", "north");
    assert_eq!(lanes.naming(7), ["n7-lane"]);
    assert_eq!(Owners::default().of(7, "intro\nOwner: burn-lead-b-2. More text.\n").as_deref(), Some("burn-lead-b-2"));
    assert_eq!(Owners::default().of(7, "Owner: north#636\n"), None);
    let stale = |number: u64, hours: i64, owner: Option<&str>, open_boxes: Vec<String>, lanes: Vec<String>| Stale { number, created: now - hours * 3600, title: "Leads keep working (rewritten from north#3)".to_owned(), owner: owner.map(str::to_owned), open_boxes, lanes };
    let issues = [
        stale(7, 25, Some("burn-lead-b-2"), vec!["A box\n  Done when: it passes.".to_owned()], lanes.naming(7)),
        stale(9, 26, None, Vec::new(), Vec::new()),
        stale(8, 23, Some("fresh-owner"), Vec::new(), Vec::new()),
    ];
    let mut spy = Spy::default();
    let mut record = Record::open(&directory.path().join("messaged.txt")).unwrap();

    let first = sweep("north", &issues, now, false, &mut spy, &mut record);
    assert_eq!(first.failed, 0);
    assert_eq!(spy.closed.len(), 2);
    assert!(spy.closed[0].1.starts_with(COMMENT) && spy.closed[0].1.contains("- A box\n") && spy.closed[0].1.contains("- n7-lane\n") && spy.closed[0].1.contains("Rewritten as north#101"), "{}", spy.closed[0].1);
    assert_eq!(spy.closed[1].1, COMMENT, "an issue with no open box or lane gets the rule alone");
    assert!(COMMENT.contains("24-hour rule"));
    assert_eq!(spy.filed.len(), 1, "only the issue with an open box gets a successor");
    assert_eq!(spy.filed[0].0, "Leads keep working (rewritten from north#7)");
    assert!(spy.filed[0].1.contains("Owner: burn-lead-b-2\n") && spy.filed[0].1.contains("- [ ] A box\n  Done when: it passes.\n"), "{}", spy.filed[0].1);
    assert_eq!(spy.rewrites, [("north#7".to_owned(), "north#101".to_owned())], "the closer records the rewrite a lead's grant follows");
    assert_eq!(spy.messages.iter().map(|(to, _)| to.as_str()).collect::<Vec<_>>(), ["burn-lead-b-2", "proxy"]);
    assert!(spy.messages[0].1.starts_with("north#7 closed at 24 h: its open boxes moved to north#101"), "{}", spy.messages[0].1);

    let mut record = Record::open(&directory.path().join("messaged.txt")).unwrap();
    sweep("north", &issues, now, false, &mut spy, &mut record);
    assert_eq!(spy.messages.len(), 2, "a rerun on the same issue messages its holder no second time");
    assert_eq!(spy.filed.len(), 1, "a rerun files no second successor for a recorded rewrite");
}
