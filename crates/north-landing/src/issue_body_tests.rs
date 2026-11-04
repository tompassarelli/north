use super::*;
use std::cell::RefCell;
use tempfile::TempDir;

fn issue() -> Issue {
    Issue::parse("north#112").unwrap()
}

fn append(text: &str) -> Edit {
    Edit::Append { text: text.to_owned() }
}

fn later() -> Instant {
    Instant::now() + Duration::from_secs(60)
}

#[derive(Default)]
struct Fake {
    body: RefCell<String>,
    writes: RefCell<usize>,
    racer: RefCell<Vec<String>>,
    dropped_writes: RefCell<usize>,
}

impl Fake {
    fn with(body: &str) -> Fake {
        Fake { body: RefCell::new(body.to_owned()), ..Fake::default() }
    }
}

impl Github for Fake {
    fn body(&self, _: &Issue) -> Result<String, String> {
        Ok(self.body.borrow().clone())
    }

    fn set_body(&self, _: &Issue, body: &str) -> Result<(), String> {
        *self.writes.borrow_mut() += 1;
        if *self.dropped_writes.borrow() > 0 {
            *self.dropped_writes.borrow_mut() -= 1;
            return Ok(());
        }
        let racing = self.racer.borrow_mut().pop();
        *self.body.borrow_mut() = racing.unwrap_or_else(|| body.to_owned());
        Ok(())
    }
}

fn script(directory: &Path, name: &str, text: &str) -> PathBuf {
    let path = directory.join(name);
    north_base::foundation::shim::write_script(&path, text);
    path
}

fn working_gh(directory: &Path) -> Gh {
    let state = directory.join("state-body");
    fs::write(&state, "# Goal\n- [ ] first box").unwrap();
    let text = format!("case \"$2\" in view) cat '{0}'; echo ;; edit) cat > '{0}' ;; *) exit 2 ;; esac\n", state.display());
    Gh::at(script(directory, "gh-working", &text), Duration::from_secs(10))
}

fn hanging_gh(directory: &Path, limit: Duration) -> Gh {
    Gh::at(script(directory, "gh-hanging", "exec sleep 30\n"), limit)
}

#[test]
fn an_issue_parses_to_its_owner_repo_and_number() {
    assert_eq!(issue(), Issue { repo: "tompassarelli/north".into(), number: 112 });
    assert_eq!(issue().to_string(), "tompassarelli/north#112");
    assert!(Issue::parse("north").is_err());
}

#[test]
fn an_append_adds_the_box_once_and_a_replay_is_already_applied() {
    let Change::Applied(first) = append("- [ ] new box").apply("# Goal\n- [ ] old box\n") else { panic!("not applied") };
    assert_eq!(first, "# Goal\n- [ ] old box\n- [ ] new box\n");
    assert_eq!(append("- [ ] new box").apply(&first), Change::Already);
    assert_eq!(append("- [ ] new box").apply("- [x] new box"), Change::Already);
    assert_eq!(append("- [ ] a\n- [ ] b").apply("- [ ] a\n"), Change::Applied("- [ ] a\n- [ ] a\n- [ ] b\n".into()));
    assert_eq!(append("- [ ] only").apply(""), Change::Applied("- [ ] only\n".into()));
    assert!(matches!(append("  \n").apply("body"), Change::Refused(_)));
}

#[test]
fn a_tick_changes_one_unticked_box_and_refuses_none_or_many() {
    let body = "- [ ] build it\n- [ ] ship it\r\n- [x] done already\n";
    let tick = |text: &str| Edit::Tick { box_text: text.to_owned() }.apply(body);
    assert_eq!(tick("ship it"), Change::Applied("- [ ] build it\n- [x] ship it\r\n- [x] done already\n".into()));
    assert_eq!(tick("done already"), Change::Already);
    assert!(matches!(tick("absent"), Change::Refused(_)));
    assert!(matches!(tick("it"), Change::Refused(why) if why.contains("2 unticked")));
}

#[test]
fn a_clean_write_is_read_back_once_and_verified() {
    let fake = Fake::with("# Goal\n");
    assert_eq!(write(&fake, &issue(), &append("- [ ] box"), later()), Ok(Outcome::Written));
    assert_eq!(*fake.body.borrow(), "# Goal\n- [ ] box\n");
    assert_eq!(*fake.writes.borrow(), 1);
    assert_eq!(write(&fake, &issue(), &append("- [ ] box"), later()), Ok(Outcome::Already));
    assert_eq!(*fake.writes.borrow(), 1);
}

#[test]
fn a_lost_race_is_retried_on_the_body_the_racer_left() {
    let fake = Fake::with("# Goal\n");
    fake.racer.borrow_mut().push("# Goal\nrewritten by another writer\n".to_owned());
    assert_eq!(write(&fake, &issue(), &append("- [ ] box"), later()), Ok(Outcome::Written));
    assert_eq!(*fake.body.borrow(), "# Goal\nrewritten by another writer\n- [ ] box\n");
    assert_eq!(*fake.writes.borrow(), 2);
}

#[test]
fn a_write_that_gh_accepts_and_drops_is_caught_by_the_read_back() {
    let fake = Fake::with("# Goal\n");
    *fake.dropped_writes.borrow_mut() = 1;
    assert_eq!(write(&fake, &issue(), &append("- [ ] box"), later()), Ok(Outcome::Written));
    assert_eq!(*fake.writes.borrow(), 2);

    let stubborn = Fake::with("# Goal\n");
    *stubborn.dropped_writes.borrow_mut() = 99;
    let why = write(&stubborn, &issue(), &append("- [ ] box"), later()).unwrap_err();
    assert!(why.contains("did not stay after 4 attempts"), "{why}");
    assert_eq!(*stubborn.writes.borrow(), 4);
}

#[test]
fn a_passed_deadline_stops_the_writer_before_it_calls_gh() {
    let fake = Fake::with("# Goal\n");
    assert!(write(&fake, &issue(), &append("- [ ] box"), Instant::now()).is_err());
    assert_eq!(*fake.writes.borrow(), 0);
}

#[test]
fn the_real_gh_wrapper_reads_and_writes_through_the_cli() {
    let temporary = TempDir::new().unwrap();
    let gh = working_gh(temporary.path());
    assert_eq!(gh.body(&issue()).unwrap(), "# Goal\n- [ ] first box");
    assert_eq!(write(&gh, &issue(), &append("- [ ] second box"), later()), Ok(Outcome::Written));
    assert_eq!(fs::read_to_string(temporary.path().join("state-body")).unwrap(), "# Goal\n- [ ] first box\n- [ ] second box\n");
}

#[test]
fn submitting_twice_keeps_one_entry_and_a_different_edit_a_second() {
    let temporary = TempDir::new().unwrap();
    let first = submit(temporary.path(), &issue(), &append("- [ ] box")).unwrap();
    assert_eq!(submit(temporary.path(), &issue(), &append("- [ ] box")).unwrap(), first);
    submit(temporary.path(), &issue(), &append("- [ ] other")).unwrap();
    assert_eq!(pending(&directory(temporary.path())).unwrap().len(), 2);
}

#[test]
fn a_drain_applies_every_entry_in_order_and_removes_it() {
    let temporary = TempDir::new().unwrap();
    submit(temporary.path(), &issue(), &append("- [ ] one")).unwrap();
    submit(temporary.path(), &issue(), &append("- [ ] two")).unwrap();
    submit(temporary.path(), &issue(), &Edit::Tick { box_text: "absent".into() }).unwrap();
    let fake = Fake::with("# Goal\n");
    let drained = drain(&directory(temporary.path()), &fake, DRAIN_LIMIT).unwrap();
    assert_eq!(drained, Drained { written: 2, refused: 1, ..Drained::default() });
    assert_eq!(*fake.body.borrow(), "# Goal\n- [ ] one\n- [ ] two\n");
    assert!(pending(&directory(temporary.path())).unwrap().is_empty());
    assert!(directory(temporary.path()).join(REFUSED).is_dir());
}

#[test]
fn a_hung_gh_never_holds_the_landing_and_the_entry_survives_a_restart() {
    let temporary = TempDir::new().unwrap();
    let queue = temporary.path().join("queue");
    let tools = temporary.path().join("tools");
    fs::create_dir_all(&tools).unwrap();

    let landing = Instant::now();
    submit(&queue, &issue(), &append("- [ ] verdict box")).unwrap();
    let background = drain_in_background(directory(&queue), hanging_gh(&tools, Duration::from_millis(400)), DRAIN_LIMIT);
    assert!(landing.elapsed() < Duration::from_millis(300), "the landing waited {:?}", landing.elapsed());
    assert!(!background.is_finished());

    let drained = background.join().unwrap().unwrap();
    assert_eq!(drained.waiting, 1);
    assert!(drained.failure.as_deref().is_some_and(|why| why.contains("timed out")), "{drained:?}");
    assert_eq!(pending(&directory(&queue)).unwrap().len(), 1);

    let restarted = drain(&directory(&queue), &working_gh(&tools), DRAIN_LIMIT).unwrap();
    assert_eq!(restarted, Drained { written: 1, ..Drained::default() });
    assert_eq!(fs::read_to_string(tools.join("state-body")).unwrap(), "# Goal\n- [ ] first box\n- [ ] verdict box\n");
    assert!(pending(&directory(&queue)).unwrap().is_empty());
}

#[test]
fn an_unreadable_entry_moves_to_refused_instead_of_blocking_the_rest() {
    let temporary = TempDir::new().unwrap();
    let outbox = directory(temporary.path());
    fs::create_dir_all(&outbox).unwrap();
    fs::write(outbox.join("1-bad.json"), "not json").unwrap();
    submit(temporary.path(), &issue(), &append("- [ ] box")).unwrap();
    let fake = Fake::with("");
    assert_eq!(drain(&outbox, &fake, DRAIN_LIMIT).unwrap().written, 1);
    assert!(outbox.join(REFUSED).join("1-bad.json").is_file());
}
