use super::{Options, Outcome, Repository, Runtime, link_hold, output, swap, unit};
use std::fs;
use std::path::Path;

/// Points the lander link at `target` before any replay: land-first applies here as it does to a lane, so the runtime
/// current names runs the lander as soon as its canary passed, and the recorded-round replay and shadow judge it after.
/// Returns a settled line instead when another runtime already runs ahead of `target` or current moved before the link did.
fn move_first(options: &Options, runtime: &Runtime, repositories: &[Repository], target: &Path) -> Result<Option<String>, String> {
    let checkout = repositories.first().map(|repository| repository.checkout.as_path());
    let sha = |link: &Path| link.file_name().map(|name| name.to_string_lossy().into_owned());
    let descends = |earlier: &Path, later: &Path| match (checkout, sha(earlier), sha(later)) {
        (Some(checkout), Some(earlier), Some(later)) => output(checkout, &["merge-base", "--is-ancestor", &earlier, &later]).is_ok_and(|output| output.status.success()),
        _ => false,
    };
    if let Some(ahead) = fs::read_link(runtime.lander()).ok().filter(|ahead| *ahead != target && descends(target, ahead)) {
        return Ok(Some(format!("the lander already runs {}, newer than {}; the link is not moved back", ahead.display(), target.display())));
    }
    let now = fs::read_link(runtime.current()).ok();
    if now.as_deref() != Some(target) {
        let moved = now.map_or_else(|| "nothing".to_owned(), |link| link.display().to_string());
        return Ok(Some(format!("{} moved from {} to {moved} before the lander link moved; the next promotion follows it", runtime.current().display(), target.display())));
    }
    if options.dry_run {
        println!("north land promote: dry run: the lander link would point at {}", target.display());
        return Ok(None);
    }
    unit::stop(runtime);
    if link_hold::rollback_of(&runtime.home).is_none() && let Some(previous) = fs::read_link(runtime.lander()).ok().filter(|previous| previous != target) {
        link_hold::record_rollback(&runtime.home, &previous);
    }
    swap(&runtime.lander(), target).map_err(|error| format!("{}: {error}; the lander link is unchanged", runtime.lander().display()))?;
    println!("north land promote: the lander link now points at {}; the recorded-round replay and shadow judge it after", target.display());
    Ok(None)
}

/// Moves the link first and hands the replay that judges it to its unit; `None` means this promotion judges the candidate itself, on a dry run or when the unit could not start.
pub(super) fn arrive(options: &Options, runtime: &Runtime, repositories: &[Repository], binary: &Path, target: &Path, held: Option<&Path>) -> Result<Option<Outcome>, String> {
    if let Some(result) = move_first(options, runtime, repositories, target)? {
        return Ok(Some(Outcome::Settled(result)));
    }
    if options.dry_run {
        return Ok(None);
    }
    match unit::hand_off(runtime, options, binary, target, held) {
        Ok(handed) => Ok(Some(Outcome::Handed(handed))),
        Err(why) => {
            println!("north land promote: {why}; this promotion replays the recorded rounds itself");
            Ok(None)
        }
    }
}
