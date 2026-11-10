//! The provider process a session host runs: its pid, its exit and the signals that end it.
use tokio::sync::watch;

/// The provider process a host runs and whether it has exited; the exit is left unreaped so a new image of the host
/// can still collect it.
pub(super) struct Provider {
    pub(super) pid: u32,
    exited: watch::Receiver<bool>,
}

impl Provider {
    pub(super) fn watch(pid: u32) -> Self {
        let (exit, exited) = watch::channel(false);
        std::thread::spawn(move || {
            if let Some(pid) = process_id(pid) {
                let options = rustix::process::WaitidOptions::EXITED | rustix::process::WaitidOptions::NOWAIT;
                while matches!(rustix::process::waitid(rustix::process::WaitId::Pid(pid), options), Err(rustix::io::Errno::INTR)) {}
            }
            let _ = exit.send(true);
        });
        Self { pid, exited }
    }

    pub(super) async fn ended(&mut self) {
        let _ = self.exited.wait_for(|exited| *exited).await;
    }

    pub(super) fn has_ended(&self) -> bool {
        *self.exited.borrow()
    }

    pub(super) fn reap(&self) -> u8 {
        let status = process_id(self.pid).and_then(|pid| rustix::process::waitpid(Some(pid), rustix::process::WaitOptions::empty()).ok().flatten());
        match status.map(|status| (status.exit_status(), status.terminating_signal())) {
            Some((Some(code), _)) => u8::try_from(code).unwrap_or(1),
            Some((None, Some(signal))) => u8::try_from(128 + signal).unwrap_or(255),
            _ => 1,
        }
    }

}

pub(super) fn signal(pid: u32, signal: rustix::process::Signal) {
    if let Some(pid) = process_id(pid) {
        let _ = rustix::process::kill_process(pid, signal);
    }
}

pub(super) fn process_id(pid: u32) -> Option<rustix::process::Pid> {
    i32::try_from(pid).ok().and_then(rustix::process::Pid::from_raw)
}
