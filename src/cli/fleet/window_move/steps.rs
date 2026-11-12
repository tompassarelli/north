//! The move's steps in the order the plan prints them. The new window opens at once, beside the running old one, and nothing irreversible
//! happens before it is confirmed live; every failure before the old window ends closes only the new one and binds the coordinator's name
//! back to the old, which keeps running as it was.
use crate::error::{NorthError, NorthResult};

use super::plan::Step;
use super::session::Window;

/// What each step does on a desktop: the move orders the calls and a test stands in for the windows and the fleet record.
pub(super) trait Mover {
    type Opened;
    fn carry(&self, window: &Window) -> NorthResult<()>;
    /// Opens the new window and returns once it runs on the target account; a window that never got there is closed before the error.
    fn open(&self, window: &Window) -> NorthResult<Self::Opened>;
    fn rebind(&self, window: &Window, opened: &Self::Opened) -> NorthResult<()>;
    fn deliver(&self) -> NorthResult<()>;
    /// Ends the old window once its turn ends, within a bound of seconds, saying on it what it waits for.
    fn end_old(&self, window: &Window, opened: &Self::Opened) -> NorthResult<()>;
    /// Closes the new window and returns once its process is gone.
    fn close_new(&self, opened: Self::Opened);
    /// Binds the coordinator's name back to the old window, the only one left on the conversation.
    fn restore(&self, window: &Window) -> NorthResult<()>;
}

pub(super) fn perform<M: Mover>(mover: &M, window: &Window, to: &str) -> NorthResult<()> {
    let say = |step: Step| println!("north proxy move: {}", step.describe(window, to));
    say(Step::Carry);
    mover.carry(window)?;
    say(Step::Open);
    let opened = mover.open(window)?;
    say(Step::Rebind);
    if let Err(error) = mover.rebind(window, &opened) {
        return Err(roll_back(mover, window, opened, error));
    }
    say(Step::Deliver);
    if let Err(error) = mover.deliver() {
        return Err(roll_back(mover, window, opened, error));
    }
    say(Step::EndOld);
    mover.end_old(window, &opened)
}

fn roll_back<M: Mover>(mover: &M, window: &Window, opened: M::Opened, error: NorthError) -> NorthError {
    mover.close_new(opened);
    match mover.restore(window) {
        Ok(()) => error,
        Err(restore) => NorthError::State(format!("{}; binding the coordinator back to the old window failed too: {}", error.user_message(), restore.user_message())),
    }
}
