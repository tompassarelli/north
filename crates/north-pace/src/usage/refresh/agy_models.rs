//! The Gemini models `agy models` offers, checked against the catalog's gemini keys.
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use toml_edit::{DocumentMut, Item};

static SLUG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bgemini-[a-z0-9.-]*-(?:low|medium|high|xhigh|max)\b").unwrap());

fn listed(output: &str) -> BTreeSet<String> {
    SLUG.find_iter(&output.to_lowercase()).map(|found| found.as_str().to_owned()).collect()
}

fn catalogued(catalog: &DocumentMut) -> BTreeSet<String> {
    let text = |entry: &Item, key: &str| entry.get(key).and_then(Item::as_str).map(str::to_owned);
    catalog
        .get("models")
        .and_then(Item::as_table_like)
        .into_iter()
        .flat_map(|models| models.iter())
        .filter(|(key, _)| key.starts_with("gemini:"))
        .filter_map(|(_, entry)| Some(format!("{}-{}", text(entry, "model")?, text(entry, "effort")?)))
        .collect()
}

fn drift(catalog: &DocumentMut, offered: &BTreeSet<String>) -> Vec<String> {
    let known = catalogued(catalog);
    let missing = offered.difference(&known).map(|slug| format!("agy offers {slug}; harness/model-catalog.toml has no entry for it"));
    let retired = known.difference(offered).map(|slug| format!("harness/model-catalog.toml lists {slug}; agy no longer offers it"));
    missing.chain(retired).collect()
}

pub(super) fn check(catalog: &DocumentMut, state: &Path, output: Option<Vec<u8>>, send: &dyn Fn(&str, &str)) -> Result<(), String> {
    let Some(output) = output else { return Ok(()) };
    let text = String::from_utf8_lossy(&output);
    let offered = listed(&text);
    if offered.is_empty() {
        return Ok(());
    }
    let directory = state.join("agents");
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    fs::write(directory.join("agy-models.txt"), text.as_bytes()).map_err(|error| error.to_string())?;
    let lines = drift(catalog, &offered);
    let report = directory.join("agy-models-drift.txt");
    if lines.is_empty() {
        return fs::remove_file(&report).or_else(|error| if error.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(error) }).map_err(|error| error.to_string());
    }
    let body = lines.join("\n") + "\n";
    for line in &lines {
        eprintln!("model catalog drift: {line}");
    }
    if fs::read_to_string(&report).is_ok_and(|previous| previous == body) {
        return Ok(());
    }
    send("Model catalog drift", &lines.join("; "));
    fs::write(&report, body).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const OFFERED: &str = "gemini-4-argon-low\ngemini-4-argon-medium\ngemini-4-argon-high\ngemini-3.8-flash-low\ngemini-3.8-flash-medium\ngemini-3.8-flash-high\nclaude-opus-5-5-low\nclaude-sonnet-5-5-high\ngpt-oss-120b-medium\n";

    fn lines(output: &str) -> Vec<String> {
        drift(&crate::usage::settings::catalog(), &listed(output))
    }

    #[test]
    fn catalog_matches_the_gemini_models_agy_offered_on_2026_11_08() {
        assert_eq!(lines(OFFERED), Vec::<String>::new());
    }

    #[test]
    fn an_added_and_a_retired_model_are_both_reported() {
        let output = OFFERED.replace("gemini-4-argon-low", "gemini-3.7-flash-low");
        assert_eq!(
            lines(&output),
            ["agy offers gemini-3.7-flash-low; harness/model-catalog.toml has no entry for it", "harness/model-catalog.toml lists gemini-4-argon-low; agy no longer offers it"]
        );
    }
}
