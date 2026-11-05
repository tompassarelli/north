pub const ROLE_SQL: &str = "(CASE WHEN role <> 'worker' THEN role \
WHEN category = 'unknown' AND (lower(coalesce(brief_text, '')) LIKE '%category: review%' OR lower(coalesce(brief_text, '')) LIKE '%category:review%') THEN 'reviewer' \
WHEN category = 'unknown' AND outcome = 'done' AND minutes = 0 THEN 'judge' \
ELSE 'worker' END)";

pub fn worker_where() -> String {
    format!("{ROLE_SQL} = 'worker'")
}

/// The stored role of a finished worker run, by the rule `ROLE_SQL` reads old rows with.
pub fn ingest_role(category: &str, brief: &str, outcome: &str, minutes: i64) -> &'static str {
    let brief = brief.to_lowercase();
    if category != "unknown" {
        "worker"
    } else if brief.contains("category: review") || brief.contains("category:review") {
        "reviewer"
    } else if outcome == "done" && minutes == 0 {
        "judge"
    } else {
        "worker"
    }
}
