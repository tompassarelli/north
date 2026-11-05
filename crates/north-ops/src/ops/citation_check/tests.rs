use super::*;
use std::fs;

fn commands(path: &str, text: &str) -> Vec<String> {
    scan(path, text).into_iter().map(|citation| citation.command).collect()
}

#[test]
fn only_a_command_in_command_position_is_cited_with_its_line() {
    let markdown = "Run `north fleet send` then `safe-push && north land-clean X`.\nThe north gate is not a command, and north of here is prose.\n```sh\n$ north pace plan\nnorth --help\nnorth <command>\nnorth Fleet\n```\nnorth pace outside\n";
    assert_eq!(commands("docs/a.md", markdown), ["fleet", "land-clean", "pace"]);
    let script = "# north gone in a comment\nnorth fleet list\nout=$(north capacity status) || exit 1\necho north not-a-command\n";
    assert_eq!(commands("harness/hooks/x.sh", script), ["fleet", "capacity"]);
    let found = scan("docs/a.md", "one\n\n`north missing`\n");
    assert_eq!(found, [Citation { path: "docs/a.md".into(), line: 3, command: "missing".into(), flags: vec![] }]);
}

fn repository(files: &[(&str, &str)]) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    for (path, text) in files {
        let path = root.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    for args in [vec!["init", "-q"], vec!["add", "-A"]] {
        assert!(Command::new("git").arg("-C").arg(root.path()).args(args).status().unwrap().success());
    }
    root
}

fn defined(command: &str) -> bool {
    matches!(command, "fleet" | "health")
}

#[test]
fn a_citation_of_a_missing_subcommand_is_named_and_defined_ones_pass() {
    let root = repository(&[("docs/guide.md", "Use `north fleet list`.\nThen `north capture`.\n")]);
    let found = undefined(root.path(), defined).unwrap();
    assert_eq!(found, [Citation { path: "docs/guide.md".into(), line: 2, command: "capture".into(), flags: vec![] }]);
    let root = repository(&[("docs/guide.md", "Use `north fleet list` and `north health`.\n")]);
    assert!(undefined(root.path(), defined).unwrap().is_empty());
}

#[test]
fn rationale_history_fixtures_and_files_outside_the_roots_are_not_scanned() {
    let root = repository(&[
        ("docs/rationale/old.md", "`north gone`\n"),
        ("harness/skills/x/fixtures/a.md", "`north gone`\n"),
        ("src/a.md", "`north gone`\n"),
        ("harness/hooks/a.test.sh", "north gone\n"),
    ]);
    assert!(undefined(root.path(), defined).unwrap().is_empty());
}

#[test]
fn the_command_exits_one_on_an_undefined_citation_and_zero_when_clean() {
    let root = repository(&[("README.md", "[g](docs/guide.md)\n"), ("docs/guide.md", "`north gone`\n")]);
    let arguments = ["--root".to_owned(), root.path().to_string_lossy().into_owned()];
    assert_eq!(run(&arguments, defined), 1);
    fs::write(root.path().join("docs/guide.md"), "`north fleet`\n").unwrap();
    assert_eq!(run(&arguments, defined), 0);
}

fn targets(found: Vec<BrokenPath>) -> Vec<(String, usize, String)> {
    found.into_iter().map(|broken| (broken.path, broken.line, broken.target)).collect()
}

#[test]
fn an_agent_machinery_readme_naming_a_missing_package_path_exits_one_and_skills_stay_links_only() {
    let root = repository(&[
        ("agent-machinery/scripts/kept.mjs", ""),
        ("src/kept.rs", ""),
        ("bin/kept", ""),
        ("agent-machinery/README.md", "Run `agent-machinery/scripts/gone.mjs`, see [k](scripts/kept.mjs).\n"),
        ("agent-machinery/skills/a/SKILL.md", "`bin/elsewhere` [a](../gone/SKILL.md)\n"),
        ("agent-machinery/PROVENANCE.md", "Upstream `src/elsewhere.ts`.\n"),
    ]);
    assert_eq!(
        targets(broken_paths(root.path()).unwrap()),
        [
            ("agent-machinery/README.md".into(), 1, "agent-machinery/scripts/gone.mjs".into()),
            ("agent-machinery/skills/a/SKILL.md".into(), 1, "../gone/SKILL.md".into()),
        ]
    );
    let arguments = ["--root".to_owned(), root.path().to_string_lossy().into_owned()];
    assert_eq!(run(&arguments, defined), 1);
    fs::write(root.path().join("agent-machinery/README.md"), "Run `agent-machinery/scripts/kept.mjs`.\n").unwrap();
    fs::write(root.path().join("agent-machinery/skills/a/SKILL.md"), "`bin/elsewhere`\n").unwrap();
    assert_eq!(run(&arguments, defined), 0);
}

#[test]
fn a_named_path_or_link_that_no_tracked_file_backs_is_named_with_its_line() {
    let root = repository(&[
        ("bin/kept", ""),
        ("docs/guide.md", "Run `bin/kept` and `bin/gone`, see [x](../bin/kept) and [y](terms.md#a).\n```\n`bin/fenced-gone`\n```\n`bin/NAME`, `crates/<crate>/`, `bin/*`, `bin/kept:12`, `nope/x` and [web](https://x.invalid/y)\n"),
        ("docs/rationale/old.md", "`bin/gone` [z](gone.md)\n"),
        ("docs/features/01-a/0001-a.md", "[h](harness.md)\n"),
        ("docs/references/paper.md", "`bin/gone` [r](../gone.md)\n"),
        ("harness/skills/a/SKILL.md", "`bin/gone` [b](../b/SKILL.md) [c](../c/SKILL.md)\n"),
        ("agent-machinery/skills/b/SKILL.md", "[a](../a/SKILL.md)\n"),
    ]);
    assert_eq!(
        targets(broken_paths(root.path()).unwrap()),
        [
            ("docs/guide.md".into(), 1, "terms.md#a".into()),
            ("docs/guide.md".into(), 1, "bin/gone".into()),
            ("docs/references/paper.md".into(), 1, "../gone.md".into()),
            ("harness/skills/a/SKILL.md".into(), 1, "../c/SKILL.md".into()),
        ]
    );
}

#[test]
fn the_command_exits_one_on_a_missing_path_and_zero_once_it_exists() {
    let root = repository(&[("README.md", "[x](docs/x.md)\n"), ("bin/kept", ""), ("docs/x.md", "Run `bin/gone`.\n")]);
    let arguments = ["--root".to_owned(), root.path().to_string_lossy().into_owned()];
    assert_eq!(run(&arguments, defined), 1);
    fs::create_dir_all(root.path().join("bin")).unwrap();
    fs::write(root.path().join("bin/gone"), "").unwrap();
    assert!(Command::new("git").arg("-C").arg(root.path()).args(["add", "-A"]).status().unwrap().success());
    assert_eq!(run(&arguments, defined), 0);
}

#[test]
fn a_cited_flag_that_no_source_string_literal_holds_is_named_and_the_command_exits_one() {
    let root = repository(&[
        ("README.md", "[g](docs/guide.md)\n"),
        ("crates/a/src/lib.rs", "const FLAGS: &[&str] = &[\"--root\", \"--since=\"];\n"),
        ("docs/guide.md", "Run `north health --root .` or `north health --since=2d`.\nThen `north health --no-such-flag`, `north capture --gone` and `safe-push --to main`.\n"),
    ]);
    let found = missing_flags(root.path(), defined).unwrap();
    assert_eq!(found, [MissingFlag { path: "docs/guide.md".into(), line: 2, command: "health".into(), flag: "--no-such-flag".into() }]);
    let arguments = ["--root".to_owned(), root.path().to_string_lossy().into_owned()];
    assert_eq!(run(&arguments, defined), 1);
    fs::write(root.path().join("docs/guide.md"), "Run `north health --root .`.\n").unwrap();
    assert_eq!(run(&arguments, defined), 0);
}

#[test]
fn a_named_config_table_or_key_that_no_owner_holds_is_named_and_the_command_exits_one() {
    let root = repository(&[
        ("README.md", "[g](docs/guide.md)\n"),
        ("harness/defaults.toml", "[min_runs]\nvalue = 5\n"),
        (".safe-push", "[land]\n\tcheck = ** :: true\n"),
        ("bin/safe-push", "git config --get land.local-only\n"),
        ("crates/a/src/lib.rs", "fn f() { cfg.get(\"placement\").and_then(|t| t.get(\"cloud_host\")); }\n"),
        ("flake.nix", ""),
        (
            "docs/guide.md",
            "`[min_runs]`, `[min_runs] value`, `land.check`, `land.local-only`, `[placement] cloud_host`, `flake.nix`, `[projects.NAME]`.\n`[no_such_default]`, `[min_runs] gone`, `land.gone`, `[placement] gone`.\n```\n`[fenced_gone]`\n```\n",
        ),
        ("docs/rationale/old.md", "`[no_such_default]`\n"),
    ]);
    let names: Vec<_> = missing_keys(root.path()).unwrap().into_iter().map(|key| (key.path, key.line, key.name)).collect();
    assert_eq!(
        names,
        [
            ("docs/guide.md".into(), 2, "[no_such_default]".into()),
            ("docs/guide.md".into(), 2, "[min_runs] gone".into()),
            ("docs/guide.md".into(), 2, "land.gone".into()),
            ("docs/guide.md".into(), 2, "[placement] gone".into()),
        ]
    );
    let arguments = ["--root".to_owned(), root.path().to_string_lossy().into_owned()];
    assert_eq!(run(&arguments, defined), 1);
    fs::write(root.path().join("docs/guide.md"), "`[min_runs]`\n").unwrap();
    assert_eq!(run(&arguments, defined), 0);
}

#[test]
fn a_top_level_doc_that_neither_entry_links_is_named_and_the_command_exits_one() {
    let root = repository(&[
        ("README.md", "[h](docs/harness.md)\n"),
        ("AGENTS.md", "[t](docs/terms.md#words)\n"),
        ("docs/harness.md", "# Harness\n"),
        ("docs/terms.md", "## Words\n"),
        ("docs/orphan.md", "# Orphan\n"),
        ("docs/features/01-a/0001-a.md", "- a\n"),
    ]);
    assert_eq!(unlinked_docs(root.path()).unwrap(), ["docs/orphan.md"]);
    let arguments = ["--root".to_owned(), root.path().to_string_lossy().into_owned()];
    assert_eq!(run(&arguments, defined), 1);
    fs::write(root.path().join("README.md"), "[h](docs/harness.md) [o](docs/orphan.md)\n").unwrap();
    assert_eq!(run(&arguments, defined), 0);
}

#[test]
fn a_link_to_a_heading_its_file_lacks_is_named_and_the_command_exits_one() {
    let root = repository(&[
        ("README.md", "[t](docs/terms.md)\n"),
        ("docs/terms.md", "# Terms\n## The `north` app, in short!\n## Repeat\n## Repeat\n```\n# not a heading\n```\n"),
        (
            "docs/harness.md",
            "[a](terms.md#the-north-app-in-short) [b](terms.md#repeat-1) [c](terms.md#no-such-heading) [d](terms.md#not-a-heading) [e](terms.md)\n",
        ),
    ]);
    let found = targets(broken_anchors(root.path()).unwrap());
    assert_eq!(
        found,
        [("docs/harness.md".into(), 1, "terms.md#no-such-heading".into()), ("docs/harness.md".into(), 1, "terms.md#not-a-heading".into())]
    );
    fs::write(root.path().join("README.md"), "[t](docs/terms.md) [h](docs/harness.md)\n").unwrap();
    let arguments = ["--root".to_owned(), root.path().to_string_lossy().into_owned()];
    assert_eq!(run(&arguments, defined), 1);
    fs::write(root.path().join("docs/harness.md"), "[a](terms.md#the-north-app-in-short)\n").unwrap();
    assert_eq!(run(&arguments, defined), 0);
}

#[test]
fn a_heading_slug_follows_github() {
    assert_eq!(reach::slug(" Landing a change "), "landing-a-change");
    assert_eq!(reach::slug("`north fleet` (TUI): v2.0"), "north-fleet-tui-v20");
    assert_eq!(reach::slug("snake_case and -dash-"), "snake_case-and--dash-");
}
