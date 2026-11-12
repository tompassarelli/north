use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::path::Path;

use super::model::{Role, SessionRecord};
use super::upward::opens;

pub const HELD: &str = "done-hold";
pub const MESSAGE: &str = "[north: your Done is held once, because this task came with no Done-when check] Before it stands, check each thing the task's own text says it expects of your result, the way that text says the result will be used: its command line, argument names, file paths and output format as written there, not a call of your own choosing. Fix what that shows, then end on your report with what the check printed.";

pub fn holds(record: &SessionRecord, reply: &str) -> bool {
    record.bare_task && record.role() == Role::Worker && opens(reply.trim(), "Done")
}

pub fn claim(session_dir: &Path) -> bool {
    match OpenOptions::new().write(true).create_new(true).open(session_dir.join(HELD)) {
        Ok(_) => true,
        Err(error) if error.kind() == ErrorKind::AlreadyExists => false,
        Err(error) => {
            eprintln!("north fleet: the Done hold of {} was not recorded, so its Done stands: {error}", session_dir.display());
            false
        }
    }
}

pub fn state(session_dir: &Path, record: &SessionRecord) -> Option<&'static str> {
    if session_dir.join(HELD).exists() {
        return Some("held");
    }
    record.bare_task.then_some("due")
}
