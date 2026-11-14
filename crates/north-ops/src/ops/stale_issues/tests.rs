use super::{COMMENT, Hands, Lanes, Owners, Record, Stale, sweep};

#[derive(Default)]
struct Spy {
    messages: Vec<(String, String)>,
    closed: Vec<(u64, String)>,
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
}

#[test]
fn an_open_issue_25_hours_old_is_closed_with_its_open_boxes_and_lanes_and_its_holder_messaged() {
    let directory = tempfile::tempdir().unwrap();
    let now = 1_000_000;
    let lanes = Lanes::parse("\u{1}refs/heads/n7-lane\nA change\n\nItem: tompassarelli/north#7\nBox: one\n\u{1}refs/heads/other\nB\n\nItem: wisp#7\n", "north");
    assert_eq!(lanes.naming(7), ["n7-lane"]);
    assert_eq!(Owners::default().of(7, "intro\nOwner: burn-lead-b-2. More text.\n").as_deref(), Some("burn-lead-b-2"));
    assert_eq!(Owners::default().of(7, "Owner: north#636\n"), None);
    let issues = [
        Stale { number: 7, created: now - 25 * 3600, owner: Some("burn-lead-b-2".to_owned()), open_boxes: vec!["A box".to_owned()], lanes: lanes.naming(7) },
        Stale { number: 9, created: now - 26 * 3600, owner: None, open_boxes: Vec::new(), lanes: Vec::new() },
        Stale { number: 8, created: now - 23 * 3600, owner: Some("fresh-owner".to_owned()), open_boxes: Vec::new(), lanes: Vec::new() },
    ];
    let mut spy = Spy::default();
    let mut record = Record::open(&directory.path().join("messaged.txt")).unwrap();

    let first = sweep("north", &issues, now, false, &mut spy, &mut record);
    assert_eq!(first.failed, 0);
    assert_eq!(spy.closed.len(), 2);
    assert!(spy.closed[0].1.starts_with(COMMENT) && spy.closed[0].1.contains("- A box\n") && spy.closed[0].1.contains("- n7-lane\n"), "{}", spy.closed[0].1);
    assert_eq!(spy.closed[1].1, COMMENT, "an issue with no open box or lane gets the rule alone");
    assert!(COMMENT.contains("24-hour rule"));
    assert_eq!(spy.messages.iter().map(|(to, _)| to.as_str()).collect::<Vec<_>>(), ["burn-lead-b-2", "proxy"]);
    assert_eq!(spy.messages[0].1, "north#7 closed at 24 h: file a finishable successor for its open boxes, comment `rewritten as REPO#N` on it so its holder's launches follow, and re-point lanes that name it");

    let mut record = Record::open(&directory.path().join("messaged.txt")).unwrap();
    sweep("north", &issues, now, false, &mut spy, &mut record);
    assert_eq!(spy.messages.len(), 2, "a rerun on the same issue messages its holder no second time");
}
