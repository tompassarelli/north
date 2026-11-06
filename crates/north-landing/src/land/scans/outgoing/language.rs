use std::path::Path;

use crate::landing::safe_push_state::cancelled;
use crate::landing::step::{Refused, Step, git_read};

pub fn language_gate(root: &Path, base: &str, head: &str) -> Step {
    let base = if base.is_empty() { git_read(root, &["hash-object", "-t", "tree", "/dev/null"])? } else { base.to_owned() };
    if !crate::language_gate::check(root, &base, head) {
        cancelled()?;
        eprintln!("safe-push: language gate refused the landing; NOT pushed.");
        return Err(Refused(1));
    }
    if !crate::build_cache_gate::check(root, &base, head) {
        cancelled()?;
        eprintln!("safe-push: build-cache gate refused the landing; NOT pushed.");
        return Err(Refused(1));
    }
    if !crate::runtime_floor_gate::check(root, &base, head) {
        cancelled()?;
        eprintln!("safe-push: runtime-floor gate refused the landing; NOT pushed.");
        return Err(Refused(1));
    }
    Ok(())
}
