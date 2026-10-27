use super::process;
use std::process::Output;

pub(crate) fn run(args: &[&str]) -> std::io::Result<Output> {
    process::run("gh", None, args)
}

pub(crate) fn run_owned(args: &[String]) -> std::io::Result<Output> {
    process::run("gh", None, args)
}

pub(crate) async fn run_async(args: &[String]) -> std::io::Result<Output> {
    process::run_killing_on_drop("gh", args).await
}

pub(crate) fn json(args: &[&str]) -> Result<serde_json::Value, String> {
    let out = run(args).map_err(|error| error.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    serde_json::from_slice(&out.stdout).map_err(|error| error.to_string())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BranchTip {
    pub(crate) branch: String,
    pub(crate) sha: String,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum PostError {
    Forbidden(String),
    Failed(String),
}

impl PostError {
    pub(crate) fn classify(error: String) -> Self {
        if error.contains("HTTP 403") { Self::Forbidden(error) } else { Self::Failed(error) }
    }
}

pub(crate) trait CommitStatuses {
    fn matching_branches(&self, repo: &str, prefix: &str) -> Result<Vec<BranchTip>, String>;
    fn status_state(&self, repo: &str, sha: &str, context: &str) -> Result<Option<String>, String>;
    fn post_status(&self, repo: &str, sha: &str, context: &str, state: &str, description: &str) -> Result<(), PostError>;
    fn dispatch_workflow(&self, repo: &str, workflow: &str, branch: &str) -> Result<(), String>;
    fn clone_url(&self, repo: &str) -> String;
}

pub(crate) struct Gh;

fn succeeded(args: &[String]) -> Result<Vec<u8>, String> {
    let out = run_owned(args).map_err(|error| format!("cannot run gh: {error}"))?;
    if !out.status.success() {
        return Err(format!("gh {} failed: {}", args[0], String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(out.stdout)
}

fn owned(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_owned()).collect()
}

impl CommitStatuses for Gh {
    fn matching_branches(&self, repo: &str, prefix: &str) -> Result<Vec<BranchTip>, String> {
        let endpoint = format!("repos/{repo}/git/matching-refs/heads/{prefix}");
        let stdout = succeeded(&owned(&["api", "--paginate", &endpoint, "--jq", ".[] | \"\\(.ref) \\(.object.sha)\""]))?;
        Ok(String::from_utf8_lossy(&stdout)
            .lines()
            .filter_map(|line| {
                let (name, sha) = line.split_once(' ')?;
                Some(BranchTip { branch: name.strip_prefix("refs/heads/")?.to_owned(), sha: sha.to_owned() })
            })
            .collect())
    }

    fn status_state(&self, repo: &str, sha: &str, context: &str) -> Result<Option<String>, String> {
        let endpoint = format!("repos/{repo}/commits/{sha}/statuses");
        let stdout = succeeded(&owned(&["api", "--paginate", &endpoint, "--jq", ".[] | \"\\(.context)\\t\\(.state)\""]))?;
        Ok(String::from_utf8_lossy(&stdout)
            .lines()
            .find_map(|line| line.split_once('\t').filter(|(held, _)| *held == context).map(|(_, state)| state.to_owned())))
    }

    fn post_status(&self, repo: &str, sha: &str, context: &str, state: &str, description: &str) -> Result<(), PostError> {
        let endpoint = format!("repos/{repo}/statuses/{sha}");
        let fields = [format!("state={state}"), format!("context={context}"), format!("description={description}")];
        let mut args = owned(&["api", "--method", "POST", &endpoint, "--silent"]);
        fields.into_iter().for_each(|field| args.extend(["-f".to_owned(), field]));
        succeeded(&args).map(drop).map_err(PostError::classify)
    }

    fn dispatch_workflow(&self, repo: &str, workflow: &str, branch: &str) -> Result<(), String> {
        succeeded(&owned(&["workflow", "run", workflow, "-R", repo, "-f", &format!("branch={branch}")])).map(drop)
    }

    fn clone_url(&self, repo: &str) -> String {
        format!("https://github.com/{repo}.git")
    }
}
