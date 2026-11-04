use regex::Regex;
use std::fs;
use std::path::Path;

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

const CONCEPT_DOCS: &[&str] = &["docs/harness.md", "docs/architecture.md", "docs/terms.md"];

#[derive(Clone, Copy, PartialEq)]
enum Rules {
    Links,
    Concept,
    Readme,
}

fn problems(base: &Path, text: &str, rules: Rules) -> Vec<String> {
    let link = Regex::new(r"\[[^\]]*\]\(([^)\s]+)\)").unwrap();
    let code = Regex::new(r"`([^`\s]+)`").unwrap();
    let scheme_or_anchor = Regex::new(r"^([a-z][a-z0-9+.-]*:|#)").unwrap();
    let mut targets: Vec<String> = link
        .captures_iter(text)
        .map(|c| c[1].to_string())
        .filter(|t| !scheme_or_anchor.is_match(t))
        .collect();
    if rules == Rules::Readme {
        targets.extend(
            code.captures_iter(text)
                .map(|c| c[1].to_string())
                .filter(|t| t.contains('/') || t.contains('.')),
        );
    }
    let mut out = Vec::new();
    for target in targets {
        let path = target.split('#').next().unwrap_or("");
        if !path.is_empty() && !base.join(path).exists() {
            out.push(format!("missing path: {target}"));
        }
    }
    let mut patterns = vec![
        ("version", r"\bv?\d+(?:\.\d+)+\b"),
        ("date", r"\b\d{4}-\d{2}(?:-\d{2})?\b"),
        ("percentage", r"\d+(?:\.\d+)?\s*%"),
        ("model id", r"(?i)\b(?:claude|gpt)-[\w.-]*\d[\w.-]*"),
    ];
    match rules {
        Rules::Links => patterns.clear(),
        Rules::Concept => {}
        Rules::Readme => patterns.push(("count", r"\b\d+\b")),
    }
    for (kind, pattern) in patterns {
        for m in Regex::new(pattern).unwrap().find_iter(text) {
            out.push(format!("{kind}: {:?}", m.as_str()));
        }
    }
    out
}

#[test]
fn flags_every_kind_of_rot_and_missing_path() {
    let bad = "[x](docs/gone.md) `bin/gone` v1.2 2026-10-12 80% 3 claude-opus-5-5";
    let found = problems(Path::new(ROOT), bad, Rules::Readme);
    for want in ["docs/gone.md", "bin/gone", "version", "date", "percentage", "count", "model id"] {
        assert!(found.iter().any(|p| p.contains(want)), "{want}: {found:?}");
    }
}

#[test]
fn concept_rules_flag_dates_versions_percentages_and_model_ids_but_not_counts() {
    let text = "v1.2 2026-10-12 80% claude-opus-5-5 four packages, 3 steps";
    let found = problems(Path::new(ROOT), text, Rules::Concept);
    for want in ["version", "date", "percentage", "model id"] {
        assert!(found.iter().any(|p| p.contains(want)), "{want}: {found:?}");
    }
    assert!(!found.iter().any(|p| p.starts_with("count")), "{found:?}");
}

#[test]
fn plain_model_family_names_are_not_rot() {
    let found = problems(Path::new(ROOT), "Opus, Sonnet, Sol and Haiku; claude-code", Rules::Readme);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn links_resolve_relative_to_their_document_without_rot_checks() {
    let docs = Path::new(ROOT).join("docs");
    assert_eq!(
        problems(&docs, "[x](../bin/gone) 2026-10-12", Rules::Links),
        ["missing path: ../bin/gone"]
    );
}

#[test]
fn readme_and_references_resolve_and_readme_has_no_rot() {
    let root = Path::new(ROOT);
    let readme = fs::read_to_string(root.join("README.md")).unwrap();
    let mut found: Vec<String> = problems(root, &readme, Rules::Readme)
        .into_iter()
        .map(|p| format!("README.md: {p}"))
        .collect();
    let references = fs::read_to_string(root.join("docs/references.md")).unwrap();
    found.extend(
        problems(&root.join("docs"), &references, Rules::Links)
            .into_iter()
            .map(|p| format!("docs/references.md: {p}")),
    );
    assert!(found.is_empty(), "{}", found.join("\n"));
}

#[test]
fn concept_docs_resolve_and_carry_no_dates_versions_percentages_or_model_ids() {
    let root = Path::new(ROOT);
    let found: Vec<String> = CONCEPT_DOCS
        .iter()
        .flat_map(|doc| {
            let text = fs::read_to_string(root.join(doc)).unwrap();
            problems(&root.join("docs"), &text, Rules::Concept)
                .into_iter()
                .map(move |p| format!("{doc}: {p}"))
        })
        .collect();
    assert!(found.is_empty(), "{}", found.join("\n"));
}
