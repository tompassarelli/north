use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Eq, PartialEq)]
pub struct Refused(pub u8);

pub type Step<T = ()> = Result<T, Refused>;

static CANCELLED: AtomicBool = AtomicBool::new(false);
static CANCEL_ORIGIN: OnceLock<CancelOrigin> = OnceLock::new();

/// Who cancelled a run: the signal's name and, when the kernel named it, the process that sent it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CancelOrigin {
    pub signal: &'static str,
    pub sender: Option<CancelSender>,
}

/// The process that sent the cancel signal, with the command name read from `/proc/PID/comm` while it lived.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CancelSender {
    pub pid: u32,
    pub uid: u32,
    pub comm: String,
}

impl std::fmt::Display for CancelOrigin {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.sender {
            Some(sender) => write!(formatter, "{} from pid {} uid {} comm {}", self.signal, sender.pid, sender.uid, sender.comm),
            None => write!(formatter, "{} from no process, such as the kernel", self.signal),
        }
    }
}

pub fn cancelled() -> Step {
    if CANCELLED.load(Ordering::SeqCst) { Err(Refused(130)) } else { Ok(()) }
}

pub fn request_cancel() {
    CANCELLED.store(true, Ordering::SeqCst);
}

/// Records the signal's sender before the cancel flag it sets can be read, so a receipt that names the cancel names
/// its sender too.
pub fn request_cancel_from(origin: CancelOrigin) {
    let _ = CANCEL_ORIGIN.set(origin);
    request_cancel();
}

pub fn cancel_origin() -> Option<&'static CancelOrigin> {
    CANCEL_ORIGIN.get()
}

pub fn refuse(message: impl std::fmt::Display) -> Refused {
    if let Err(cancel) = cancelled() {
        return cancel;
    }
    println!("{message}");
    Refused(1)
}
