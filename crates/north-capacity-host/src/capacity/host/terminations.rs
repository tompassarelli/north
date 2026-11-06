use tokio::signal::unix::{Signal, SignalKind, signal};

pub(super) struct Terminations([Option<Signal>; 3]);

impl Terminations {
    #[cfg(test)]
    pub(super) fn none() -> Self {
        Self([None, None, None])
    }

    pub(super) fn listen() -> Self {
        Self([SignalKind::interrupt(), SignalKind::terminate(), SignalKind::hangup()].map(|kind| signal(kind).ok()))
    }

    pub(super) async fn next(&mut self) -> i32 {
        async fn one(signal: &mut Option<Signal>) {
            match signal {
                Some(signal) => drop(signal.recv().await),
                None => std::future::pending().await,
            }
        }
        let [interrupt, terminate, hangup] = &mut self.0;
        tokio::select! {
            () = one(interrupt) => 2,
            () = one(terminate) => 15,
            () = one(hangup) => 1,
        }
    }
}
