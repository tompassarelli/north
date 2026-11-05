use crate::github_mirror::store::{Mirror, OPEN_ISSUES, OWNER, key};
use crate::github_mirror::sync::{self, Gh};
use crate::landing::mutant_sweep::Issues;
use north_core::foundation::clock::Timestamp;

pub(super) struct MirrorIssues;

impl Issues for MirrorIssues {
    fn body(&self, repo: &str, number: u64) -> Result<String, String> {
        let mirror = Mirror::local();
        sync::issues(&Gh, &mirror, repo, Timestamp::now().unix_seconds_f64())?;
        let copy = mirror.read(&key(repo, OPEN_ISSUES)).ok_or_else(|| format!("the GitHub mirror holds no open issues for {repo}"))?;
        copy.body
            .as_array()
            .into_iter()
            .flatten()
            .find(|node| node["number"].as_u64() == Some(number))
            .and_then(|node| node["body"].as_str().map(str::to_owned))
            .ok_or_else(|| format!("{repo}#{number} is not an open issue in the GitHub mirror"))
    }

    fn set_body(&self, repo: &str, number: u64, body: &str) -> Result<(), String> {
        let slug = format!("{OWNER}/{repo}");
        let output = north_core::platform::process::run_with_input_within("gh", &["issue", "edit", &number.to_string(), "-R", &slug, "--body-file", "-"], Some(body.as_bytes()), std::time::Duration::from_secs(60))
            .ok_or_else(|| format!("gh issue edit {slug}#{number} did not finish"))?;
        if output.status.success() { Ok(()) } else { Err(format!("gh issue edit {slug}#{number}: {}", String::from_utf8_lossy(&output.stderr).trim())) }
    }
}
