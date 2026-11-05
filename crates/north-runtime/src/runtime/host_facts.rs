use super::bounded;
use super::layout::Layout;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

const RECORD_LIMIT: Duration = Duration::from_secs(120);
const PRIVATE_CHECKOUT: &str = "code/south/main";

pub(super) fn record_after_promotion(layout: &Layout, target: &str) {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else { return };
    let Some(repo) = private_checkout(&home) else {
        println!("north runtime: no {PRIVATE_CHECKOUT} checkout under {}; this host's config facts stay unpublished", home.display());
        return;
    };
    let mut command = Command::new(layout.runtime(target).join("north"));
    command.args(["config", "agents", "live-refs", "--record", "--repo"]).arg(&repo).stdin(Stdio::null());
    match bounded::status(&mut command, RECORD_LIMIT) {
        Ok(status) if status.success() => {}
        Ok(status) => println!("north runtime: recording this host's config facts exited {status}"),
        Err(error) => println!("north runtime: recording this host's config facts failed: {error}"),
    }
}

fn private_checkout(home: &Path) -> Option<PathBuf> {
    Some(home.join(PRIVATE_CHECKOUT)).filter(|repo| repo.join(".git").exists())
}

#[cfg(test)]
mod tests {
    use super::private_checkout;

    #[test]
    fn facts_are_published_only_through_an_existing_south_checkout() {
        let home = tempfile::tempdir().expect("home");
        assert_eq!(private_checkout(home.path()), None);
        std::fs::create_dir_all(home.path().join("code/south/main/.git")).expect("checkout");
        assert_eq!(private_checkout(home.path()), Some(home.path().join("code/south/main")));
    }
}
