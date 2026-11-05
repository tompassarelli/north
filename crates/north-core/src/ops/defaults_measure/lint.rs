use std::fs;
use std::path::Path;

use toml_edit::{DocumentMut, Item};

const KINDS: [&str; 4] = ["measured: ", "tom: ", "standard: ", "prior: "];
const NAME_PREFIXES: [&str; 2] = ["codex_route_", "codex_tier_"];
const PRIORS: &str = "crates/north-core/src/ops/defaults_measure/priors.rs";

pub(super) fn lint(root: &Path) -> Vec<String> {
    let text = fs::read_to_string(root.join("harness/defaults.toml")).expect("harness/defaults.toml reads");
    let registry: DocumentMut = text.parse().expect("harness/defaults.toml parses");
    let counters = fs::read_to_string(root.join(PRIORS)).unwrap_or_default();
    let mut failures = Vec::new();
    for (name, entry) in registry.iter() {
        let text = |key: &str| entry.get(key).and_then(Item::as_str).unwrap_or_default();
        let source = text("source");
        if entry.get("value").is_none() {
            failures.push(format!("{name}: no value"));
        }
        if !KINDS.iter().any(|kind| source.starts_with(kind)) || source.split_once(": ").is_none_or(|(_, reason)| reason.trim().is_empty()) {
            failures.push(format!("{name}: source {source:?} is not one of measured, tom, standard, prior with a reason"));
        }
        if source.starts_with("prior: ") {
            failures.extend(settles_failure(name, entry.get("settles_when"), &counters));
        }
        if text("fallback").trim().is_empty() {
            failures.push(format!("{name}: empty fallback"));
        }
        let consumers: Vec<&str> = entry.get("consumers").and_then(Item::as_array).map(|list| list.iter().filter_map(|consumer| consumer.as_str()).collect()).unwrap_or_default();
        if consumers.is_empty() {
            failures.push(format!("{name}: no consumer"));
        }
        for consumer in consumers {
            match fs::read_to_string(root.join(consumer)) {
                Err(_) => failures.push(format!("{name}: consumer {consumer} missing")),
                Ok(body) if !reads_default(root, name, consumer, &body) => failures.push(format!("{name}: {consumer} does not read DEFAULTS[\"{name}\"]")),
                Ok(_) => {}
            }
        }
    }
    failures
}

fn settles_failure(name: &str, settles: Option<&Item>, counters: &str) -> Option<String> {
    let need = settles.and_then(|settles| settles.get("need")).and_then(Item::as_integer).filter(|need| *need >= 1);
    let Some(settles) = settles.filter(|_| need.is_some()) else {
        return Some(format!("{name}: a prior needs settles_when with a positive integer need"));
    };
    let count = settles.get("count").and_then(Item::as_str).unwrap_or_default();
    let what = settles.get("what").and_then(Item::as_str).unwrap_or_default();
    (!counters.contains(&format!("\"{count}\" =>")) || what.trim().is_empty())
        .then(|| format!("{name}: settles_when count {count:?} is not a north defaults-measure counter with a what"))
}

fn reads_default(root: &Path, name: &str, consumer: &str, body: &str) -> bool {
    if body.contains(&format!("DEFAULTS[\"{name}\"]")) {
        return true;
    }
    let path = root.join(consumer);
    if path.extension().is_none_or(|extension| extension != "rs") {
        return false;
    }
    let keys = std::iter::once(name).chain(NAME_PREFIXES.into_iter().filter(|prefix| name.starts_with(prefix)));
    if !keys.into_iter().any(|key| body.contains(&format!("\"{key}\""))) {
        return false;
    }
    let loads = |text: &str| text.contains("/harness/defaults.toml\")");
    loads(body)
        || path.parent().and_then(|dir| fs::read_dir(dir).ok()).is_some_and(|entries| {
            entries.filter_map(Result::ok).map(|entry| entry.path()).filter(|sibling| sibling.extension().is_some_and(|extension| extension == "rs")).any(|sibling| fs::read_to_string(sibling).is_ok_and(|text| loads(&text)))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repository() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    #[test]
    fn every_default_carries_a_value_a_sourced_reason_a_fallback_and_a_reading_consumer() {
        let failures = lint(&repository());
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn a_blank_source_a_bare_prior_a_missing_fallback_and_a_dead_consumer_each_fail() {
        let root = tempfile::tempdir().unwrap();
        let write = |path: &str, body: &str| {
            let path = root.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, body).unwrap();
        };
        write(PRIORS, "\"count_worker_runs\" => 1,\n");
        write("reader.rs", "include_str!(\"../harness/defaults.toml\")\nDEFAULTS[\"reads\"]");
        write(
            "harness/defaults.toml",
            "[no_value]\nsource = \"standard: x\"\nfallback = \"f\"\nconsumers = [\"reader.rs\"]\n\
             [blank_source]\nvalue = 1\nsource = \"measured: \"\nfallback = \"f\"\nconsumers = [\"reader.rs\"]\n\
             [bare_prior]\nvalue = 1\nsource = \"prior: x\"\nfallback = \"f\"\nconsumers = [\"reader.rs\"]\n\
             [unknown_counter]\nvalue = 1\nsource = \"prior: x\"\nsettles_when = { need = 2, count = \"nope\", what = \"w\" }\nfallback = \"f\"\nconsumers = [\"reader.rs\"]\n\
             [no_fallback]\nvalue = 1\nsource = \"tom: x\"\nconsumers = [\"reader.rs\"]\n\
             [dead_consumer]\nvalue = 1\nsource = \"tom: x\"\nfallback = \"f\"\nconsumers = [\"gone.rs\", \"reader.rs\"]\n\
             [no_consumer]\nvalue = 1\nsource = \"tom: x\"\nfallback = \"f\"\nconsumers = []\n\
             [reads]\nvalue = 1\nsource = \"standard: x\"\nfallback = \"f\"\nconsumers = [\"reader.rs\"]\n",
        );
        let failures = lint(root.path());
        let expected = [
            "no_value: no value",
            "blank_source: source \"measured: \" is not one of measured, tom, standard, prior with a reason",
            "bare_prior: a prior needs settles_when with a positive integer need",
            "unknown_counter: settles_when count \"nope\" is not a north defaults-measure counter with a what",
            "no_fallback: empty fallback",
            "dead_consumer: consumer gone.rs missing",
            "no_consumer: no consumer",
        ];
        for line in expected {
            assert!(failures.iter().any(|failure| failure == line), "{line} missing from {failures:?}");
        }
        assert!(failures.iter().all(|failure| !failure.starts_with("reads:")), "{failures:?}");
        assert_eq!(failures.iter().filter(|failure| failure.contains("does not read")).count(), 6, "{failures:?}");
    }
}
