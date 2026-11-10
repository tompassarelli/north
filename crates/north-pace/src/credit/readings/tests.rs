use std::path::Path;

use super::{Reading, Source, directory, get, record, record_console};
use crate::credit::sources;
use crate::foundation::clock::Timestamp;

const AT: i64 = 1_794_305_400;

fn place(dir: &Path) {
    std::fs::write(
        dir.join("orchestration.toml"),
        "[accounts.claude.api_orgs.gmail]\nid = \"g\"\nkeys = { here = [\"/k/g\"] }\n",
    )
    .unwrap();
    std::fs::write(dir.join("claude-accounts.toml"), "[[account]]\n[account.api_org]\nid = \"g\"\ngrants = [{ usd = 200.0, expires = \"2026-11-12\" }]\n").unwrap();
}

#[test]
fn a_console_reading_overrides_the_list_price_estimate_and_the_plan_line_names_its_time() {
    let dir = tempfile::tempdir().unwrap();
    place(dir.path());
    let state = dir.path().join("state");

    let line = record_console(
        &dir.path().join("orchestration.toml"),
        &state,
        "api:gmail",
        111.0,
        AT,
    )
    .unwrap();
    assert!(
        line.contains("$111.00") && line.contains("2026-11-10T10:10Z"),
        "{line}"
    );

    let sources = sources(
        &dir.path().join("orchestration.toml"),
        &state,
        Timestamp::from_unix_seconds(AT),
    );
    let gmail = sources
        .iter()
        .find(|source| source.name == "gmail")
        .unwrap();
    assert_eq!(gmail.left, 111.0);
    assert_eq!(gmail.estimated, 200.0);
    let line = gmail.line(AT);
    assert!(
        line.contains("$111.00 left (console reading 2026-11-10T10:10Z, estimate $200.00)"),
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
                usd: 111.0,
                at: AT + 60,
                source: Source::Console
            }
        )
        .unwrap()
    );

    let kept = get(&readings, id).unwrap();
    assert_eq!((kept.usd, kept.source), (120.0, Source::Provider));
}

#[test]
fn a_console_reading_may_name_the_account_with_or_without_its_provider_prefix() {
    let dir = tempfile::tempdir().unwrap();
    place(dir.path());
    let state = dir.path().join("state");
    let policy = dir.path().join("orchestration.toml");
    record_console(&policy, &state, "claude:api:gmail", 98.5, AT).unwrap();
    assert_eq!(get(&directory(&state), "g").unwrap().usd, 98.5);
}
#[test]
fn a_reading_file_written_by_the_console_overrides_the_estimate_even_without_the_recording_helper() {
    let dir = tempfile::tempdir().unwrap();
    place(dir.path());
    let state = dir.path().join("state");
    let readings = directory(&state);
    std::fs::create_dir_all(&readings).unwrap();
    std::fs::write(readings.join("g"), "console 111.00 2026-11-10T10:10:00Z\n").unwrap();

    let sources = sources(&dir.path().join("orchestration.toml"), &state, Timestamp::from_unix_seconds(AT));
    let gmail = sources.iter().find(|source| source.name == "gmail").unwrap();
    assert_eq!(gmail.left, 111.0);
    assert!(gmail.line(AT).contains("$111.00 left (console reading 2026-11-10T10:10Z"), "{}", gmail.line(AT));
}
