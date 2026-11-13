use std::path::Path;

use super::{Reading, Source, directory, get, record, record_console};
use crate::credit::{sources, standing};
use crate::foundation::clock::Timestamp;

const AT: i64 = 1_794_305_400;

fn place(dir: &Path) {
    std::fs::write(
        dir.join("orchestration.toml"),
        "[accounts.claude.api_orgs.ember]\nid = \"g\"\nkeys = { here = [\"/k/g\"] }\n",
    )
    .unwrap();
    std::fs::write(dir.join("claude-accounts.toml"), "[[account]]\n[account.api_org]\nid = \"g\"\ngrants = [{ usd = 240.0, expires = \"2026-11-12\" }]\n").unwrap();
}

#[test]
fn a_console_reading_overrides_the_list_price_estimate_and_the_plan_line_names_its_time() {
    let dir = tempfile::tempdir().unwrap();
    place(dir.path());
    let state = dir.path().join("state");

    let line = record_console(
        &dir.path().join("orchestration.toml"),
        &state,
        "api:ember",
        137.0,
        AT,
    )
    .unwrap();
    assert!(
        line.contains("$137.00") && line.contains("2026-11-10T10:10Z"),
        "{line}"
    );

    let sources = sources(&standing(
        &dir.path().join("orchestration.toml"),
        &state,
        Timestamp::from_unix_seconds(AT),
    ));
    let ember = sources
        .iter()
        .find(|source| source.credit.name == "ember")
        .unwrap();
    assert_eq!(ember.credit.left, 137.0);
    assert_eq!(ember.credit.estimated, 240.0);
    let line = ember.line(AT);
    assert!(
        line.contains("$137.00 left (console reading 2026-11-10T10:10Z, estimate $240.00)"),
        "{line}"
    );
}

#[test]
fn a_provider_reading_outranks_a_console_reading_until_a_newer_provider_figure_is_recorded() {
    let dir = tempfile::tempdir().unwrap();
    place(dir.path());
    let readings = directory(&dir.path().join("state"));
    let id = "g";
    assert!(
        record(
            &readings,
            id,
            &Reading {
                usd: 120.0,
                at: AT,
                source: Source::Provider
            }
        )
        .unwrap()
    );
    assert!(
        !record(
            &readings,
            id,
            &Reading {
                usd: 137.0,
                at: AT + 60,
                source: Source::Console
            }
        )
        .unwrap()
    );

    let kept = get(&readings, id).unwrap();
    assert_eq!((kept.usd, kept.source), (120.0, Source::Provider));
    assert!(
        !record(&readings, id, &Reading { usd: 130.0, at: AT - 60, source: Source::Provider }).unwrap(),
        "an older provider reading replaced a newer one"
    );
    assert_eq!(get(&readings, id).unwrap().usd, 120.0);
    assert!(
        record(&readings, id, &Reading { usd: 110.0, at: AT + 120, source: Source::Provider }).unwrap(),
        "a newer provider reading did not replace the older one"
    );
    assert_eq!(get(&readings, id).unwrap().usd, 110.0);
}

#[test]
fn a_console_reading_may_name_the_account_with_or_without_its_provider_prefix() {
    let dir = tempfile::tempdir().unwrap();
    place(dir.path());
    let state = dir.path().join("state");
    let policy = dir.path().join("orchestration.toml");
    record_console(&policy, &state, "claude:api:ember", 98.5, AT).unwrap();
    assert_eq!(get(&directory(&state), "g").unwrap().usd, 98.5);
}
#[test]
fn a_reading_file_written_by_the_console_overrides_the_estimate_even_without_the_recording_helper() {
    let dir = tempfile::tempdir().unwrap();
    place(dir.path());
    let state = dir.path().join("state");
    let readings = directory(&state);
    std::fs::create_dir_all(&readings).unwrap();
    std::fs::write(readings.join("g"), "console 137.00 2026-11-10T10:10:00Z\n").unwrap();

    let sources = sources(&standing(&dir.path().join("orchestration.toml"), &state, Timestamp::from_unix_seconds(AT)));
    let ember = sources.iter().find(|source| source.credit.name == "ember").unwrap();
    assert_eq!(ember.credit.left, 137.0);
    assert!(ember.line(AT).contains("$137.00 left (console reading 2026-11-10T10:10Z"), "{}", ember.line(AT));
}
