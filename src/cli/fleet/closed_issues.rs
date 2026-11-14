use crate::fleet::registry::rewritten::ClosedIssues;

const OWNER: &str = "tompassarelli";

/// Reads a closed issue's comments with `gh issue view`.
pub(super) struct GitHub;

impl ClosedIssues for GitHub {
    fn closed_comments(&self, task: &str) -> Vec<String> {
        let Some((repository, number)) = task.split_once('#') else { return Vec::new() };
        let repository = if repository.contains('/') { repository.to_owned() } else { format!("{OWNER}/{repository}") };
        let Ok(found) = crate::platform::github::json(&["issue", "view", number, "-R", &repository, "--json", "state,comments"]) else { return Vec::new() };
        if found["state"] != "CLOSED" {
            return Vec::new();
        }
        found["comments"].as_array().into_iter().flatten().filter_map(|comment| comment["body"].as_str().map(str::to_owned)).collect()
    }
}
