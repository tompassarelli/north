use north_mutants::sweep::Issues;

const OWNER: &str = "tompassarelli";

pub(super) struct GhIssues;

impl Issues for GhIssues {
    fn body(&self, repo: &str, number: u64) -> Result<String, String> {
        let slug = format!("{OWNER}/{repo}");
        let issue = north_base::platform::github::json(&["issue", "view", &number.to_string(), "-R", &slug, "--json", "body"])?;
        issue["body"].as_str().map(str::to_owned).ok_or_else(|| format!("{slug}#{number} has no body"))
    }

    fn set_body(&self, repo: &str, number: u64, body: &str) -> Result<(), String> {
        let slug = format!("{OWNER}/{repo}");
        let output = north_base::platform::process::run_with_input_within("gh", &["issue", "edit", &number.to_string(), "-R", &slug, "--body-file", "-"], Some(body.as_bytes()), std::time::Duration::from_secs(60))
            .ok_or_else(|| format!("gh issue edit {slug}#{number} did not finish"))?;
        if output.status.success() { Ok(()) } else { Err(format!("gh issue edit {slug}#{number}: {}", String::from_utf8_lossy(&output.stderr).trim())) }
    }

    fn comment(&self, repo: &str, number: u64, body: &str) -> Result<String, String> {
        let slug = format!("{OWNER}/{repo}");
        let output = north_base::platform::process::run_with_input_within("gh", &["issue", "comment", &number.to_string(), "-R", &slug, "--body-file", "-"], Some(body.as_bytes()), std::time::Duration::from_secs(60))
            .ok_or_else(|| format!("gh issue comment {slug}#{number} did not finish"))?;
        if output.status.success() { Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned()) } else { Err(format!("gh issue comment {slug}#{number}: {}", String::from_utf8_lossy(&output.stderr).trim())) }
    }
}
