//! The heads whose landing a revert is queued for, kept as files under the queue so a lander restart between the
//! Buck2 red and the Cargo verdict of one landing still knows it (north#1338).
use std::fs;
use std::path::{Path, PathBuf};

const DIR: &str = "reverted";

pub(super) struct Reverted {
    dir: PathBuf,
}

impl Reverted {
    pub(super) fn under(queue: &Path) -> Self {
        Self { dir: queue.join(DIR) }
    }

    pub(super) fn holds(&self, head: &str) -> bool {
        self.dir.join(head).exists()
    }

    /// Runs `queue` unless a revert was already queued for `head`, and records `head` when it returns true.
    pub(super) fn once(&self, head: &str, queue: impl FnOnce() -> bool) -> bool {
        if self.holds(head) || !queue() {
            return false;
        }
        if let Err(error) = fs::create_dir_all(&self.dir).and_then(|()| fs::write(self.dir.join(head), b"")) {
            println!("north land: could not record that a revert is queued for {head}, so a lander restart before its after-landing verdict can queue a second: {error}");
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::Reverted;

    #[test]
    fn a_restarted_watch_queues_no_second_revert_for_a_head_already_reverted() {
        let queue = tempfile::tempdir().unwrap();
        let mut queued = Vec::new();
        let mut revert = |watch: &Reverted, head: &str| {
            watch.once(head, || {
                queued.push(head.to_owned());
                println!("north land: revert of lane {head} queued as revert-{head}");
                true
            })
        };
        assert!(revert(&Reverted::under(queue.path()), "abc"));
        assert!(!revert(&Reverted::under(queue.path()), "abc"), "the watch after the restart does not revert it again");
        assert!(revert(&Reverted::under(queue.path()), "def"));
        assert_eq!(queued, ["abc", "def"]);
    }
}
