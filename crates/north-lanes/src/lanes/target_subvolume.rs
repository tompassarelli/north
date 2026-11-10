use std::fs;
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

use super::target_seed::{STAGING_PREFIX, Staging, workspace_root};

const BTRFS_SUPER_MAGIC: u64 = 0x9123_683e;
const CACHEDIR_TAG: &str = "Signature: 8a477f597d28d172789f06886806bc55\n# This file is a cache directory tag created by cargo.\n# For information about cache directory tags see https://bford.info/cachedir/\n";

pub(super) fn fresh_target(here: &Path) -> io::Result<()> {
    let Some(workspace) = workspace_root(here) else { return Ok(()) };
    let target = workspace.join("target");
    if target.exists() || !on_btrfs(&workspace) {
        return Ok(());
    }
    let staging = Staging::new(workspace.join(format!("{STAGING_PREFIX}{}", std::process::id())));
    target_dir(&staging.path)?;
    fs::write(staging.path.join("CACHEDIR.TAG"), CACHEDIR_TAG)?;
    if fs::rename(&staging.path, &target).is_ok() {
        staging.keep();
    }
    Ok(())
}

pub(super) fn target_dir(path: &Path) -> io::Result<()> {
    if on_btrfs(path.parent().unwrap_or(path)) {
        let created = Command::new("btrfs").args(["subvolume", "create"]).arg(path).stdout(Stdio::null()).status();
        return match created {
            Ok(status) if status.success() => Ok(()),
            _ => Err(io::Error::other(format!("could not create the btrfs subvolume {}", path.display()))),
        };
    }
    fs::create_dir(path)
}

fn on_btrfs(dir: &Path) -> bool {
    rustix::fs::statfs(dir).is_ok_and(|stat| u64::try_from(stat.f_type).is_ok_and(|kind| kind == BTRFS_SUPER_MAGIC))
}
