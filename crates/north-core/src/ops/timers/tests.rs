use serde_json::{Value, json};

use super::child_failure;
use super::cross_project::{Asked, Reply, ask};
use super::git_maintenance::{maintenance_argv, repositories};
use super::gcroots::store_roots;
use super::runtime_update::steps;
use super::skill_prune::body_row;
use super::skill_review::{due_count, issue_body, items};
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
    assert_eq!(order, ["codex-runtime-update latest", "north runtime update", "claude-runtime-update latest", "agy-runtime-update latest"]);
}

#[test]
fn a_failed_runtime_update_is_named_and_does_not_stop_the_others() {
    let mut ran = 0;
    let failed = steps("latest", &mut |program, _| {
        ran += 1;
        match program {
            "codex-runtime-update" => 7,
            "agy-runtime-update" => 9,
            _ => 0,
        }
    });
    assert_eq!(ran, 4);
    assert_eq!(failed, ["codex-runtime-update latest (exit 7)", "agy-runtime-update latest (exit 9)"]);
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
