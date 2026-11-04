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

#[test]
fn agent_machinery_docs_and_skills_resolve_and_its_readme_has_no_rot() {
    let package = Path::new(ROOT).join("agent-machinery");
    let mut found = Vec::new();
    for entry in fs::read_dir(&package).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if name.ends_with(".md") {
            let text = fs::read_to_string(&path).unwrap();
            found.extend(problems(&package, &text, if name == "README.md" { Rules::Readme } else { Rules::Links }).into_iter().map(|p| format!("agent-machinery/{name}: {p}")));
        }
    }
    let twin_root = Path::new(ROOT).join("harness/skills");
    for entry in fs::read_dir(package.join("skills")).unwrap() {
        let skill = entry.unwrap().path();
        let name = skill.file_name().unwrap().to_string_lossy().into_owned();
        let text = fs::read_to_string(skill.join("SKILL.md")).unwrap();
        let twin = twin_root.join(&name);
        found.extend(
            problems(&skill, &text, Rules::Links)
                .into_iter()
                .filter(|p| !problems(&twin, &text, Rules::Links).contains(p))
                .map(|p| format!("agent-machinery/skills/{name}/SKILL.md: {p}")),
        );
    }
    assert!(found.is_empty(), "{}", found.join("\n"));
}

fn readme_items(readme: &str, title: &str) -> Vec<String> {
    let body = readme
        .split("\n## ")
        .find_map(|section| section.strip_prefix(title).and_then(|rest| rest.strip_prefix('\n')))
        .unwrap_or_else(|| panic!("agent-machinery/README.md has no ## {title} section"));
    body.lines().filter_map(|line| line.strip_prefix("- ")).map(str::to_owned).collect()
}

fn captured(items: &[String], pattern: &str) -> Vec<String> {
    let pattern = Regex::new(pattern).unwrap();
    items.iter().map(|item| pattern.captures(item).map_or_else(String::new, |c| c[1].to_owned())).collect()
}

#[test]
fn agent_machinery_readme_lists_equal_the_catalog_and_the_package_manifest() {
    let package = Path::new(ROOT).join("agent-machinery");
    let readme = fs::read_to_string(package.join("README.md")).unwrap();
    let json = |name: &str| -> serde_json::Value { serde_json::from_str(&fs::read_to_string(package.join(name)).unwrap()).unwrap() };
    let (catalog, manifest) = (json("catalog.json"), json("package.json"));
    let field = |list: &str, key: &str| -> Vec<String> { catalog[list].as_array().unwrap().iter().map(|v| v[key].as_str().unwrap().to_owned()).collect() };
    let names = |title: &str| captured(&readme_items(&readme, title), r"^\[?`([^`]+)`");
    let targets = |title: &str| captured(&readme_items(&readme, title), r"^\[[^\]]*\]\(([^)]+)\)");
    let mut units: Vec<&serde_json::Value> = catalog["units"].as_array().unwrap().iter().collect();
    units.sort_by_key(|unit| unit["id"].as_str().unwrap().to_owned());
    let of_kind = |kind: &str, key: &str| -> Vec<String> {
        units.iter().filter(|unit| unit["kind"] == kind).map(|unit| unit[key].as_str().unwrap().to_owned()).collect()
    };
    assert_eq!(names("Skills"), of_kind("skill", "id"));
    assert_eq!(targets("Skills"), of_kind("skill", "source"));
    assert_eq!(names("Modules"), of_kind("module", "id"));
    assert_eq!(names("Assets"), field("assets", "id"));
    assert_eq!(targets("Assets"), field("assets", "path"));
    assert_eq!(names("Contracts"), field("contracts", "id"));
    assert_eq!(targets("Contracts"), field("contracts", "schema"));
    let mut subpaths: Vec<String> = manifest["exports"].as_object().unwrap().values().map(|v| v.as_str().unwrap().trim_start_matches("./").trim_end_matches('*').to_owned()).collect();
    let mut listed = targets("Package subpaths");
    listed.sort();
    subpaths.sort();
    assert_eq!(listed, subpaths);
    let mut commands: Vec<String> = manifest["scripts"].as_object().unwrap().keys().cloned().collect();
    commands.sort();
    assert_eq!(captured(&readme_items(&readme, "Commands"), r"^`([^`]+)`"), commands);
    let index = fs::read_to_string(package.join("index.mjs")).unwrap();
    let mut exports: Vec<String> = Regex::new(r"export \{([^}]*)\}").unwrap().captures_iter(&index).flat_map(|c| c[1].split(',').map(|name| name.trim().to_owned()).collect::<Vec<_>>()).filter(|name| !name.is_empty()).collect();
    exports.extend(Regex::new(r"export (?:const|function|class) (\w+)").unwrap().captures_iter(&index).map(|c| c[1].to_owned()));
    exports.sort();
    assert_eq!(names("Module exports"), exports);
    let mut scripts: Vec<String> = fs::read_dir(package.join("scripts")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".mjs")).map(|n| format!("scripts/{n}")).collect();
    scripts.sort();
    assert_eq!(targets("Scripts"), scripts);
}
