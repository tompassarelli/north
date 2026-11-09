use std::path::Path;
use std::process::Command;

use super::route::Visibility;

/// The visibility of the GitHub repository a lane's `origin` names; unknown when the remote, `gh` or the answer cannot be read.
pub fn lane_visibility(lane: &Path) -> Visibility {
    let Some(slug) = origin_slug(lane) else { return Visibility::Unknown };
    let output = Command::new("gh").args(["repo", "view", &slug, "--json", "visibility", "-q", ".visibility"]).output();
    match output {
        Ok(output) if output.status.success() => Visibility::from_github(&String::from_utf8_lossy(&output.stdout)),
        _ => Visibility::Unknown,
    }
}

fn origin_slug(lane: &Path) -> Option<String> {
    let output = Command::new("git").arg("-C").arg(lane).args(["remote", "get-url", "origin"]).output().ok().filter(|output| output.status.success())?;
    slug(String::from_utf8_lossy(&output.stdout).trim())
}

pub(super) fn slug(url: &str) -> Option<String> {
    let path = url.strip_prefix("git@github.com:").or_else(|| url.strip_prefix("https://github.com/")).or_else(|| url.strip_prefix("ssh://git@github.com/"))?;
    let path = path.trim_end_matches('/').trim_end_matches(".git");
    (path.matches('/').count() == 1).then(|| path.to_owned())
}
