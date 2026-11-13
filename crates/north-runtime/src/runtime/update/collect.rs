//! The runtime collection north-runtime-follow performs, read over this host's process table and agent links.

use super::super::layout::Layout;
use std::path::{Path, PathBuf};

pub(crate) fn collect(layout: &Layout, target: &str, previous: Option<&Path>) -> std::io::Result<()> {
    collect_beside(layout, target, previous, Path::new("/proc"), &super::agent_link::targets())
}

pub(super) fn collect_beside(layout: &Layout, target: &str, previous: Option<&Path>, proc_root: &Path, agent_targets: &[PathBuf]) -> std::io::Result<()> {
    north_runtime_follow::collection::collect(layout, target, previous, proc_root, agent_targets)
}
