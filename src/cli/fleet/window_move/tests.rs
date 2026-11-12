use std::cell::RefCell;
use std::path::PathBuf;

use crate::error::{NorthError, NorthResult};

use super::session::{Session, Window, choose};
use super::steps::{Mover, perform};

const NEW: u32 = 900;

fn window(client: u32, conversation: &str, terminal: bool) -> NorthResult<Window> {
    let session = Session { id: conversation.to_owned(), transcript: PathBuf::new(), job: None, model: None };
    Ok(Window { client, profile: PathBuf::from("/profiles/claude-a"), account: "a".into(), arguments: Vec::new(), cwd: PathBuf::from("/work"), terminal, session })
}

struct Desk {
    opens: bool,
    calls: RefCell<Vec<String>>,
    record: RefCell<(String, u32)>,
}

impl Desk {
    fn new(opens: bool) -> Self {
        Self { opens, calls: RefCell::default(), record: RefCell::new(("a".into(), 12)) }
    }

    fn call(&self, step: &str, window: &Window) {
        self.calls.borrow_mut().push(format!("{step} {}", window.client));
    }
}

impl Mover for Desk {
    type Opened = u32;

    fn carry(&self, window: &Window) -> NorthResult<()> {
        self.call("carry", window);
        Ok(())
    }

    fn open(&self, window: &Window) -> NorthResult<u32> {
        self.call("open", window);
        if self.opens { Ok(NEW) } else { Err(NorthError::State("the new window did not resume the conversation".into())) }
    }

    fn rebind(&self, window: &Window, opened: &u32) -> NorthResult<()> {
        self.call("rebind", window);
        *self.record.borrow_mut() = ("b".into(), *opened);
        Ok(())
    }

    fn deliver(&self) -> NorthResult<()> {
        self.calls.borrow_mut().push("deliver".into());
        Ok(())
    }

    fn end_old(&self, window: &Window, _: &u32) -> NorthResult<()> {
        self.call("end", window);
        Ok(())
    }

    fn close_new(&self, opened: u32) {
        self.calls.borrow_mut().push(format!("close {opened}"));
    }

    fn restore(&self, window: &Window) -> NorthResult<()> {
        self.call("restore", window);
        *self.record.borrow_mut() = ("a".into(), window.client);
        Ok(())
    }
}

#[test]
fn the_move_takes_the_recorded_window_among_three_and_a_failed_open_leaves_the_old_window_and_the_record() {
    let three = || vec![window(11, "owner-1", true), window(12, "coordinator", true), window(13, "owner-2", true)];
    let chosen = choose(three(), "coordinator").unwrap();
    assert_eq!(chosen.client, 12, "the window on the conversation the fleet record names, whatever else is open");
    let desk = Desk::new(true);
    perform(&desk, &chosen, "b").unwrap();
    assert_eq!(*desk.calls.borrow(), ["carry 12", "open 12", "rebind 12", "deliver", "end 12"], "the old window ends last, after the new one is live and holds the name");
    assert_eq!(*desk.record.borrow(), ("b".to_owned(), NEW));

    let refused = choose(vec![window(12, "coordinator", false), window(11, "owner-1", true)], "coordinator").err().map(|error| error.user_message()).unwrap_or_default();
    assert!(refused.contains("process 12 on account a holding conversation coordinator, not on a terminal") && refused.contains("process 11"), "{refused}");

    let desk = Desk::new(false);
    assert!(perform(&desk, &choose(three(), "coordinator").unwrap(), "b").is_err());
    assert_eq!(*desk.calls.borrow(), ["carry 12", "open 12"], "a failed open ends nothing and binds nothing");
    assert_eq!(*desk.record.borrow(), ("a".to_owned(), 12));
}
