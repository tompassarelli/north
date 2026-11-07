use regex::Regex;
use std::sync::LazyLock;

static COMMIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bgit\b[^\n|;&]*\bcommit\b|\bsafe-push\b").expect("commit pattern"));
static CHECK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bcargo\s+(?:test|clippy|nextest)\b|\bbun\s+(?:test|run\s+check)\b|\s--check\b|\.test\.sh\b|\bverify-north\b").expect("check pattern"));

/// Whether a tool call is a commit or a check run, the work that spends a try.
pub fn did_work(tool: &str) -> bool {
    COMMIT.is_match(tool) || CHECK.is_match(tool)
}
