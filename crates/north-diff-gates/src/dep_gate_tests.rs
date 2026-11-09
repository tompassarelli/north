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

struct Published(i64);

impl Registry for Published {
    fn read(&self, _host: &str, _path: &str) -> Result<String, &'static str> {
        let at = time::OffsetDateTime::now_utc() - time::Duration::days(self.0);
        let at = format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}+00:00", at.year(), u8::from(at.month()), at.day(), at.hour(), at.minute(), at.second());
        Ok(serde_json::json!({"name": "recorded-crate", "vers": "1.1.0", "pubtime": at}).to_string())
    }
}

#[test]
fn cargo_update_restores_the_lock_when_it_resolves_a_version_under_seven_days() {
    let dir = tempfile::tempdir().unwrap();
    let lock = dir.path().join("Cargo.lock");
    std::fs::write(&lock, CARGO_AFTER).unwrap();
    let young = hold_floor(&lock, CARGO_BEFORE, &Published(1)).unwrap();
    assert!(young.iter().any(|line| line.contains("recorded-crate@1.1.0: published less than 7 days ago")), "{young:?}");
    assert_eq!(std::fs::read_to_string(&lock).unwrap(), CARGO_BEFORE);
    std::fs::write(&lock, CARGO_AFTER).unwrap();
    let young = hold_floor(&lock, CARGO_BEFORE, &Published(8)).unwrap();
    assert_eq!(young, Vec::<String>::new());
    assert_eq!(std::fs::read_to_string(&lock).unwrap(), CARGO_AFTER);
}
