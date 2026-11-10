pub const ROLE_SQL: &str = "(CASE WHEN role <> 'worker' THEN role \
WHEN category = 'review' THEN 'reviewer' \
WHEN category = 'unknown' AND (lower(coalesce(brief_text, '')) LIKE '%category: review%' OR lower(coalesce(brief_text, '')) LIKE '%category:review%') THEN 'reviewer' \
WHEN instr(lower(substr(coalesce(brief_text, ''), 1, 1500)), 'review') > 0 AND (instr(lower(substr(coalesce(brief_text, ''), 1, 1500)), 'read-only') > 0 OR instr(lower(substr(coalesce(brief_text, ''), 1, 1500)), 'do not edit') > 0) THEN 'reviewer' \
WHEN category = 'unknown' AND outcome NOT IN ('idle', 'running') AND minutes = 0 THEN 'judge' \
ELSE 'worker' END)";

/// The characters of a brief the read-only review rule reads, as `ROLE_SQL` reads them with substr.
const BRIEF_HEAD_CHARS: usize = 1500;

pub fn worker_where() -> String {
    format!("{ROLE_SQL} = 'worker'")
}

/// A brief that sends its session to review something without editing it: a reviewer or test-keep judge, not a worker on a box.
fn read_only_review(brief: &str) -> bool {
    let head: String = brief.chars().take(BRIEF_HEAD_CHARS).collect::<String>().to_lowercase();
    head.contains("review") && (head.contains("read-only") || head.contains("do not edit"))
}

/// The stored role of a finished worker run, by the rule `ROLE_SQL` reads old rows with. A run that landed a commit is a worker: every ended run arrives `not done` for the ledger to settle, so a quick run whose brief names no category read as a judge, which the settle skips, and verify-north's landed worker stayed `not done` (north#1022).
pub fn ingest_role(category: &str, brief: &str, outcome: &str, minutes: i64, landed: bool) -> &'static str {
    let lowered = brief.to_lowercase();
    let declared = category == "unknown" && (lowered.contains("category: review") || lowered.contains("category:review"));
    if category == "review" || declared || read_only_review(brief) {
        "reviewer"
    } else if category == "unknown" && !matches!(outcome, "idle" | "running") && minutes == 0 && !landed {
        "judge"
    } else {
        "worker"
    }
}
