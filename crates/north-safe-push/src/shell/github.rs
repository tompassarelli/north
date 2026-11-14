use std::process::{Command, Stdio};

pub trait GitHub {
    fn public(&self, url: &str) -> bool;
    fn release_exists(&self, tag: &str) -> bool;
    fn create_release(&self, tag: &str) -> Option<bool>;
}

pub struct Live;

impl GitHub for Live {
    fn public(&self, url: &str) -> bool {
        let answer = Command::new("gh")
            .args(["repo", "view", url, "--json", "visibility", "-q", ".visibility"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
            .filter(|visibility| !visibility.is_empty());
        answer.map_or_else(|| url.contains("github.com"), |visibility| visibility == "PUBLIC")
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

pub fn private_destination(public: bool, to: &str, default: &str) -> Result<(), String> {
    if !public {
        return Ok(());
    }
    Err(format!(
        "safe-push: origin is public, so it carries no branch beside {default} and only the lander pushes to it; NOT pushed {to}.\n\
         safe-push: check a commit on a build host with north capacity offload run --base HEAD --head HEAD (it hands the commit over as a bundle, never through origin), keep work in its lane or push it to a private remote, and land with safe-push --to {default}\n"
    ))
}
