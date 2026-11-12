//! The move's steps in the order the plan prints them. Nothing irreversible happens before the new window is confirmed live, and every
//! failure before the old window ends closes only the new one and binds the coordinator's name back to the old, which stays as it was.
use crate::error::{NorthError, NorthResult};

use super::plan::Step;
use super::session::Window;

/// What each step does on a desktop: the move orders the calls and a test stands in for the windows and the fleet record.
pub(super) trait Mover {
    type Opened;
    fn carry(&self, window: &Window) -> NorthResult<()>;
    /// Opens the new window and returns once it runs on the target account; a window that never got there is closed before the error.
    fn open(&self, window: &Window) -> NorthResult<Self::Opened>;
    /// Whether the old window has stayed at the end of its turn with nothing written since the move began.
    fn untouched(&self, window: &Window) -> bool;
    fn rebind(&self, window: &Window, opened: &Self::Opened) -> NorthResult<()>;
    fn deliver(&self) -> NorthResult<()>;
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
    if !mover.untouched(window) {
        mover.close_new(opened);
        return Err(took_a_turn());
    }
    say(Step::Rebind);
    if let Err(error) = mover.rebind(window, &opened) {
        return Err(roll_back(mover, window, opened, error));
    }
    say(Step::Deliver);
    if let Err(error) = mover.deliver() {
        return Err(roll_back(mover, window, opened, error));
    }
    if !mover.untouched(window) {
        return Err(roll_back(mover, window, opened, took_a_turn()));
    }
    say(Step::EndOld);
    mover.end_old(window, &opened)
}

fn took_a_turn() -> NorthError {
    NorthError::Refused("the old window took a turn while the new one started; the new window is closed and the old one holds the conversation".into())
}

fn roll_back<M: Mover>(mover: &M, window: &Window, opened: M::Opened, error: NorthError) -> NorthError {
    mover.close_new(opened);
    match mover.restore(window) {
        Ok(()) => error,
        Err(restore) => NorthError::State(format!("{}; binding the coordinator back to the old window failed too: {}", error.user_message(), restore.user_message())),
    }
}
