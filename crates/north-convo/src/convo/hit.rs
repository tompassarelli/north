#[derive(Clone)]
pub(super) struct Hit {
    pub id: i64,
    pub line: i64,
    pub ts: Option<String>,
    pub role: String,
    pub tool: Option<String>,
    pub bm25: f64,
    pub session: String,
    pub agent: Option<String>,
    pub path: String,
    pub project: Option<String>,
    pub text: String,
    pub w: f64,
}
