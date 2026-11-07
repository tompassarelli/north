use super::{Options, Outcome, Repository, Runtime, output, shadow};
use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn arrive(options: &Options, runtime: &Runtime, repositories: &[Repository], binary: &Path, target: PathBuf, replayed: usize) -> Result<Outcome, String> {
    let shadowed = shadow(binary, repositories, &options.checker, options.shadow_minutes)?;
    println!("north land promote: shadow: {shadowed}");
    let checkout = repositories.first().map(|repository| repository.checkout.as_path());
    let sha = |link: &Path| link.file_name().map(|name| name.to_string_lossy().into_owned());
    let descends = |earlier: &Path, later: &Path| match (checkout, sha(earlier), sha(later)) {
        (Some(checkout), Some(earlier), Some(later)) => output(checkout, &["merge-base", "--is-ancestor", &earlier, &later]).is_ok_and(|output| output.status.success()),
        _ => false,
    };
    if let Some(ahead) = fs::read_link(runtime.lander()).ok().filter(|ahead| *ahead != target && descends(&target, ahead)) {
        return Ok(Outcome::Settled(format!("the lander already runs {}, newer than {}; the link is not moved back", ahead.display(), target.display())));
    }
    let now = fs::read_link(runtime.current()).ok();
    if now.as_deref() != Some(target.as_path()) && !now.as_deref().is_some_and(|now| descends(&target, now)) {
        let moved = now.map_or_else(|| "nothing".to_owned(), |link| link.display().to_string());
        return Err(format!("{} moved from {} to {moved} while the candidate replayed", runtime.current().display(), target.display()));
    }
    if options.dry_run {
        return Ok(Outcome::Settled(format!("dry run: {replayed} round(s) matched; {} would point at {}", runtime.lander().display(), target.display())));
    }
    Ok(Outcome::Swap { link: runtime.lander(), target })
}

