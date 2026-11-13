use tokio::sync::watch;

pub const STOPPED: &str = "the checks before queueing were stopped because a gate refused the lane";

pub struct Stop {
    sender: watch::Sender<bool>,
}

impl Stop {
    pub fn new() -> Self {
        Self { sender: watch::channel(false).0 }
    }

    pub fn stop(&self) {
        self.sender.send_replace(true);
    }

    pub fn stopped(&self) -> bool {
        *self.sender.borrow()
    }

    pub(super) fn watch(&self) -> watch::Receiver<bool> {
        let mut receiver = self.sender.subscribe();
        if *receiver.borrow() {
            receiver.mark_changed();
        }
        receiver
    }
}

impl Default for Stop {
    fn default() -> Self {
        Self::new()
    }
}
