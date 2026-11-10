//! Refuse a GitHub Actions job a lane adds or changes that runs on a push to main and neither runs
//! before landing as a `land.check` nor carries a `land.after-only` reason, the same rule the
//! after-only reason lane gate applies.

use std::io::Write;
use std::path::Path;

use north_land_scans::Range;
use north_land_scans::push_jobs;

use super::super::slots::select::Config;

pub fn scan(repo: &Path, range: &str, out: &mut impl Write) -> u8 {
    let Some(range) = Range::parse(range) else {
        let _ = writeln!(out, "push-jobs: expected BASE..HEAD");
        return 1;
    };
    let config = Config::load(repo, range.head);
    let unnamed = match unnamed(repo, range.base, range.head, &config) {
        Ok(unnamed) => unnamed,
        Err(error) => {
            let _ = writeln!(out, "push-jobs: cannot read the workflows this lane adds to a push to main: {error}");
            return 1;
        }
    };
    for name in &unnamed {
        let _ = writeln!(out, "push-jobs: this lane adds or changes the GitHub Actions job `{name}`, which runs on a push to main; make each of its run steps a land.check so it runs before, or record why it cannot with `land.after-only = REASON :: {name}`");
    }
    u8::from(!unnamed.is_empty())
}

/// The `FILE:JOB` names the lane adds or changes that a push to main runs and `config` neither checks
/// before landing nor records an after-only reason for.
pub fn unnamed(repo: &Path, base: &str, head: &str, config: &Config) -> Result<Vec<String>, String> {
    let jobs = push_jobs::newly_on_main(repo, base, head)?;
    let unnamed = jobs.iter().filter(|job| !job.runs_only(|command| config.runs(command)) && config.after_only_reason(&job.name).is_none());
    Ok(unnamed.map(|job| job.name.clone()).collect())
}
