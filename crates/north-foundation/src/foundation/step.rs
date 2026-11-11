use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Eq, PartialEq)]
pub struct Refused(pub u8);

pub type Step<T = ()> = Result<T, Refused>;

static CANCELLED: AtomicBool = AtomicBool::new(false);

pub fn cancelled() -> Step {
    if CANCELLED.load(Ordering::SeqCst) { Err(Refused(130)) } else { Ok(()) }
}

pub fn request_cancel() {
    CANCELLED.store(true, Ordering::SeqCst);
}

pub fn refuse(message: impl std::fmt::Display) -> Refused {
    if let Err(cancel) = cancelled() {
        return cancel;
    }
    println!("{message}");
    Refused(1)
}
