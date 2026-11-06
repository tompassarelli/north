//! A fleet-tick stage that calls another host runs on its own thread, so the tick waits for it only
//! until its deadline and leaves a stage still running to the next tick.
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

#[derive(Debug, Eq, PartialEq)]
pub enum Late {
    Running,
    Panicked,
    Unstarted(String),
}

impl Late {
    pub fn why(&self, budget: Duration) -> String {
        match self {
            Self::Running => format!("still running after its {}ms budget", budget.as_millis()),
            Self::Panicked => "it panicked".to_owned(),
            Self::Unstarted(error) => format!("its thread did not start: {error}"),
        }
    }
}

pub struct Detached<R>(Result<Receiver<R>, String>);

pub fn detach<R: Send + 'static>(name: &str, run: impl FnOnce() -> R + Send + 'static) -> Detached<R> {
    let (answer, answered) = mpsc::channel();
    Detached(std::thread::Builder::new().name(name.into()).spawn(move || answer.send(run())).map(|_| answered).map_err(|error| error.to_string()))
}

impl<R> Detached<R> {
    pub fn by(self, deadline: Instant) -> Result<R, Late> {
        let answered = self.0.map_err(Late::Unstarted)?;
        answered.recv_timeout(deadline.saturating_duration_since(Instant::now())).map_err(|error| match error {
            RecvTimeoutError::Timeout => Late::Running,
            RecvTimeoutError::Disconnected => Late::Panicked,
        })
    }
}

pub fn each_by<R: Send + 'static, F: FnOnce() -> R + Send + 'static>(name: &str, runs: impl IntoIterator<Item = F>, deadline: Instant) -> Vec<Result<R, Late>> {
    let running: Vec<Detached<R>> = runs.into_iter().map(|run| detach(name, run)).collect();
    running.into_iter().map(|running| running.by(deadline)).collect()
}

