use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::{Repository, SHADOW_PAUSE, run_replay};
use crate::land::queue;

fn shadow_copy(queue: &Path, scratch: &Path) -> Result<Vec<PathBuf>, String> {
    let pending = queue::pending(queue).map_err(|error| format!("{}: {error}", queue.display()))?;
    pending
        .into_iter()
        .map(|(path, ticket)| {
            let copy = scratch.join(path.file_name().unwrap_or_default());
            queue::write(&copy, &ticket).map(|()| copy).map_err(|error| error.to_string())
        })
        .collect()
}

pub(super) fn shadow(binary: &Path, repositories: &[Repository], checker: &str, minutes: u64) -> Result<String, String> {
    let deadline = Instant::now() + Duration::from_secs(minutes * 60);
    let mut errors: HashSet<String> = HashSet::new();
    let (mut passes, mut checked) = (0, 0);
    loop {
        passes += 1;
        for repository in repositories {
            let scratch = tempfile::tempdir().map_err(|error| error.to_string())?;
            let tickets = shadow_copy(&repository.queue, scratch.path())?;
            if tickets.is_empty() {
                continue;
            }
            checked += 1;
            let error = match run_replay(binary, &repository.checkout, "FETCH_HEAD", checker, &tickets) {
                Ok(replayed) => {
                    println!("north land promote: shadow pass {passes}: {} queued lane(s) of {} replayed {}", tickets.len(), repository.checkout.display(), replayed.verdict);
                    replayed.verdict.starts_with("transient").then_some(replayed.verdict)
                }
                Err(error) => Some(error),
            };
            if let Some(error) = error {
                println!("north land promote: shadow pass {passes}: error {error}");
                if !errors.insert(error.clone()) {
                    return Err(format!("the shadow lander repeated an error: {error}"));
                }
            }
        }
        if Instant::now() + SHADOW_PAUSE >= deadline {
            break;
        }
        std::thread::sleep(SHADOW_PAUSE);
    }
    Ok(format!("{passes} pass(es), {checked} candidate(s) checked, {} error(s), none repeated", errors.len()))
}

