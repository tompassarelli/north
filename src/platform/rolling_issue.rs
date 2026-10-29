use super::github;

const REPO: &str = "tompassarelli/north";

pub(crate) struct Issue {
    pub(crate) number: u64,
    pub(crate) body: String,
}

pub(crate) trait Tracker {
    fn open(&self, title: &str) -> Result<Option<Issue>, String>;
    fn listed(&self, search: &str) -> Result<Vec<(u64, String)>, String>;
    fn create(&mut self, title: &str, body: &str) -> Result<(), String>;
    fn edit(&mut self, number: u64, body: &str) -> Result<(), String>;
    fn close(&mut self, number: u64, comment: &str) -> Result<(), String>;
}

pub(crate) fn settle(tracker: &mut dyn Tracker, title: &str, body: Option<&str>, closing: &str) -> Result<(), String> {
    match (tracker.open(title)?, body) {
        (None, None) => {}
        (Some(issue), None) => tracker.close(issue.number, closing)?,
        (None, Some(body)) => tracker.create(title, body)?,
        (Some(issue), Some(body)) => {
            if issue.body.trim() != body.trim() {
                tracker.edit(issue.number, body)?;
            }
        }
    }
    Ok(())
}

pub(crate) struct Gh;

impl Tracker for Gh {
    fn open(&self, title: &str) -> Result<Option<Issue>, String> {
        let search = format!("in:title \"{title}\"");
        let found = github::json(&["issue", "list", "-R", REPO, "--state", "open", "--search", &search, "--json", "number,title,body"])?;
        Ok(found.as_array().into_iter().flatten().find(|issue| issue["title"] == title).and_then(|issue| {
            Some(Issue { number: issue["number"].as_u64()?, body: issue["body"].as_str().unwrap_or_default().to_owned() })
        }))
    }

    fn listed(&self, search: &str) -> Result<Vec<(u64, String)>, String> {
        let found = github::json(&["issue", "list", "-R", REPO, "--state", "open", "--search", search, "--json", "number,title"])?;
        Ok(found.as_array().into_iter().flatten().filter_map(|issue| Some((issue["number"].as_u64()?, issue["title"].as_str()?.to_owned()))).collect())
    }

    fn create(&mut self, title: &str, body: &str) -> Result<(), String> {
        succeeded(github::run(&["issue", "create", "-R", REPO, "--title", title, "--body", body]), "issue create")
    }

    fn edit(&mut self, number: u64, body: &str) -> Result<(), String> {
        succeeded(github::run(&["issue", "edit", &number.to_string(), "-R", REPO, "--body", body]), "issue edit")
    }

    fn close(&mut self, number: u64, comment: &str) -> Result<(), String> {
        succeeded(github::run(&["issue", "close", &number.to_string(), "-R", REPO, "--comment", comment]), "issue close")
    }
}

fn succeeded(out: std::io::Result<std::process::Output>, what: &str) -> Result<(), String> {
    let out = out.map_err(|error| format!("gh {what}: {error}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("gh {what}: {}", String::from_utf8_lossy(&out.stderr).trim()))
    }
}
