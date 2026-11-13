use north_base::foundation::step::{CancelOrigin, CancelSender, request_cancel_from};
use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
use signal_hook::iterator::SignalsInfo;
use signal_hook::iterator::exfiltrator::WithOrigin;
use signal_hook::low_level::siginfo::Origin;

pub fn watch_for_cancel() {
    std::thread::spawn(|| {
        let Ok(mut signals) = SignalsInfo::<WithOrigin>::new([SIGINT, SIGTERM, SIGHUP]) else { return };
        for origin in signals.forever() {
            request_cancel_from(sender(&origin));
        }
    });
}

fn sender(origin: &Origin) -> CancelOrigin {
    let sender = origin.process.as_ref().map(|process| CancelSender { pid: process.pid.cast_unsigned(), uid: process.uid, comm: comm(process.pid.cast_unsigned()) });
    CancelOrigin { signal: name(origin.signal), sender }
}

fn name(signal: i32) -> &'static str {
    match signal {
        SIGINT => "SIGINT",
        SIGTERM => "SIGTERM",
        SIGHUP => "SIGHUP",
        _ => "an unnamed signal",
    }
}

fn comm(pid: u32) -> String {
    std::fs::read_to_string(format!("/proc/{pid}/comm")).map_or_else(|_| "unknown".to_owned(), |comm| comm.trim().to_owned())
}
