use super::{COMMENT, Hands, Record, Stale, sweep};

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
fn an_open_issue_25_hours_old_is_closed_and_its_owner_messaged() {
    let directory = tempfile::tempdir().unwrap();
    let now = 1_000_000;
    let issues = [
        Stale { number: 7, created: now - 25 * 3600, owner: Some("burn-lead-b-2".to_owned()) },
        Stale { number: 8, created: now - 23 * 3600, owner: Some("fresh-owner".to_owned()) },
    ];
    let mut spy = Spy::default();
    let mut record = Record::open(&directory.path().join("messaged.txt")).unwrap();

    let first = sweep("north", &issues, now, false, &mut spy, &mut record);
    assert_eq!(first.failed, 0);
    assert_eq!(spy.closed, vec![(7, COMMENT.to_owned())]);
    assert!(COMMENT.contains("24-hour rule"));
    assert_eq!(spy.messages.len(), 1);
    assert_eq!(spy.messages[0].0, "burn-lead-b-2");
    assert!(spy.messages[0].1.contains("north#7"));

    let mut record = Record::open(&directory.path().join("messaged.txt")).unwrap();
    sweep("north", &issues, now, false, &mut spy, &mut record);
    assert_eq!(spy.messages.len(), 1, "a rerun on the same issue messages its owner no second time");
}
