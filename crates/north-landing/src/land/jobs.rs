use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::after::{self, Recovered};
use super::lander::Lander;
use super::sheriff;

#[derive(Clone, Copy)]
pub struct Chores {
    pub patrol: fn(&Lander) -> Result<(), String>,
    pub recover: fn(&Lander, &Path) -> Result<Option<Recovered>, String>,
    pub pull: fn(&Lander) -> Vec<String>,
}

impl Default for Chores {
    fn default() -> Self {
        Self { patrol: sheriff::patrol, recover: after::recover, pull: Lander::pull_remote }
    }
}

enum Done {
    Patrolled,
    Recovered(Option<Recovered>),
    Pulled(Vec<String>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Sheriff,
    Recovery,
    OutboxPull,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Sheriff => "sheriff",
            Kind::Recovery => "recovery",
            Kind::OutboxPull => "outbox pull",
        }
    }
}

struct Job {
    kind: Kind,
    started: Instant,
    run: JoinHandle<Result<Done, String>>,
}

const REST: Duration = Duration::from_secs(5);

#[derive(Default)]
pub struct Jobs {
    running: Vec<Job>,
    ended: Vec<(Kind, Instant)>,
    listed: Option<(Instant, bool)>,
}

impl Jobs {
    fn spawn(&mut self, lander: &Lander, kind: Kind, chore: impl FnOnce(&Lander) -> Result<Done, String> + Send + 'static) {
        let resting = self.ended.iter().any(|(ended, at)| *ended == kind && at.elapsed() < REST);
        if resting || self.running.iter().any(|job| job.kind == kind) {
            return;
        }
        let owned = lander.detached();
        match std::thread::Builder::new().name(format!("land {}", kind.name())).spawn(move || chore(&owned)) {
            Ok(run) => self.running.push(Job { kind, started: Instant::now(), run }),
            Err(error) => println!("north land: could not start the {}: {error}", kind.name()),
        }
    }

    pub fn start(&mut self, lander: &Lander) {
        let chores = lander.chores;
        self.spawn(lander, Kind::Sheriff, move |lander| (chores.patrol)(lander).map(|()| Done::Patrolled));
        if let Some(claim) = after::claim(lander) {
            self.spawn(lander, Kind::Recovery, move |lander| (chores.recover)(lander, &claim).map(Done::Recovered));
        }
        self.spawn(lander, Kind::OutboxPull, move |lander| Ok(Done::Pulled((chores.pull)(lander))));
    }

    pub fn settle(&mut self) -> Vec<Recovered> {
        let (finished, running): (Vec<Job>, Vec<Job>) = self.running.drain(..).partition(|job| job.run.is_finished());
        self.running = running;
        let mut recovered = Vec::new();
        for job in finished {
            self.ended.retain(|(kind, _)| *kind != job.kind);
            self.ended.push((job.kind, Instant::now()));
            match job.run.join() {
                Ok(Ok(Done::Recovered(Some(landing)))) if !landing.lanes.is_empty() => recovered.push(landing),
                Ok(Ok(Done::Pulled(reasons))) => reasons.iter().for_each(|reason| println!("north land: waiting: {reason}")),
                Ok(Ok(Done::Patrolled | Done::Recovered(_))) => {}
                Ok(Err(error)) => println!("north land: waiting: {}: {error}", job.kind.name()),
                Err(_) => println!("north land: the {} thread panicked", job.kind.name()),
            }
        }
        recovered
    }

    pub fn answering(&mut self, lander: &Lander) -> bool {
        if !self.running.iter().any(|job| job.kind == Kind::Sheriff) {
            return false;
        }
        if let Some((at, open)) = self.listed
            && at.elapsed() < REST
        {
            return open;
        }
        let open = sheriff::requests(lander).is_ok_and(|requests| !requests.is_empty());
        self.listed = Some((Instant::now(), open));
        open
    }

    pub fn running(&self) -> Vec<(Kind, Duration)> {
        self.running.iter().map(|job| (job.kind, job.started.elapsed())).collect()
    }
}

pub fn claimed(lander: &Lander) -> Vec<PathBuf> {
    after::recovering(lander).into_iter().map(|name| lander.queue.join(name)).collect()
}
