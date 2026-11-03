pub const ROLE_SQL: &str = "(CASE WHEN role <> 'worker' THEN role \
WHEN category = 'unknown' AND (lower(coalesce(brief_text, '')) LIKE '%category: review%' OR lower(coalesce(brief_text, '')) LIKE '%category:review%') THEN 'reviewer' \
WHEN category = 'unknown' AND outcome = 'done' AND minutes = 0 THEN 'judge' \
ELSE 'worker' END)";

pub fn worker_where() -> String {
    format!("{ROLE_SQL} = 'worker'")
}
