use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::json;

use super::Denylist;
use super::refuse::refuse_static_gates;

fn run(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(repo).args(["-c", "commit.gpgsign=false", "-c", "user.name=Gate fixture", "-c", "user.email=fixture@example.invalid"]).args(args).output().unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn allow(repo: &Path, globs: &[&str]) {
    let entries: Vec<_> = globs.iter().map(|glob| json!({"glob":glob,"class":"script","gap":"fixture gap","provenance":"Tom 2026-10-15: permit this fixture","port_owner":"fixture-port-owner","port_target":"src/ported.rs","commitment":"Rust port by 2026-10-16 18:00","created":"2026-10-15"})).collect();
    fs::write(repo.join(".language-allowlist.json"), json!({"ratchet":["script"],"entries":entries}).to_string()).unwrap();
}

fn commit(repo: &Path, message: &str) -> String {
    run(repo, &["add", "--all"]);
    run(repo, &["commit", "-q", "-m", message]);
    run(repo, &["rev-parse", "HEAD"])
}

#[test]
fn a_lane_only_behind_main_is_judged_on_its_own_changes_not_on_what_main_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    run(repo, &["init", "-q", "-b", "main"]);
    allow(repo, &["tool.sh"]);
    let fork = commit(repo, "allow a script");
    allow(repo, &[]);
    let main = commit(repo, "main drops the allowance");
    run(repo, &["checkout", "-q", "-b", "lane", &fork]);
    fs::write(repo.join("notes.md"), "lane change\n").unwrap();
    commit(repo, "lane touches only notes");
    assert!(!crate::language_gate::check(repo, &main, "HEAD"), "diffing from main's tip shows main's removal as the lane widening the allowlist");
    assert_eq!(refuse_static_gates(repo, &main, "HEAD"), Ok(()));
}

struct Lane {
    dir: tempfile::TempDir,
    repo: std::path::PathBuf,
    base: String,
}

impl Lane {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let (origin, repo) = (dir.path().join("origin.git"), dir.path().join("lane"));
        run(dir.path(), &["init", "-q", "--bare", "-b", "main", origin.to_str().unwrap()]);
        run(dir.path(), &["init", "-q", "-b", "main", repo.to_str().unwrap()]);
        run(&repo, &["remote", "add", "origin", origin.to_str().unwrap()]);
        for (key, value) in [("user.name", "Gate fixture"), ("user.email", "fixture@example.invalid"), ("commit.gpgsign", "false")] {
            run(&repo, &["config", key, value]);
        }
        fs::write(repo.join("content.txt"), "base\n").unwrap();
        let base = commit(&repo, "base");
        run(&repo, &["push", "-q", "origin", "main"]);
        run(&repo, &["switch", "-q", "-c", "lane"]);
        run(&repo, &["commit", "-q", "--allow-empty", "-m", "outgoing"]);
        Self { dir, repo, base }
    }

    fn commit(&self, subject: &str, trailers: &[&str]) {
        fs::write(self.repo.join("content.txt"), format!("{}{subject}\n", fs::read_to_string(self.repo.join("content.txt")).unwrap())).unwrap();
        run(&self.repo, &["add", "--all"]);
        let mut args = vec!["commit", "-q", "-m", subject];
        for trailer in trailers {
            args.extend(["--trailer", trailer]);
        }
        run(&self.repo, &args);
    }

    fn try_enqueue(&self, shape: super::Shape) -> Result<super::Queued, String> {
        self.try_enqueue_from("fixture-host", shape)
    }

    fn try_enqueue_from(&self, host: &str, shape: super::Shape) -> Result<super::Queued, String> {
        let (state, denylist) = (self.dir.path().join("state"), without_south(self.dir.path()));
        let environment = super::Environment { called_micros: 0, state_home: &state, host, session: None, waits: false, denylist: &denylist };
        super::enqueue(&self.repo, shape, false, &environment)
    }

    fn enqueue(&self, shape: super::Shape) -> super::Queued {
        self.try_enqueue(shape).unwrap()
    }

    fn shadowed(&self) -> String {
        fs::read_to_string(self.dir.path().join("state").join(crate::landing_refusals::LOG)).unwrap_or_default()
    }
}

#[test]
fn a_queued_lane_is_one_commit_keeping_each_trailer_once_unless_several_commits_keeps_it_as_it_is_a_test_over_the_budget_or_a_revert_of_newer_main_queues_with_its_would_be_refusal_logged_and_a_conflict_with_main_is_refused_before_it_queues() {
    let lane = Lane::new();
    lane.commit("add the first lane line", &["Refs: #7", "Lane-note: first"]);
    lane.commit("add the second lane line", &["Refs: #7", "Lane-note: second"]);
    let tree = run(&lane.repo, &["rev-parse", "HEAD^{tree}"]);
    let queued = lane.enqueue(super::Shape::OneCommit { message: None });
    assert_eq!((queued.ticket.head, run(&lane.repo, &["rev-parse", "HEAD^"]), run(&lane.repo, &["rev-parse", "HEAD^{tree}"])), (run(&lane.repo, &["rev-parse", "HEAD"]), lane.base.clone(), tree));
    let message = run(&lane.repo, &["show", "-s", "--format=%B", "HEAD"]);
    assert_eq!(message.lines().next(), Some("add the second lane line"), "{message}");
    assert_eq!(run(&lane.repo, &["show", "-s", "--format=%(trailers:only,unfold)", "HEAD"]), "Refs: #7\nLane-note: first\nLane-note: second");

    let lane = Lane::new();
    run(&lane.repo, &["push", "-q", "origin", "HEAD:refs/heads/shared"]);
    let published = run(&lane.repo, &["rev-parse", "HEAD"]);
    lane.commit("add a line after the published commit", &[]);
    lane.commit("add another line after the published commit", &[]);
    lane.enqueue(super::Shape::OneCommit { message: None });
    assert_eq!(run(&lane.repo, &["rev-parse", "HEAD^"]), published, "squashing rewrote a commit already on origin");

    let lane = Lane::new();
    run(&lane.repo, &["commit", "--amend", "--allow-empty", "-q", "-m", "outgoing", "--trailer", "Refs: #9"]);
    lane.enqueue(super::Shape::OneCommit { message: Some("Name the change once\n\nWhy it lands.\n\nLane-note: supplied\n") });
    assert_eq!(run(&lane.repo, &["show", "-s", "--format=%B", "HEAD"]), "Name the change once\n\nWhy it lands.\n\nLane-note: supplied\nRefs: #9");

    let lane = Lane::new();
    lane.commit("a line kept as its own commit", &[]);
    let head = run(&lane.repo, &["rev-parse", "HEAD"]);
    let queued = lane.enqueue(super::Shape::SeveralCommits { reason: "each commit is a bisect point" });
    assert_eq!((queued.ticket.several_commits.as_deref(), queued.ticket.head.as_str()), (Some("each commit is a bisect point"), head.as_str()));
    assert_eq!(run(&lane.repo, &["rev-list", "--count", "origin/main..HEAD"]), "2", "--several-commits squashed the lane");

    let lane = Lane::new();
    let ledger = lane.dir.path().join("state/safe-push/post-land.tsv");
    fs::create_dir_all(ledger.parent().unwrap()).unwrap();
    fs::write(&ledger, "1\torigin\tabc\tgreen\t\tlane\t900\t\t100000\n".repeat(3)).unwrap();
    fs::create_dir_all(lane.repo.join("src")).unwrap();
    fs::write(lane.repo.join("src/tests.rs"), "#[test]\nfn added() {}\n").unwrap();
    lane.commit("add a test while verification is over its budget", &[]);
    lane.enqueue(super::Shape::OneCommit { message: None });
    let shadowed = lane.shadowed();
    assert!(shadowed.contains("\"gate\":\"test admission\"") && shadowed.contains("test admission: origin's verification takes 100000 s"), "{shadowed}");

    let lane = Lane::new();
    run(&lane.repo, &["config", "landing.lander-host", "lander-fixture"]);
    fs::create_dir_all(lane.repo.join("src")).unwrap();
    fs::write(lane.repo.join("src/tests.rs"), "#[test]\nfn added() {}\n").unwrap();
    lane.commit("add a test from a host that keeps no post-land ledger", &[]);
    let mirror = super::queue::directory(&lane.dir.path().join("state"), &run(&lane.repo, &["remote", "get-url", "--push", "origin"])).join(super::queue::LANDER_LEDGER);
    fs::create_dir_all(mirror.parent().unwrap()).unwrap();
    fs::write(&mirror, "1\torigin\tabc\tgreen\t\tlane\t900\t\t100000\n".repeat(3)).unwrap();
    lane.try_enqueue_from("build-fixture", super::Shape::OneCommit { message: None }).unwrap();
    let shadowed = lane.shadowed();
    assert!(shadowed.contains("test admission: origin's verification takes 100000 s"), "{shadowed}");

    let lane = Lane::new();
    run(&lane.repo, &["switch", "-q", "main"]);
    fs::write(lane.repo.join("content.txt"), "base rewritten on main\n").unwrap();
    let newer = commit(&lane.repo, "main rewrites the content");
    run(&lane.repo, &["push", "-q", "origin", "main"]);
    run(&lane.repo, &["switch", "-q", "lane"]);
    run(&lane.repo, &["reset", "-q", "--soft", "main"]);
    fs::write(lane.repo.join("notes.md"), "lane work\n").unwrap();
    commit(&lane.repo, "lane squashed onto main without a rebase");
    lane.enqueue(super::Shape::OneCommit { message: None });
    let shadowed = lane.shadowed();
    assert!(shadowed.contains("\"gate\":\"revert guard\"") && shadowed.contains("undoes recent commits on main") && shadowed.contains(&newer[..9]), "{shadowed}");

    let lane = Lane::new();
    lane.commit("the lane adds its own line", &[]);
    run(&lane.repo, &["switch", "-q", "main"]);
    lane.commit("main adds a different line", &[]);
    run(&lane.repo, &["push", "-q", "origin", "main"]);
    run(&lane.repo, &["switch", "-q", "lane"]);
    let refused = lane.try_enqueue(super::Shape::OneCommit { message: None }).unwrap_err();
    assert!(refused.contains("conflicts with origin/main") && refused.contains("on content.txt"), "{refused}");
    let queue = super::queue::directory(&lane.dir.path().join("state"), &run(&lane.repo, &["remote", "get-url", "--push", "origin"]));
    assert!(fs::read_dir(&queue).map_or(true, |mut entries| entries.next().is_none()), "a conflicting lane left a ticket in {}", queue.display());

    let lane = Lane::new();
    run(&lane.repo, &["switch", "-q", "main"]);
    fs::write(lane.repo.join(".safe-push"), "[land]\n\tcheck = @enqueue,harness/ :: test ! -e harness/skill.md\n").unwrap();
    commit(&lane.repo, "main carries the policy check");
    run(&lane.repo, &["push", "-q", "origin", "main"]);
    run(&lane.repo, &["switch", "-q", "lane"]);
    run(&lane.repo, &["rebase", "-q", "main"]);
    fs::create_dir_all(lane.repo.join("harness")).unwrap();
    fs::write(lane.repo.join("harness/skill.md"), "grown\n").unwrap();
    lane.commit("grow harness text", &[]);
    let refused = lane.try_enqueue(super::Shape::OneCommit { message: None }).unwrap_err();
    assert!(refused.contains("a check before queueing exited 1"), "{refused}");
}

#[test]
fn a_lane_that_began_with_a_wip_commit_lands_under_its_last_commit_and_a_lane_ending_on_one_is_refused() {
    let lane = Lane::new();
    run(&lane.repo, &["commit", "--amend", "--allow-empty", "-q", "-m", "WIP: resume module, reaper keep and tests not written yet"]);
    lane.commit("Resume a landing the lander stopped on", &[]);
    lane.enqueue(super::Shape::OneCommit { message: None });
    assert_eq!(run(&lane.repo, &["show", "-s", "--format=%B", "HEAD"]), "Resume a landing the lander stopped on");

    let lane = Lane::new();
    run(&lane.repo, &["commit", "--amend", "--allow-empty", "-q", "-m", "Add the resume module"]);
    lane.commit("wip: reaper keep", &[]);
    let refusal = lane.try_enqueue(super::Shape::OneCommit { message: None }).unwrap_err();
    assert!(refusal.contains("lane lane") && refusal.contains("--message"), "{refusal}");
    lane.enqueue(super::Shape::OneCommit { message: Some("Resume a landing the lander stopped on\n") });
}

#[test]
fn a_withdrawn_lane_is_refused_with_the_withdrawal_reason_when_its_session_queues_the_same_head_until_the_withdrawer_clears_it_or_the_head_changes() {
    land_ticket_model_is_sound();
    let lane = Lane::new();
    let (state, denylist) = (lane.dir.path().join("state"), without_south(lane.dir.path()));
    let enqueue_as = |session: &str| {
        let environment = super::Environment { called_micros: 0, state_home: &state, host: "fixture-host", session: Some(session), waits: false, denylist: &denylist };
        super::enqueue(&lane.repo, super::Shape::OneCommit { message: None }, false, &environment)
    };
    let queued = enqueue_as("worker-1").unwrap();
    let queue = super::queue::directory(&state, &run(&lane.repo, &["remote", "get-url", "--push", "origin"]));
    super::queue::settle(&queued.path, queued.ticket.clone(), "withdrawn by proxy: caller audit pending".to_owned(), north_base::land_ticket::Phase::Withdrawn).unwrap();

    let refused = enqueue_as("worker-1").unwrap_err();
    assert!(refused.contains("withdrawn by proxy: caller audit pending") && refused.contains("clear-withdrawal"), "{refused}");
    let name = queued.path.file_stem().unwrap().to_string_lossy().into_owned();
    north_land_scans::withdrawal::clear(&queue, &name, "proxy", "audit done").unwrap();
    let requeued = enqueue_as("worker-1").unwrap();

    super::queue::settle(&requeued.path, requeued.ticket, "withdrawn by proxy: again".to_owned(), north_base::land_ticket::Phase::Withdrawn).unwrap();
    assert!(enqueue_as("worker-1").is_err());
    lane.commit("a changed head", &[]);
    enqueue_as("worker-1").unwrap();
}

#[test]
fn the_lane_local_land_checks_that_refused_n882_context_in_every_probe_run_before_its_ticket_is_written() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    run(repo, &["init", "-q", "-b", "main"]);
    fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.safe-push"), repo.join(".safe-push")).unwrap();
    let base = commit(repo, "main carries north's land lines");
    for (path, text) in [("crates/north-context-pack/src/pack.rs", "use north_graph_ground::ground;\n"), ("docs/rationale/experiments.toml", "[[experiment]]\nname = \"context-pack\"\n")] {
        fs::create_dir_all(repo.join(path).parent().unwrap()).unwrap();
        fs::write(repo.join(path), text).unwrap();
    }
    commit(repo, "n882-context: pack context from graph-ground");
    let queued = crate::land::slots::runner::queue_commands(repo, &base, "HEAD").unwrap();
    for gate in ["cargo clippy --all-targets", "code-health layers", "check experiments", "check citations", "land scan test-admission kills", "code-health habits", "cargo nextest run"] {
        assert!(queued.iter().any(|command| command.contains(gate)), "{gate} is not run before queueing: {queued:#?}");
    }
    let (store, entry) = ("~/.local/share/makgora-build-inputs/store", "14fbf7845b5d9c9705d0f0b4b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0");
    fs::create_dir_all(repo.join("build-inputs")).unwrap();
    fs::write(repo.join("build-inputs/impact-assets"), format!("{entry}\n")).unwrap();
    let mut config = fs::read_to_string(repo.join(".safe-push")).unwrap();
    config.push_str(&format!("\n[land]\n\thost-input = {store}@build-inputs :: bun wisp map build\n"));
    fs::write(repo.join(".safe-push"), config).unwrap();
    commit(repo, "pin makgora's impact-assets entry");
    let pinned_entry = format!("{store}/impact-assets/{entry}");
    let pinned = crate::land::slots::select::Config::load(repo, "HEAD");
    assert_eq!(pinned.host_inputs("bun wisp map build"), [store, pinned_entry.as_str()], "a store declaration names the entries the head pins, not only the store path, so a host whose store lacks one is skipped");
}

#[test]
fn a_ref_lock_a_killed_fetch_left_in_the_shared_git_dir_no_longer_stops_the_enqueue_fetch_north_937() {
    let lane = Lane::new();
    run(&lane.repo, &["push", "-q", "origin", "lane:moved"]);
    let origin = lane.dir.path().join("origin.git");
    run(&origin, &["update-ref", "refs/heads/main", "refs/heads/moved"]);
    let moved = run(&lane.repo, &["rev-parse", "lane"]);
    let lock = lane.repo.join(".git/refs/remotes/origin/main.lock");
    fs::create_dir_all(lock.parent().unwrap()).unwrap();
    fs::File::create(&lock).unwrap().set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000)).unwrap();
    assert_eq!(super::git(&lane.repo, &["fetch", "-q", "origin", "+refs/heads/main:refs/remotes/origin/main"]), Ok(String::new()));
    assert!(!lock.exists());
    assert_eq!(run(&lane.repo, &["rev-parse", "refs/remotes/origin/main"]), moved);
}

struct Published {
    versions: &'static [&'static str],
    days_ago: i64,
}

impl crate::dep_gate::Registry for Published {
    fn read(&self, host: &str, path: &str) -> Result<String, &'static str> {
        let name = path.rsplit('/').next().unwrap_or_default();
        let at = super::rfc3339(time::OffsetDateTime::now_utc() - time::Duration::days(self.days_ago));
        if host == "registry.npmjs.org" {
            let time: serde_json::Map<String, serde_json::Value> = self.versions.iter().map(|version| ((*version).to_owned(), json!(at))).collect();
            return Ok(json!({"time": time}).to_string());
        }
        Ok(self.versions.iter().map(|version| json!({"name": name, "vers": version, "pubtime": at}).to_string()).collect::<Vec<_>>().join("\n"))
    }
}

const FIXTURE_LOCK: &str = "version = 4\n\n[[package]]\nname = \"fixture\"\nversion = \"0.1.0\"\n";

fn lane_locking(crate_name: &str, version: &str, advisories: &[(&str, &str)]) -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let (repo, database) = (dir.path().join("repo"), dir.path().join("state/north/advisory-db/advisory-db-3157b0e258782691"));
    for (id, patched) in advisories {
        fs::create_dir_all(database.join("crates").join(crate_name)).unwrap();
        fs::write(database.join(format!("crates/{crate_name}/{id}.md")), format!("```toml\n[advisory]\nid = \"{id}\"\npackage = \"{crate_name}\"\ndate = \"2026-11-01\"\nurl = \"https://example.invalid/{id}\"\n\n[versions]\npatched = [\"{patched}\"]\n```\n\n# Fixture advisory\n\nA fixture advisory.\n")).unwrap();
    }
    fs::create_dir_all(&database).unwrap();
    fs::write(database.join("README.md"), "fixture advisory database\n").unwrap();
    run(&database, &["init", "-q"]);
    commit(&database, "advisory database");
    fs::write(dir.path().join("state/north/advisory-db/fetched"), "").unwrap();
    fs::create_dir_all(&repo).unwrap();
    run(&repo, &["init", "-q", "-b", "main"]);
    fs::write(repo.join("Cargo.lock"), FIXTURE_LOCK).unwrap();
    let main = commit(&repo, "main");
    run(&repo, &["checkout", "-q", "-b", "lane"]);
    fs::write(repo.join("Cargo.lock"), format!("{FIXTURE_LOCK}dependencies = [\"{crate_name}\"]\n\n[[package]]\nname = \"{crate_name}\"\nversion = \"{version}\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n")).unwrap();
    commit(&repo, "lane locks a crate");
    (dir, main)
}

#[test]
fn a_lane_locking_a_crate_published_this_week_is_refused_at_enqueue() {
    let (dir, main) = lane_locking("aes", "0.8.5", &[]);
    let refused = super::refuse::refuse_lane_gates(&dir.path().join("repo"), &main, "HEAD", &dir.path().join("state"), None, &Published { versions: &["0.8.5"], days_ago: 1 }).unwrap();
    let (gates, detail) = refused.unwrap_err();
    assert!(gates.contains("dependency age"), "{gates}: {detail}");
    assert!(detail.contains("Cargo.lock: aes@0.8.5: published less than 7 days ago"), "{detail}");
}

#[test]
fn a_lane_locking_a_version_with_an_open_advisory_is_refused() {
    let (dir, main) = lane_locking("itoa", "1.0.18", &[("RUSTSEC-2026-9999", ">= 1.0.19")]);
    let refused = super::refuse::refuse_lane_gates(&dir.path().join("repo"), &main, "HEAD", &dir.path().join("state"), None, &Published { versions: &["1.0.18"], days_ago: 30 }).unwrap();
    let (gates, detail) = refused.unwrap_err();
    assert!(gates.contains("dependency advisory") && !gates.contains("dependency age"), "{gates}: {detail}");
    assert!(detail.contains("Cargo.lock: itoa@1.0.18: RUSTSEC-2026-9999"), "{detail}");
}

fn stored_zip(entries: &[(&str, &str)]) -> Vec<u8> {
    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = u32::MAX;
        for byte in bytes {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
            }
        }
        !crc
    }
    let (mut body, mut directory) = (Vec::new(), Vec::new());
    for (name, text) in entries {
        let offset = body.len() as u32;
        let (crc, size, name) = (crc32(text.as_bytes()), text.len() as u32, name.as_bytes());
        body.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        body.extend_from_slice(&20u16.to_le_bytes());
        body.extend_from_slice(&[0; 8]);
        body.extend_from_slice(&crc.to_le_bytes());
        body.extend_from_slice(&size.to_le_bytes());
        body.extend_from_slice(&size.to_le_bytes());
        body.extend_from_slice(&(name.len() as u16).to_le_bytes());
        body.extend_from_slice(&[0; 2]);
        body.extend_from_slice(name);
        body.extend_from_slice(text.as_bytes());
        directory.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        directory.extend_from_slice(&20u16.to_le_bytes());
        directory.extend_from_slice(&20u16.to_le_bytes());
        directory.extend_from_slice(&[0; 8]);
        directory.extend_from_slice(&crc.to_le_bytes());
        directory.extend_from_slice(&size.to_le_bytes());
        directory.extend_from_slice(&size.to_le_bytes());
        directory.extend_from_slice(&(name.len() as u16).to_le_bytes());
        directory.extend_from_slice(&[0; 12]);
        directory.extend_from_slice(&offset.to_le_bytes());
        directory.extend_from_slice(name);
    }
    let start = body.len() as u32;
    let count = entries.len() as u16;
    body.extend_from_slice(&directory);
    body.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    body.extend_from_slice(&[0; 4]);
    body.extend_from_slice(&count.to_le_bytes());
    body.extend_from_slice(&count.to_le_bytes());
    body.extend_from_slice(&(directory.len() as u32).to_le_bytes());
    body.extend_from_slice(&start.to_le_bytes());
    body.extend_from_slice(&[0; 2]);
    body
}

const BUN_BEFORE: &str = "{\"lockfileVersion\":1,\"workspaces\":{\"\":{\"name\":\"fixture\"}},\"packages\":{}}";

fn lane_locking_npm(name: &str, version: &str, advisories: &[(&str, &str)]) -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let (repo, state) = (dir.path().join("repo"), dir.path().join("state/north"));
    let advisories: Vec<(String, String)> = advisories
        .iter()
        .map(|(id, fixed)| {
            let advisory = json!({"schema_version": "1.6.0", "id": id, "modified": "2026-11-01T00:00:00Z", "summary": "fixture advisory", "affected": [{"package": {"ecosystem": "npm", "name": name}, "ranges": [{"type": "SEMVER", "events": [{"introduced": "0"}, {"fixed": fixed}]}]}]});
            (format!("{id}.json"), advisory.to_string())
        })
        .collect();
    let entries: Vec<(&str, &str)> = advisories.iter().map(|(id, advisory)| (id.as_str(), advisory.as_str())).collect();
    fs::create_dir_all(state.join("osv-scanner/npm")).unwrap();
    fs::write(state.join("osv-scanner/npm/all.zip"), stored_zip(&entries)).unwrap();
    fs::write(state.join("osv-scanner-fetched"), "").unwrap();
    fs::create_dir_all(&repo).unwrap();
    run(&repo, &["init", "-q", "-b", "main"]);
    fs::write(repo.join("bun.lock"), BUN_BEFORE).unwrap();
    let main = commit(&repo, "main");
    run(&repo, &["checkout", "-q", "-b", "lane"]);
    fs::write(repo.join("bun.lock"), format!("{{\"lockfileVersion\":1,\"workspaces\":{{\"\":{{\"name\":\"fixture\"}}}},\"packages\":{{\"{name}\":[\"{name}@{version}\",\"\",{{}},\"\"]}}}}")).unwrap();
    commit(&repo, "lane locks an npm package");
    (dir, main)
}

#[test]
fn a_lane_locking_an_npm_version_with_an_open_advisory_is_refused() {
    let (dir, main) = lane_locking_npm("fixture-npm", "1.0.0", &[("GHSA-fixture", "1.0.1")]);
    let refused = super::refuse::refuse_lane_gates(&dir.path().join("repo"), &main, "HEAD", &dir.path().join("state"), None, &Published { versions: &["1.0.0"], days_ago: 30 }).unwrap();
    let (gates, detail) = refused.unwrap_err();
    assert!(gates.contains("dependency advisory") && !gates.contains("dependency age"), "{gates}: {detail}");
    assert!(detail.contains("bun.lock: fixture-npm@1.0.0: GHSA-fixture"), "{detail}");
}

fn without_south(dir: &Path) -> Denylist {
    Denylist { south: dir.join("absent-south"), south_named: false, config: None, kinds: None }
}

/// The land-ticket model `north check machines` explores: one ticket through every ordering, then a second ticket
/// starting at every situation the withdrawal check admits. The assertion is here so
/// `cargo nextest run -p north-lander -E 'test(a_withdrawn_lane_is_refused_with_the_withdrawal_reason_when_its_session_queues_the_same_head_until_the_withdrawer_clears_it_or_the_head_changes)'`
/// replays the model, and an enqueue that stops consulting the withdrawal fails it.
fn land_ticket_model_is_sound() {
    use north_base::land_ticket::{LandTicket, Phase};
    use north_machine_model::{Promises, Together, explore_together};
    let together = Together {
        alone: &[Phase::Withdrawn],
        admits: |other| match other {
            Phase::Withdrawn => withdrawn_head_admitted(),
            _ => true,
        },
        race: || 1,
    };
    let model = explore_together::<LandTicket>(&Promises { ends: &[Phase::Landed, Phase::Replaced, Phase::Withdrawn], repeats: &[Phase::Queued] }, &together).expect("the land-ticket search runs");
    assert_eq!(model.instances, 2, "the withdrawal check is checked between two tickets");
    let broken: Vec<String> = model.findings.iter().map(|finding| format!("{}: {}", model.machine, finding.line())).collect();
    assert!(broken.is_empty(), "{}", broken.join("; "));
}

/// Whether the withdrawal check admits a second ticket at a head a session left withdrawn, which the model's second
/// instance asks before it starts.
fn withdrawn_head_admitted() -> bool {
    let Ok(root) = tempfile::tempdir() else { return false };
    let queue = root.path().join("queue");
    let withdrawn = queue.join("withdrawn");
    if fs::create_dir_all(&withdrawn).is_err() {
        return false;
    }
    let ticket = json!({
        "root": root.path().to_string_lossy(),
        "branch": "lane",
        "head": "abc",
        "session": "session",
        "verdict": "withdrawn by Tom: the callers were not audited",
        "queued_micros": 1
    });
    if fs::write(withdrawn.join("1-ticket.json"), ticket.to_string()).is_err() {
        return false;
    }
    north_land_scans::withdrawal::refuse_requeue(&queue, root.path(), "lane", "abc", Some("session")).is_ok()
}

#[test]
fn a_hand_in_whose_head_is_already_on_main_is_refused_before_the_queue_naming_its_uncommitted_paths_while_a_lane_behind_main_with_its_own_commit_still_queues() {
    let lane = Lane::new();
    run(&lane.repo, &["switch", "-q", "-c", "elsewhere", &lane.base]);
    run(&lane.repo, &["commit", "-q", "--allow-empty", "-m", "another lane lands"]);
    run(&lane.repo, &["push", "-q", "origin", "elsewhere:main"]);
    run(&lane.repo, &["switch", "-q", "lane"]);
    assert!(lane.try_enqueue(super::Shape::OneCommit { message: None }).is_ok(), "a lane behind main with a commit of its own must still queue");

    let lane = Lane::new();
    run(&lane.repo, &["reset", "-q", "--hard", &lane.base]);
    fs::write(lane.repo.join("content.txt"), "base\nthe change never committed\n").unwrap();
    fs::write(lane.repo.join("new.txt"), "untracked\n").unwrap();
    let refused = lane.try_enqueue(super::Shape::OneCommit { message: None }).err().unwrap_or_default();
    assert!(refused.starts_with("nothing to land:") && refused.contains("content.txt, new.txt"), "on 2026-11-12 a hand-in from a lane at an old main commit with its change uncommitted was answered landed with main's head: {refused}");
}
