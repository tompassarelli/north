use super::units;
use super::{Verdict, main_red};
use crate::platform::rolling_issue::Gh;

pub(super) fn file(results: &[(String, Verdict)], north_runs: Option<&str>) {
    let failing: Vec<(String, String)> = results
        .iter()
        .filter_map(|(check, result)| Some((check.strip_prefix("unit ")?.to_owned(), result.as_ref().err()?.clone())))
        .collect();
    if let Err(error) = units::reconcile(&mut Gh, &crate::platform::host::name(), &failing) {
        eprintln!("health: issue: {error}");
    }
    if results.iter().any(|(check, result)| check == "ci" && result.is_ok())
        && let Some(runs) = north_runs
        && let Err(error) = main_red::settle(&mut Gh, runs)
    {
        eprintln!("health: main-red: {error}");
    }
}
