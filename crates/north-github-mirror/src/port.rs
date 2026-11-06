use serde_json::Value;

pub const REFETCH_S: f64 = 30.0 * 60.0;

#[derive(Debug, PartialEq)]
pub enum Answer {
    Unchanged,
    Changed { body: Value, etag: Option<String> },
}

pub trait GitHub {
    fn get(&self, endpoint: &str, etag: Option<&str>) -> Result<Answer, String>;
    fn open_issues(&self, repo: &str) -> Result<Vec<Value>, String>;
    fn closed_issues(&self, repo: &str) -> Result<Vec<Value>, String>;
    fn branches(&self, repo: &str) -> Result<Vec<Value>, String>;
    fn pages(&self, repo: &str) -> Result<Value, String>;
    fn repo_list(&self) -> Result<Value, String>;
    fn compares(&self, repo: &str, branches: &[String]) -> Result<Vec<Value>, String>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fetch {
    Unchanged,
    Refreshed,
}
