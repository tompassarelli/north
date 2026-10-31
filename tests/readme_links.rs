use regex::Regex;
use std::fs;
use std::path::Path;

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

fn problems(base: &Path, text: &str, rot: bool) -> Vec<String> {
    let link = Regex::new(r"\[[^\]]*\]\(([^)\s]+)\)").unwrap();
    let code = Regex::new(r"`([^`\s]+)`").unwrap();
    let scheme_or_anchor = Regex::new(r"^([a-z][a-z0-9+.-]*:|#)").unwrap();
    let mut targets: Vec<String> = link
        .captures_iter(text)
        .map(|c| c[1].to_string())
        .filter(|t| !scheme_or_anchor.is_match(t))
        .collect();
    if rot {
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
    if rot {
        let patterns = [
            ("version", r"\bv?\d+(?:\.\d+)+\b"),
            ("date", r"\b\d{4}-\d{2}(?:-\d{2})?\b"),
            ("percentage", r"\d+(?:\.\d+)?\s*%"),
            ("count", r"\b\d+\b"),
            ("model id", r"(?i)\b(?:claude|gpt)-[\w.-]*\d[\w.-]*"),
        ];
        for (kind, pattern) in patterns {
            for m in Regex::new(pattern).unwrap().find_iter(text) {
                out.push(format!("{kind}: {:?}", m.as_str()));
            }
        }
    }
    out
}

#[test]
fn flags_every_kind_of_rot_and_missing_path() {
    let bad = "[x](docs/gone.md) `bin/gone` v1.2 2026-10-12 80% 3 claude-opus-5-5";
    let found = problems(Path::new(ROOT), bad, true);
    for want in ["docs/gone.md", "bin/gone", "version", "date", "percentage", "count", "model id"] {
        assert!(found.iter().any(|p| p.contains(want)), "{want}: {found:?}");
    }
}

#[test]
fn plain_model_family_names_are_not_rot() {
    let found = problems(Path::new(ROOT), "Opus, Sonnet, Sol and Haiku; claude-code", true);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn links_resolve_relative_to_their_document_without_rot_checks() {
    let docs = Path::new(ROOT).join("docs");
    assert_eq!(
        problems(&docs, "[x](../bin/gone) 2026-10-12", false),
        ["missing path: ../bin/gone"]
    );
}

#[test]
fn readme_and_references_resolve_and_readme_has_no_rot() {
    let root = Path::new(ROOT);
    let readme = fs::read_to_string(root.join("README.md")).unwrap();
    let mut found: Vec<String> = problems(root, &readme, true)
        .into_iter()
        .map(|p| format!("README.md: {p}"))
        .collect();
    let references = fs::read_to_string(root.join("docs/references.md")).unwrap();
    found.extend(
        problems(&root.join("docs"), &references, false)
            .into_iter()
            .map(|p| format!("docs/references.md: {p}")),
    );
    assert!(found.is_empty(), "{}", found.join("\n"));
}
