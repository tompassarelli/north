use regex::Regex;
use std::sync::LazyLock;

static FIELD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?(?:r#)?([a-z_][a-z0-9_]*)\s*:\s*([^,\n]+)").expect("field pattern"));
static LIFECYCLE: LazyLock<Regex> = LazyLock::new(|| {
    let words = "done|started|running|finished|failed|paused|stopped|active|pending|ready|closed|alive|dead|held|exhausted|landed|claimed|locked|published|spent|stale|exited|blocked|cancelled|canceled|complete|completed|waiting|retired|armed|killed|merged|in_progress|aborted|succeeded|passed|open|opened|draining|drained|parked|suspended|resumed|expired|revoked|enabled|disabled|online|offline|busy|idle";
    Regex::new(&format!(r"^(?:(?:is|was|has_been)_[a-z0-9_]+|(?:[a-z0-9_]+_)?(?:{words}))$")).expect("lifecycle pattern")
});

pub(crate) struct Fields {
    pub(crate) lifecycle: Vec<String>,
    pub(crate) optional: usize,
}

/// A struct's lifecycle booleans and its count of optional fields, read from its declaration's text.
pub(crate) fn fields(declaration: &str) -> Fields {
    let Some(body) = declaration.find('{').map(|open| &declaration[open + 1..]) else {
        return Fields { lifecycle: Vec::new(), optional: 0 };
    };
    let mut fields = Fields { lifecycle: Vec::new(), optional: 0 };
    for found in FIELD.captures_iter(body) {
        let kind = found[2].trim().trim_end_matches(',');
        if kind == "bool" && LIFECYCLE.is_match(&found[1]) {
            fields.lifecycle.push(found[1].to_owned());
        } else if kind.starts_with("Option<") {
            fields.optional += 1;
        }
    }
    fields
}
