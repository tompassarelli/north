use super::policy::Defaults;
use regex::Regex;
use std::collections::BTreeMap;
use std::sync::LazyLock;

static FIELD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([A-Za-z][A-Za-z ]*?):[ \t]*(.*)$").unwrap());
static WORD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([a-z0-9-]+)(?:\s+\(([^()]*)\))?$").unwrap());
static TOKEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[a-z0-9-]+").unwrap());
static MINUTES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:(\d+)\s*-\s*)?(\d+)\s*(?:m|mins?|minutes?)?(?:\s+\([^()]*\))?$").unwrap()
});
static COMMAND: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`[^`\n]+`").unwrap());
static PATH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_./-]+$").unwrap());
static BULLET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[-*]\s+").unwrap());

pub fn brief_header(text: &str) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    let mut last = None;
    for line in text.lines() {
        if line.trim().is_empty() {
            break;
        }
        if let Some(captures) = FIELD.captures(line) {
            let name = captures[1].to_lowercase();
            fields
                .entry(name.clone())
                .or_insert_with(|| captures[2].to_owned());
            last = Some(name);
        } else if (line.starts_with(char::is_whitespace)
            || line.trim_start().starts_with("- ")
            || line.trim_start().starts_with("* "))
            && let Some(name) = &last
            && let Some(value) = fields.get_mut(name)
        {
            value.push('\n');
            value.push_str(line);
        }
    }
    fields
}

pub fn header_word(
    fields: &BTreeMap<String, String>,
    name: &str,
    defaults: &Defaults,
) -> Option<String> {
    let raw = fields.get(name)?.trim().to_lowercase();
    let captures = WORD.captures(&raw)?;
    let facets: Vec<&str> = match name {
        "spec" => vec!["exact", "measured", "judged"],
        "scope" => vec!["one-file", "module", "cross-module", "cross-repo"],
        "surface" => vec!["shell", "docs", "nix", "ts", "assets", "lua", "workflow"],
        "verify" => vec!["none", "local-test", "native", "visual", "farm"],
        _ => defaults.codex_route.keys().map(String::as_str).collect(),
    };
    if captures.get(2).is_some_and(|note| {
        TOKEN
            .find_iter(note.as_str())
            .any(|token| facets.contains(&token.as_str()))
    }) {
        return None;
    }
    Some(captures[1].to_owned())
}

pub fn header_minutes(value: &str) -> Option<u64> {
    let raw = value.trim().to_lowercase();
    let captures = MINUTES.captures(&raw)?;
    let end = captures[2].parse().ok()?;
    if let Some(start) = captures.get(1)
        && start.as_str().parse::<u64>().ok()? > end
    {
        return None;
    }
    Some(end)
}

pub fn header_files(value: &str) -> Vec<String> {
    value
        .split([',', '\n'])
        .filter_map(|name| {
            let name = name.trim();
            let name = BULLET.replace(name, "");
            let name = name.trim();
            let name = name.trim_matches(['`', '"', '\'']);
            (!name.is_empty()).then(|| name.to_owned())
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexClass {
    pub route: String,
    pub tiers: Vec<String>,
}

pub fn codex_class(category: Option<&str>, defaults: &Defaults) -> CodexClass {
    CodexClass {
        route: category
            .and_then(|c| defaults.codex_route.get(c))
            .cloned()
            .unwrap_or_else(|| "claude".into()),
        tiers: category
            .and_then(|c| defaults.codex_tier.get(c))
            .cloned()
            .unwrap_or_default(),
    }
}

pub fn codex_qualifies(
    text: &str,
    shape_only: bool,
    defaults: &Defaults,
) -> (bool, Option<String>) {
    let fields = brief_header(text);
    let get = |name: &str| fields.get(name).map(String::as_str).unwrap_or("");
    let shown = |name: &str| {
        let first = get(name).trim().split('\n').next().unwrap_or("");
        if first.is_empty() { "missing" } else { first }
    };
    let fail = |why: String| (false, Some(why));
    if fields.is_empty() {
        return fail("Header missing".into());
    }
    let category = header_word(&fields, "category", defaults);
    let spec = header_word(&fields, "spec", defaults);
    if !shape_only && codex_class(category.as_deref(), defaults).route != "codex" {
        return fail(format!("Category {}", shown("category")));
    }
    if spec.as_deref() != Some("exact")
        && !(spec.as_deref() == Some("measured")
            && matches!(category.as_deref(), Some("tooling" | "native-check")))
    {
        return fail(format!("Spec {}", shown("spec")));
    }
    for (name, title, allowed) in [
        ("scope", "Scope", &["one-file", "module"][..]),
        ("surface", "Surface", &["shell", "docs", "nix"][..]),
        ("verify", "Verify", &["none", "local-test", "native"][..]),
    ] {
        if !header_word(&fields, name, defaults)
            .is_some_and(|word| allowed.contains(&word.as_str()))
        {
            return fail(format!("{title} {}", shown(name)));
        }
    }
    if !COMMAND.is_match(get("done when")) {
        return fail("Done when has no command".into());
    }
    if !get("open questions").trim().is_empty() {
        return fail("Open questions".into());
    }
    if header_minutes(get("eta")).is_none_or(|eta| eta > defaults.codex_eta_max_min) {
        return fail(format!(
            "ETA {}",
            if get("eta").trim().is_empty() {
                "missing"
            } else {
                get("eta").trim()
            }
        ));
    }
    let files = header_files(get("files"));
    if files.is_empty()
        || files.len() as u64 > defaults.codex_files_max
        || !files.iter().all(|name| PATH.is_match(name))
    {
        return fail("Files".into());
    }
    if get("follows").to_lowercase().contains("gpt-") {
        return fail("Follows a Codex run".into());
    }
    (true, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::router::policy;

    #[test]
    fn recorded_python_briefs_match_verdict_and_reason() {
        let policy = policy::parse(
            include_str!("fixtures/policy/orchestration.toml"),
            include_str!("fixtures/policy/model-catalog.toml"),
            include_str!("fixtures/policy/defaults.toml"),
        )
        .unwrap();
        let mut count = 0;
        for line in include_str!("fixtures/codex-qualifies.tsv").lines().skip(1) {
            let row: Vec<&str> = line.split('\t').collect();
            assert_eq!(row.len(), 5, "{line}");
            let brief: String = serde_json::from_str(row[2]).unwrap();
            let reason: Option<String> = serde_json::from_str(row[4]).unwrap();
            assert_eq!(
                codex_qualifies(&brief, row[1] == "true", &policy.defaults),
                (row[3] == "true", reason),
                "{}",
                row[0]
            );
            count += 1;
        }
        assert!(count > 0);
        println!("matched {count} recorded Python brief rows");
    }
}
