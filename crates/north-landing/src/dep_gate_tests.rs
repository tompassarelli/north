use super::*;

const CARGO_BEFORE: &str = "[[package]]\nname = \"recorded-crate\"\nversion = \"1.0.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n";
const CARGO_AFTER: &str = "[[package]]\nname = \"recorded-crate\"\nversion = \"1.1.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n[[package]]\nname = \"local\"\nversion = \"1.0.0\"\n";
const BUN_AFTER: &str = r#"{
      // Recorded registry alias, old boundary release and non-registry URL.
      "packages": {
        "alias": ["@scope/recorded@1.2.3", "", {}, "sha512-recorded",],
        "boundary": ["boundary@1.0.0", "", {}, "sha512-recorded",],
        "remote": ["remote@https://example.com/a,}.tgz", {},], /* kept out */
      },
    }"#;

#[test]
fn recorded_lock_ages_preserve_cooldown_boundary_urgent_and_first_party_exemptions() {
    let now = 2_000_000_000.0;
    let before = BTreeMap::from([
        ("Cargo.lock".into(), CARGO_BEFORE.into()),
        ("flake.lock".into(), r#"{"nodes":{"input":{"locked":{"rev":"old","lastModified":1}},"root":{"inputs":{"input":"input"}}}}"#.into()),
    ]);
    let mut after = BTreeMap::from([
        ("Cargo.lock".into(), CARGO_AFTER.into()),
        ("bun.lock".into(), BUN_AFTER.into()),
        ("flake.lock".into(), r#"{"nodes":{"input":{"locked":{"rev":"new","lastModified":2000000000}},"root":{"inputs":{"input":"input"}}}}"#.into()),
    ]);
    let candidates = lock_candidates(&before, &after).unwrap();
    let times = BTreeMap::from([
        (
            ("cargo".into(), "recorded-crate".into(), "1.1.0".into()),
            now - 60.0,
        ),
        (
            ("npm".into(), "@scope/recorded".into(), "1.2.3".into()),
            now - 60.0,
        ),
        (
            ("npm".into(), "boundary".into(), "1.0.0".into()),
            now - 604800.0,
        ),
    ]);
    let failures = age_violations(&candidates, &times, now, &Urgent::new());
    assert_eq!(failures.len(), 3, "{failures:?}");
    for identity in ["recorded-crate@1.1.0", "@scope/recorded@1.2.3", "input@new"] {
        assert_eq!(
            failures
                .iter()
                .filter(|line| line.contains(identity))
                .count(),
            1,
            "{failures:?}"
        );
    }
    let urgent = urgent_dependencies(&["Recorded advisory fix\n\nDependency-Urgent: recorded-crate@1.1.0 RUSTSEC-2026-0001\nDependency-Urgent: @scope/recorded@1.2.3 GHSA-recorded\nDependency-Urgent: input@new GHSA-recorded".into()]);
    assert_eq!(
        age_violations(&candidates, &times, now, &urgent),
        Vec::<String>::new()
    );
    assert_eq!(
        age_violations(
            &candidates,
            &BTreeMap::from([(
                ("npm".into(), "boundary".into(), "1.0.0".into()),
                now - 604800.0
            )]),
            now,
            &urgent
        ),
        Vec::<String>::new()
    );
    after.insert(
        "flake.lock".into(),
        r#"{"nodes":{"input":{"locked":{"rev":"new"}}}}"#.into(),
    );
    let failures = age_violations(
        &lock_candidates(&before, &after).unwrap(),
        &times,
        now,
        &urgent,
    );
    assert!(
        failures.len() == 1 && failures[0].contains("missing lastModified"),
        "{failures:?}"
    );
    after.insert("flake.lock".into(),r#"{"nodes":{"firn":{"locked":{"type":"github","owner":"tompassarelli","repo":"firn","rev":"new","lastModified":2000000000}},"local":{"locked":{"type":"path","path":"/home/recorded/code/galileo","lastModified":2000000000}},"input":{"locked":{"type":"github","owner":"someone","repo":"input","rev":"new","lastModified":2000000000}}}}"#.into());
    let candidates: Vec<_> = lock_candidates(&before, &after)
        .unwrap()
        .into_iter()
        .filter(|c| c.ecosystem == "flake")
        .collect();
    let failures = age_violations(&candidates, &times, now, &Urgent::new());
    assert!(
        failures.len() == 1 && failures[0].contains("input@new"),
        "{failures:?}"
    );
    for invalid in ["[]", "null", "1", "\"lock\""] {
        after.insert("flake.lock".into(), invalid.into());
        assert!(lock_candidates(&before, &after).is_err());
    }
}

fn commit(root: &Path, message: &str) -> String {
    git(root, &["add", "."]).unwrap();
    git(
        root,
        &["-c", "commit.gpgsign=false", "commit", "-qm", message],
    )
    .unwrap();
    git(root, &["rev-parse", "HEAD"]).unwrap().trim().to_owned()
}

#[test]
fn recorded_git_addition_requires_a_reason_and_rejects_a_forbidden_source() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    git(root, &["init", "-q"]).unwrap();
    git(root, &["config", "user.name", "fixture"]).unwrap();
    git(root, &["config", "user.email", "fixture@example.invalid"]).unwrap();
    std::fs::write(root.join("package.json"), "{}").unwrap();
    let base = commit(root, "base");
    std::fs::write(
        root.join("package.json"),
        r#"{"dependencies":{"left-pad":"1.3.0"}}"#,
    )
    .unwrap();
    let head = commit(root, "Record dependency addition without a reason");
    let args = [base.clone(), head];
    let result = evaluate(root, &args).unwrap();
    assert_eq!(result.0, 1);
    assert_eq!(
        result.1,
        format!(
            "dep-gate: package.json: left-pad: added dependency needs a reason\n{TRAILER_FORMAT}\n"
        )
    );
    git(root,&["-c","commit.gpgsign=false","commit","--amend","-qm","Record dependency addition with a reason\n\nDependency: left-pad — exercise the dependency gate"]).unwrap();
    let result = evaluate(root, &[base.clone(), "HEAD".into()]).unwrap();
    assert_eq!(result, (0, String::new()));
    std::fs::write(
        root.join("package.json"),
        r#"{"dependencies":{"left-pad":"1.3.0","x":"github:someone/x"}}"#,
    )
    .unwrap();
    commit(
        root,
        "Record forbidden source with a reason\n\nDependency: x - exercise the source restriction",
    );
    let result = evaluate(root, &[base, "HEAD".into()]).unwrap();
    assert_eq!(result.0, 1);
    assert_eq!(
        result.1,
        format!(
            "dep-gate: package.json: x: forbidden new dependency source; use github.com/tompassarelli/* or file:vendor/…\n{TRAILER_FORMAT}\n"
        )
    );
}

#[test]
fn manifests_preserve_ecosystem_sources_and_trailer_restrictions() {
    for (manifest, text, expected) in [
        (
            "package.json",
            r#"{"dependencies":{"a":"workspace:*","b":"file:vendor/b","c":"github:tompassarelli/c","d":"npm:@scope/d@1.0.0","e":"file:../external"}}"#,
            vec![
                ("a", true),
                ("b", true),
                ("c", true),
                ("d", true),
                ("e", false),
            ],
        ),
        (
            "sub/Cargo.toml",
            "[dependencies]\na = { path = \"../local\" }\nb = { git = \"https://github.com/tompassarelli/b\", rev = \"one\" }\n[target.'cfg(unix)'.build-dependencies]\nc = { package = \"renamed\", version = \"1\" }",
            vec![("a", true), ("b", true), ("renamed", true)],
        ),
        (
            "pyproject.toml",
            "[project]\ndependencies = [\"Foo_Bar[extra]>=1; python_version > '3'\", \"remote @ https://example.com/file.whl\"]\n[tool.uv.sources]\nowned = { git = \"https://github.com/tompassarelli/owned\" }\n[dependency-groups]\ndev = [{ include-group = \"test\" }, \"test==1\"]",
            vec![
                ("foo-bar", true),
                ("owned", true),
                ("remote", false),
                ("test", true),
            ],
        ),
    ] {
        assert_eq!(
            dependencies(manifest, text)
                .unwrap()
                .into_iter()
                .map(|d| (d.name, d.allowed))
                .collect::<Vec<_>>(),
            expected
                .into_iter()
                .map(|(n, a)| (n.to_owned(), a))
                .collect::<Vec<_>>()
        );
    }
    assert!(!internal_path("../outside", "Cargo.toml"));
    assert!(!internal_path("%2e%2e/outside", "Cargo.toml"));
    for text in [
        "subject\nDependency: x - reason",
        "subject\n\nDependency: x - reason\nbody",
        "subject\n\nDependency: x - reason\n\nother",
    ] {
        assert!(dependency_reasons(&[text.into()]).is_empty());
    }
    for separator in [
        "\r", "\r\n", "\u{b}", "\u{c}", "\u{1c}", "\u{1d}", "\u{1e}", "\u{85}", "\u{2028}",
        "\u{2029}",
    ] {
        assert!(
            dependency_reasons(&[format!("subject\n\nDependency: x - reason{separator}body")])
                .is_empty()
        );
        assert!(
            urgent_dependencies(&[format!(
                "subject\n\nDependency-Urgent: x@1 GHSA-recorded{separator}body"
            )])
            .is_empty()
        );
        assert!(
            dependency_reasons(&[format!(
                "subject\n\nDependency: x - reason{separator}Reviewed: someone"
            )])
            .contains("x")
        );
    }
    assert!(
        dependency_reasons(&["subject\n\nDependency: x - reason\nReviewed: someone".into()])
            .contains("x")
    );
    assert_eq!(
        dependencies("Cargo.toml", "[dependencies]\na = { version = 1 }").unwrap_err(),
        "ValueError"
    );
    assert_eq!(
        dependencies("package.json", "{").unwrap_err(),
        "JSONDecodeError"
    );
    assert_eq!(
        dependencies("Cargo.toml", "[").unwrap_err(),
        "TOMLDecodeError"
    );
}

#[test]
fn publish_times_keep_fractional_seconds_and_timezone_offsets() {
    let utc = Timestamp::parse_publish_time("2026-10-01T12:34:56.123456+00:00").unwrap();
    assert_eq!(
        utc,
        Timestamp::parse_publish_time("2026-10-01T14:34:56.123456+02:00").unwrap()
    );
    assert_eq!(
        utc,
        Timestamp::parse_publish_time("20261001T123456.123456+0000").unwrap()
    );
    assert!(Timestamp::parse_publish_time("2026-10-01T12:34:56").is_none());
    assert!(Timestamp::parse_publish_time("2026-13-01T12:34:56+00:00").is_none());
}

#[test]
fn root_channel_nixpkgs_skips_the_cooldown_and_other_nixpkgs_do_not() {
    let lock = |root_ref: &str, owner: &str, rev: &str| {
        format!(
            r#"{{"root":"root","nodes":{{"root":{{"inputs":{{"nixpkgs":"nixpkgs_2","tool":"tool"}}}},"nixpkgs_2":{{"locked":{{"type":"github","owner":"{owner}","repo":"nixpkgs","rev":"{rev}","lastModified":2000000000}},"original":{{"type":"github","owner":"{owner}","repo":"nixpkgs","ref":"{root_ref}"}}}},"tool":{{"inputs":{{"nixpkgs":"nixpkgs"}},"locked":{{"type":"github","owner":"other","repo":"tool","rev":"t1","lastModified":1}}}},"nixpkgs":{{"locked":{{"type":"github","owner":"NixOS","repo":"nixpkgs","rev":"{rev}","lastModified":2000000000}},"original":{{"type":"github","owner":"NixOS","repo":"nixpkgs","ref":"nixos-unstable"}}}}}}}}"#
        )
    };
    let before = BTreeMap::from([("flake.lock".to_string(), lock("nixos-26.05", "nixos", "old"))]);
    let names = |after: String| {
        lock_candidates(&before, &BTreeMap::from([("flake.lock".to_string(), after)]))
            .unwrap()
            .into_iter()
            .map(|c| c.name)
            .collect::<Vec<_>>()
    };
    assert_eq!(names(lock("nixos-26.05", "nixos", "new")), ["nixpkgs"]);
    assert_eq!(names(lock("master", "nixos", "new")), ["nixpkgs", "nixpkgs_2"]);
    assert_eq!(names(lock("nixos-26.05", "fork", "new")), ["nixpkgs", "nixpkgs_2"]);
}

#[test]
fn root_nixpkgs_that_follows_a_first_party_input_skips_the_cooldown() {
    let lock = |owner: &str, rev: &str| {
        format!(
            r#"{{"root":"root","nodes":{{"root":{{"inputs":{{"firn":"firn","nixpkgs":["firn","nixpkgs"]}}}},"firn":{{"inputs":{{"nixpkgs":"nixpkgs_2"}},"locked":{{"type":"github","owner":"tompassarelli","repo":"firn","rev":"{rev}","lastModified":2000000000}}}},"nixpkgs_2":{{"locked":{{"type":"github","owner":"{owner}","repo":"nixpkgs","rev":"{rev}","lastModified":2000000000}},"original":{{"type":"github","owner":"{owner}","repo":"nixpkgs","ref":"nixos-26.05"}}}}}}}}"#
        )
    };
    let before = BTreeMap::from([("flake.lock".to_string(), lock("nixos", "old"))]);
    let names = |after: String| {
        lock_candidates(&before, &BTreeMap::from([("flake.lock".to_string(), after)]))
            .unwrap()
            .into_iter()
            .map(|c| c.name)
            .collect::<Vec<_>>()
    };
    assert_eq!(names(lock("nixos", "new")), Vec::<String>::new());
    assert_eq!(names(lock("fork", "new")), ["nixpkgs_2"]);
}
