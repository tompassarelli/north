use std::process::{Command, Stdio};

pub trait GitHub {
    fn public(&self, url: &str) -> bool;
    fn release_exists(&self, tag: &str) -> bool;
    fn create_release(&self, tag: &str) -> Option<bool>;
}

pub struct Live;

impl GitHub for Live {
    fn public(&self, url: &str) -> bool {
        Command::new("gh")
            .args(["repo", "view", url, "--json", "visibility", "-q", ".visibility"])
            .output()
            .is_ok_and(|output| String::from_utf8_lossy(&output.stdout).trim() == "PUBLIC")
    }

    fn release_exists(&self, tag: &str) -> bool {
        Command::new("gh")
            .args(["release", "view", tag, "--json", "tagName"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    fn create_release(&self, tag: &str) -> Option<bool> {
        let created = Command::new("gh").args(["release", "create", tag, "--verify-tag", "--title", tag, "--notes-from-tag"]).status().ok()?;
        Some(created.success())
    }
}
