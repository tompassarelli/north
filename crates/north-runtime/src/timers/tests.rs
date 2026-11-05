use serde_json::{Value, json};

use super::child_failure;
use super::cross_project::{Asked, Reply, ask};
use super::eta_calibration::comment_body;
use super::git_maintenance::{maintenance_argv, repositories};
use super::gcroots::store_roots;
use super::runtime_update::steps;
use super::skill_prune::body_row;
use super::skill_review::{due_count, issue_body, items, without_grounded};
use super::skill_triggers::{SourceChanges, VersionProbe, lines, parse, version_token};
use super::supply_chain::{Finding, alert_texts, findings, lockfile_names, merged_seen};

fn osv() -> Value {
    json!({"results": [
        {"source": {"path": "/t/supply-chain-scan.abc123/north/Cargo.lock"}, "packages": [
            {"package": {"name": "evil", "version": "1.0.0"}, "vulnerabilities": [{"id": "MAL-2026-1"}, {"id": "MAL-2026-1"}]},
            {"package": {"name": "serde", "version": "1.0.1"}, "vulnerabilities": [
                {"id": "GHSA-aaaa", "affected": [
                    {"package": {"name": "other"}, "ranges": [{"events": [{"fixed": "9"}]}]},
                    {"package": {"name": "serde"}, "ranges": [{"events": [{"introduced": "0"}, {"fixed": "1.0.9"}]}]}]},
                {"id": "GHSA-nofix", "affected": [{"package": {"name": "serde"}, "ranges": [{"events": [{"introduced": "0"}]}]}]}]}]},
        {"source": {"path": "/t/supply-chain-scan.abc123/bun-global/bun.lock"}, "packages": [
            {"package": {"name": "left-pad", "version": "2.0.0"}, "vulnerabilities": [
                {"id": "GHSA-bbbb", "affected": [{"package": {"name": "left-pad"}, "ranges": [{"events": [{"fixed": "2.0.1"}]}]}]}]}]}]})
}

#[test]
fn lockfile_names_pick_the_lock_formats_and_skip_vendored_repos() {
    let listing = b"Cargo.lock\0README.md\0repos/x/bun.lock\0sub/package-lock.json\0sub/requirements-dev.txt\0sub/requirements.txt.bak\0uv.lock\0a/repos/b/Cargo.lock\0";
    assert_eq!(lockfile_names(listing), ["Cargo.lock", "sub/package-lock.json", "sub/requirements-dev.txt", "uv.lock"]);
}

#[test]
fn findings_are_unique_by_key_sorted_and_keep_the_fix_of_the_scanned_package() {
    let found = findings(&osv(), "/t/supply-chain-scan.abc123/");
    let keys: Vec<&str> = found.iter().map(|finding| finding.key.as_str()).collect();
    assert_eq!(keys, ["GHSA-aaaa north serde", "GHSA-bbbb bun-global left-pad", "GHSA-nofix north serde", "MAL-2026-1 north evil"]);
    assert_eq!(found[0].fixed.as_deref(), Some("1.0.9"));
    assert_eq!(found[0].pkg, "serde@1.0.1");
    assert_eq!(found[2].fixed, None);
}

#[test]
fn malware_alerts_urgent_and_only_fixable_advisories_alert_normal() {
    let found = findings(&osv(), "/t/supply-chain-scan.abc123/");
    let (urgent, normal) = alert_texts(&found);
    assert_eq!(urgent.as_deref(), Some("malware flagged: MAL-2026-1 evil@1.0.0 (north)"));
    assert_eq!(
        normal.as_deref(),
        Some("fixable advisories: GHSA-aaaa serde@1.0.1 (north) fixed 1.0.9; GHSA-bbbb left-pad@2.0.0 (bun-global) fixed 2.0.1")
    );
    let unfixed: Vec<Finding> = found.into_iter().filter(|finding| finding.fixed.is_none() && !finding.id.starts_with("MAL-")).collect();
    assert_eq!(alert_texts(&unfixed), (None, None));
}

#[test]
fn seen_ids_merge_sorted_without_duplicates() {
    let found = findings(&osv(), "/t/supply-chain-scan.abc123/");
    let merged = merged_seen(&["Z old".to_owned(), "GHSA-aaaa north serde".to_owned()], &found);
    assert_eq!(merged, ["GHSA-aaaa north serde", "GHSA-bbbb bun-global left-pad", "GHSA-nofix north serde", "MAL-2026-1 north evil", "Z old"]);
}

#[test]
fn store_roots_reduce_the_interpreter_and_rpath_to_unique_store_paths() {
    let roots = store_roots("/nix/store/bbb-glibc/lib/ld.so", "/nix/store/aaa-gcc/lib64:/usr/lib:/nix/store/bbb-glibc/lib:$ORIGIN/../lib");
    assert_eq!(roots, ["/nix/store/aaa-gcc", "/nix/store/bbb-glibc"]);
    assert!(store_roots("/lib64/ld.so", "/usr/lib").is_empty());
}

#[test]
fn skill_bodies_count_lines_after_the_frontmatter_and_mark_domain_and_long_skills() {
    let skill = |kind: &str, lines: usize| format!("---\nname: x\nmetadata:\n  kind: {kind}\n  owner: y\n---\n{}", "line\n".repeat(lines));
    assert_eq!(body_row("a", &skill("playbook", 40)), (40, "a".to_owned(), ""));
    assert_eq!(body_row("b", &skill("playbook", 75)), (75, "b".to_owned(), "over 60"));
    assert_eq!(body_row("c", &skill("domain", 120)), (120, "c".to_owned(), "domain"));
    assert_eq!(body_row("d", "no frontmatter here"), (0, "d".to_owned(), "unreadable"));
    assert_eq!(body_row("e", "---\nname: plain\n---\nl1\nl2\n"), (2, "e".to_owned(), ""));
}

#[test]
fn review_items_list_grounded_before_written_and_the_count_covers_every_due_key() {
    let stale = json!([
        {"id": "alpha", "due": {"written": {"date": "2026-08-01", "ageDays": 87}, "grounded": {"date": "2026-07-01", "ageDays": 118}}},
        {"id": "beta", "due": {"written": {"date": "2026-08-05", "ageDays": 83}}},
        {"id": "gamma", "due": {}},
        {"id": "delta", "due": {"grounded": {"date": "2026-09-01", "ageDays": 56}}}]);
    assert_eq!(
        items(&stale),
        "- [ ] `alpha` grounded (last 2026-07-01, 118 days)\n- [ ] `delta` grounded (last 2026-09-01, 56 days)\n- [ ] `alpha` written (last 2026-08-01, 87 days)\n- [ ] `beta` written (last 2026-08-05, 83 days)"
    );
    assert_eq!(due_count(&stale), 4);
    assert_eq!(due_count(&json!([{"id": "a", "due": {}}])), 0);
    assert!(issue_body("2026-10-27", "- [ ] `a`").ends_with("which review it was.\n\n- [ ] `a`"));
}

#[test]
fn nightly_maintenance_covers_each_main_git_checkout_but_not_archive() {
    let code = tempfile::tempdir().expect("tempdir");
    for name in ["alpha", "archive", "gamma", "nogit"] {
        std::fs::create_dir_all(code.path().join(name).join("main")).expect("checkout");
    }
    for name in ["alpha", "archive", "gamma"] {
        std::fs::create_dir_all(code.path().join(name).join("main/.git")).expect("git dir");
    }
    let names: Vec<String> = repositories(code.path()).into_iter().map(|(name, _)| name).collect();
    assert_eq!(names, ["alpha", "gamma"]);
    let argv = maintenance_argv(std::path::Path::new("/s/scripts/machine-capacity.mjs"), "alpha", std::path::Path::new("/c/alpha/main"));
    assert_eq!(
        argv.join(" "),
        "/s/scripts/machine-capacity.mjs run --class moderate --owner git-maintenance:alpha --timeout-seconds 3600 -- git -C /c/alpha/main -c pack.threads=2 maintenance run"
    );
}

#[test]
fn north_updates_before_claude_on_every_pass() {
    let mut order = Vec::new();
    let failed = steps("latest", &mut |program, args| {
        order.push(format!("{program} {}", args.join(" ")));
        0
    });
    assert!(failed.is_empty());
    assert_eq!(order, ["north runtime update", "north claude-runtime-update latest", "north agy-runtime-update latest"]);
}

#[test]
fn a_failed_runtime_update_is_named_and_does_not_stop_the_others() {
    let mut ran = 0;
    let failed = steps("latest", &mut |_, args| {
        ran += 1;
        match args[0] {
            "claude-runtime-update" => 7,
            "agy-runtime-update" => 9,
            _ => 0,
        }
    });
    assert_eq!(ran, 3);
    assert_eq!(failed, ["north claude-runtime-update latest (exit 7)", "north agy-runtime-update latest (exit 9)"]);
}

#[test]
fn a_failed_child_names_its_exit_code_and_the_last_non_empty_stderr_line() {
    assert_eq!(child_failure("claude", Some(1), b"warming up\nauth expired\n\n"), "claude exited 1: auth expired");
    assert_eq!(child_failure("claude", Some(2), b""), "claude exited 2");
    assert_eq!(child_failure("claude", None, b"  \n"), "claude exited 1");
}

fn reply(code: i32, stdout: &str, stderr: &str) -> Reply {
    Reply { code: Some(code), stdout: stdout.to_owned(), stderr: stderr.to_owned() }
}

fn asked_with(modes: &[(&str, &str)]) -> (Asked, Vec<String>, Vec<String>) {
    let profiles = tempfile::tempdir().expect("tempdir");
    for label in ["a", "b"] {
        std::fs::create_dir(profiles.path().join(format!("claude-{label}"))).unwrap();
    }
    let labels: Vec<String> = ["a", "b", "ghost"].map(str::to_owned).to_vec();
    let (mut tried, mut warned) = (Vec::new(), Vec::new());
    let asked = ask(
        profiles.path(),
        &labels,
        &mut |dir| {
            let label = dir.and_then(|dir| dir.file_name()).unwrap().to_string_lossy().trim_start_matches("claude-").to_owned();
            tried.push(label.clone());
            match modes.iter().find(|(name, _)| *name == label).map(|(_, mode)| *mode) {
                Some("limit") => reply(1, "", "You've hit your weekly limit"),
                Some("broken") => reply(1, "", "Not logged in"),
                _ => reply(0, &format!("answered by {label}"), ""),
            }
        },
        &mut |line| warned.push(line),
    );
    (asked, tried, warned)
}

#[test]
fn a_cross_project_scan_asks_each_pooled_account_in_turn_and_skips_the_day_only_when_every_account_is_at_its_limit() {
    let (asked, tried, _) = asked_with(&[("a", "ok"), ("b", "ok")]);
    assert!(matches!(asked, Asked::Answer(ref text) if text == "answered by a"));
    assert_eq!(tried, ["a"]);
    let (asked, tried, _) = asked_with(&[("a", "limit"), ("b", "ok")]);
    assert!(matches!(asked, Asked::Answer(ref text) if text == "answered by b"));
    assert_eq!(tried, ["a", "b"]);
    let (asked, _, warned) = asked_with(&[("a", "limit"), ("b", "limit")]);
    assert!(matches!(asked, Asked::AtLimit));
    assert!(warned.is_empty());
}

#[test]
fn a_cross_project_scan_account_that_fails_for_another_reason_fails_the_unit_and_is_named() {
    let (asked, _, warned) = asked_with(&[("a", "broken"), ("b", "limit")]);
    assert!(matches!(asked, Asked::Failed(_)));
    assert_eq!(warned, ["cross-project-scan: claude account a failed: Not logged in"]);
}

#[test]
fn a_cross_project_scan_with_no_pooled_profile_asks_the_default_account_once() {
    let profiles = tempfile::tempdir().expect("tempdir");
    let mut calls = 0;
    let asked = ask(profiles.path(), &["ghost".to_owned()], &mut |dir| {
        calls += 1;
        assert!(dir.is_none());
        reply(0, "default answer", "")
    }, &mut |_| {});
    assert!(matches!(asked, Asked::Answer(ref text) if text == "default answer"));
    assert_eq!(calls, 1);
}

#[test]
fn a_cross_project_scan_starts_a_token_file_account_through_its_credential_reader() {
    let profile = std::path::PathBuf::from("/profiles/claude-b");
    let token = crate::fleet::model::Account { provider: crate::fleet::model::ProviderName::new("claude"), label: "b".into(), home: profile.clone(), token_file: Some("/tokens/claude-b".into()) };
    let words = super::cross_project::account_words(Some(&profile), &[token], crate::providers::claude::invocation);
    let fast = words.iter().position(|word| word == "CLAUDE_CODE_DISABLE_FAST_MODE=1").unwrap();
    assert!(words[..fast].contains(&"CLAUDE_CONFIG_DIR=/profiles/claude-b".to_owned()), "{words:?}");
    assert_eq!(words[fast + 1..].first().map(String::as_str), Some("/bin/sh"), "{words:?}");
    assert_eq!(words.last().map(String::as_str), Some("/tokens/claude-b"), "{words:?}");
    let unlisted = super::cross_project::account_words(Some(std::path::Path::new("/profiles/claude-x")), &[], crate::providers::claude::invocation);
    assert_eq!(unlisted, ["CLAUDE_CONFIG_DIR=/profiles/claude-x", "CLAUDE_CODE_DISABLE_FAST_MODE=1"]);
}

#[test]
fn the_weekly_eta_calibration_comment_wraps_the_table_in_a_code_block_under_its_date() {
    let body = comment_body("2026-11-04", "category  tier  n\ntooling  claude:sonnet-high  3\n");
    assert!(body.starts_with("Weekly ETA calibration (2026-11-04, last 7d)"), "{body}");
    assert!(body.ends_with("tooling  claude:sonnet-high  3\n```"), "{body}");
}

struct Installed(&'static [(&'static str, &'static str)]);

impl VersionProbe for Installed {
    fn version(&self, tool: &str) -> Option<String> {
        self.0.iter().find(|(name, _)| *name == tool).map(|(_, version)| (*version).to_owned())
    }
}

struct Changed(&'static [&'static str]);

impl SourceChanges for Changed {
    fn changed_since(&self, source: &str, _grounded: &str) -> bool {
        self.0.contains(&source)
    }
}

fn day(text: &str) -> i64 {
    crate::foundation::clock::Timestamp::parse_loose(text).map(crate::foundation::clock::Timestamp::unix_days).unwrap()
}

fn skill(id: &str, front: &str) -> super::skill_triggers::Skill {
    parse(id, &format!("---\nname: {id}\ndescription: >-\n  text\n{front}metadata:\n  kind: playbook\n---\n\n# Body\n"))
}

#[test]
fn skill_review_flags_a_bumped_tool_version() {
    let nix = skill("nix-development", "grounded: 2026-10-30\nnix-version: 2.30.0\n");
    let found = lines(&[nix], day("2026-11-02"), &Installed(&[("nix", "2.34.9")]), &Changed(&[]));
    assert_eq!(found, ["- [ ] `nix-development` nix version drift (stamped 2.30.0, installed 2.34.9)"]);
}

#[test]
fn skill_review_leaves_a_matching_version_and_an_unprobeable_tool_alone() {
    let same = skill("a", "grounded: 2026-10-30\nnix-version: 2.34.9\n");
    let missing = skill("b", "grounded: 2026-10-30\nghost-version: 1.0\n");
    assert!(lines(&[same, missing], day("2026-11-02"), &Installed(&[("nix", "2.34.9")]), &Changed(&[])).is_empty());
}

#[test]
fn skill_review_version_stamp_matches_by_whole_components() {
    let minor = skill("a", "grounded: 2026-10-30\nnix-version: 2.34\n");
    let other = skill("b", "grounded: 2026-10-30\nnix-version: 2.3\n");
    let found = lines(&[minor, other], day("2026-11-02"), &Installed(&[("nix", "2.34.9")]), &Changed(&[]));
    assert_eq!(found, ["- [ ] `b` nix version drift (stamped 2.3, installed 2.34.9)"]);
}

#[test]
fn skill_review_cadence_of_14_days_is_due_at_day_15_and_not_at_day_13() {
    let fast = skill("nix-development", "grounded: 2026-10-15\ncadence: 14\n");
    let probe = Installed(&[]);
    assert!(lines(std::slice::from_ref(&fast), day("2026-10-28"), &probe, &Changed(&[])).is_empty());
    assert!(lines(std::slice::from_ref(&fast), day("2026-10-29"), &probe, &Changed(&[])).len() == 1);
    assert_eq!(
        lines(&[fast], day("2026-10-30"), &probe, &Changed(&[])),
        ["- [ ] `nix-development` grounded (last 2026-10-15, 15 days, cadence 14)"]
    );
}

#[test]
fn skill_review_default_cadence_is_30_days() {
    let slow = skill("s", "grounded: 2026-10-15\n");
    assert!(lines(std::slice::from_ref(&slow), day("2026-11-13"), &Installed(&[]), &Changed(&[])).is_empty());
    assert_eq!(lines(&[slow], day("2026-11-14"), &Installed(&[]), &Changed(&[])).len(), 1);
}

#[test]
fn skill_review_flags_a_changed_cited_source_and_leaves_a_fresh_skill_alone() {
    let cited = skill("cited", "grounded: 2026-10-30\nsources: [nix-manual, pstack]\n");
    let fresh = skill("fresh", "grounded: 2026-10-30\nnix-version: 2.34.9\nsources: [pstack]\ncadence: 14\n");
    let found = lines(&[cited, fresh], day("2026-11-02"), &Installed(&[("nix", "2.34.9")]), &Changed(&["nix-manual"]));
    assert_eq!(found, ["- [ ] `cited` source `nix-manual` changed since grounded 2026-10-30"]);
}

#[test]
fn skill_review_parse_reads_top_level_fields_only() {
    let parsed = skill("x", "grounded: 2026-10-15\ncadence: 14\nnix-version: 2.34.9\nsources: [a, \"b\"]\n");
    assert_eq!(parsed.cadence, Some(14));
    assert_eq!(parsed.versions, [("nix".to_owned(), "2.34.9".to_owned())]);
    assert_eq!(parsed.sources, ["a", "b"]);
    assert_eq!(skill("y", "cadence: 0\n").cadence, None);
    assert_eq!(version_token("nix (Nix) 2.34.9\n").as_deref(), Some("2.34.9"));
    assert_eq!(version_token("git version 2.51.0"), Some("2.51.0".to_owned()));
}

#[test]
fn skill_review_age_listing_defers_grounded_to_the_skill_triggers() {
    let stale = json!([{"id": "nix-development", "due": {"grounded": {"date": "2026-10-15", "ageDays": 31}, "written": {"date": "2026-10-15", "ageDays": 31}}}]);
    let owned = [skill("nix-development", "grounded: 2026-10-15\n")];
    assert_eq!(items(&without_grounded(&stale, &owned)), "- [ ] `nix-development` written (last 2026-10-15, 31 days)");
}
