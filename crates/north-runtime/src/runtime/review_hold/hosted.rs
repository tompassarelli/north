//! The judging of main's red hosted runs on a head's ancestry for the promotion screen (north#1537): each run since the last that settles them, oldest first, against the run before it, a new case tripping only once its replay on a build host regresses (replay.rs).

use super::super::main_runs::{self, Kept, MainRun};
use super::super::replay;
use super::super::trip::Trip;
use super::Screen;

enum Judged {
    Trips(Trip),
    Passes,
    Replaying,
}

impl Screen {
    /// The refusal, if any, of the red hosted runs on `head`'s ancestry since its last green run, judged oldest first so that each is judged against the cases the run before it failed: a run's trip is written only for the cases it fails that the run before it did not and that fail again on a build host (`replay`), and every trip among them stands, so each asks its revert; while a run's replay runs, the runs after it wait for it.
    pub(super) fn hosted_trip(&self, head: &str, now: u64) -> Result<Result<(), String>, String> {
        let runs = self.judged_runs(head)?;
        if runs.is_empty() {
            return Err("no completed run of main is on its ancestry".to_owned());
        }
        let (mut known, mut replays) = (Vec::new(), true);
        for run in runs.into_iter().rev() {
            if !run.red {
                known.clear();
                continue;
            }
            let failed = self.failed(&run)?;
            match self.judged(&run, &known, &failed, replays, now)? {
                Judged::Trips(trip) => {
                    let stood = self.stands(&trip, head, now)?;
                    if stood.is_err() {
                        return Ok(stood);
                    }
                }
                Judged::Replaying => replays = false,
                Judged::Passes => {}
            }
            known = failed;
        }
        Ok(Ok(()))
    }

    /// What red judged `run`, failing `failed`, says against the cases `known` the run before it failed: its trip, kept or written once its replay settles; nothing when it is cleared, unreproduced or fails only known cases; else its replay, started only when `replays`, as running.
    fn judged(&self, run: &MainRun, known: &[(String, String)], failed: &[(String, String)], replays: bool, now: u64) -> Result<Judged, String> {
        if self.trips.cleared(&run.head)? {
            return Ok(Judged::Passes);
        }
        if let Some(trip) = self.trips.read(&run.head)? {
            return Ok(Judged::Trips(trip));
        }
        let asked = Trip { head: run.head.clone(), run: Some(run.id.clone()), cases: failed.iter().filter(|case| !known.contains(case)).cloned().collect() };
        if self.trips.unreproduced(&run.head)? || (asked.cases.is_empty() && !failed.is_empty()) {
            return Ok(Judged::Passes);
        }
        if !replays {
            return Ok(Judged::Replaying);
        }
        let Some(compared) = (self.replay)(&self.trips, &asked, now)? else { return Ok(Judged::Replaying) };
        Ok(replay::settle(&self.trips, &asked, &compared, now, &self.runs.alert)?.map_or(Judged::Passes, Judged::Trips))
    }


    /// The completed runs of main on `head`'s ancestry that give a verdict, newest first, back to the first that settles the runs before it: a green run, a cleared trip or a trip whose revert was asked; runs are read page by page until it is found or the list ends.
    fn judged_runs(&self, head: &str) -> Result<Vec<MainRun>, String> {
        let mut judged = Vec::new();
        let mut number = 1;
        while let Some(runs) = (self.runs.page)(number)? {
            for run in runs {
                if !self.judged_on(&run, head)? {
                    continue;
                }
                let settles = self.settles(&run)?;
                judged.push(run);
                if settles {
                    return Ok(judged);
                }
            }
            number += 1;
        }
        Ok(judged)
    }

    fn judged_on(&self, run: &MainRun, head: &str) -> Result<bool, String> {
        Ok((self.contains)(&run.head, head)? && !self.unjudged(run)?)
    }

    /// Whether `run` settles the runs before it: green, or a trip cleared or whose revert was asked.
    fn settles(&self, run: &MainRun) -> Result<bool, String> {
        Ok(!run.red || self.trips.cleared(&run.head)? || self.trips.revert_asked(&run.head)?)
    }

    /// Whether red `run` gives no verdict (main_runs::judge), recorded once with the proxy told; a run whose log was read before keeps its verdict, and a read of the run that fails is an error that records nothing, so the head is refused as not screened and the next screen reads it again.
    fn unjudged(&self, run: &MainRun) -> Result<bool, String> {
        if !run.red || self.trips.cleared(&run.head)? || self.trips.read(&run.head)?.is_some() || self.trips.failed(&run.head)?.is_some() {
            return Ok(false);
        }
        if self.trips.unjudged(&run.head)? {
            return Ok(true);
        }
        let mut log = String::new();
        let verdict = main_runs::judge(run.red, || {
            let kept = (self.runs.kept)(&run.id)?;
            if let Kept::Log(text) = &kept {
                log.clone_from(text);
            }
            Ok(kept)
        })?;
        if let main_runs::Verdict::NotJudged(why) = verdict {
            self.trips.write_unjudged(&run.head, &run.id, &why)?;
            (self.runs.alert)(&format!("north runtime: the hosted Buck2 run {} on {} is not judged and trips nothing: {why}", run.id, run.head));
            return Ok(true);
        }
        self.trips.write_failed(&Trip { head: run.head.clone(), run: Some(run.id.clone()), cases: main_runs::failed_cases(&log) })?;
        Ok(false)
    }

    /// The failed cases of red judged `run`, as `unjudged` kept them, or a trip record written before they were kept.
    fn failed(&self, run: &MainRun) -> Result<Vec<(String, String)>, String> {
        match self.trips.failed(&run.head)? {
            Some(cases) => Ok(cases),
            None => Ok(self.trips.read(&run.head)?.map(|trip| trip.cases).unwrap_or_default()),
        }
    }
}
