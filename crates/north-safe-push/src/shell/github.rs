use std::process::Command;

pub trait GitHub {
    fn public(&self, url: &str) -> bool;
}

pub struct Live;

impl GitHub for Live {
    fn public(&self, url: &str) -> bool {
        Command::new("gh")
            .args(["repo", "view", url, "--json", "visibility", "-q", ".visibility"])
            .output()
            .is_ok_and(|output| String::from_utf8_lossy(&output.stdout).trim() == "PUBLIC")
    }
}
