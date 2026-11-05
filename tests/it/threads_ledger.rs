//! NorthThreads end to end through `north threads` over a scratch ledger and a fake gh: run closure,
//! recommend and calibrate, the native guard, parallel writers, the ledger host over a fake ssh, the
//! job queue and the routing comparison.
use regex::Regex;
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::fs;
use std::io::Write;
use north_core::foundation::shim;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const GH: &str = r#"printf '%s\n' "$*" >>"$GH_TRACE"
issue() { printf '{"number":%s,"title":"T%s","blockedBy":{"nodes":[%s]}}' "$1" "$1" "${2:-}"; }
blocker() { printf '{"number":%s,"state":"%s","repository":{"nameWithOwner":"tompassarelli/makgora"}}' "$1" "$2"; }
case "$*" in
  *name=makgora*number=3*) printf '{"data":{"repository":{"issue":{"number":3,"title":"T3","state":"OPEN","blockedBy":{"nodes":[]}}}}}\n'; exit ;;
esac
case "$*" in
  *name=makgora*) nodes="$(issue 1 "$(blocker 2 OPEN)"),$(issue 2),$(issue 3),$(issue 4 "$(blocker 5 CLOSED)")" ;;
  *) nodes="" ;;
esac
printf '{"data":{"repository":{"issues":{"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[%s]}}}}\n' "$nodes"
"#;

const SSH: &str = r#"[ -e "$SSH_DOWN" ] && exit 255
if [ -f "$HOME/.config/threads/ssh-config" ]; then
  [ "${1:-}" = -F ] && [ "${2:-}" = "$HOME/.config/threads/ssh-config" ] || exit 255
fi
printf '%s\n' "${@: -1}" >>"$SSH_TRACE"
THREADS_DB=$REMOTE_DB PATH=$REMOTE_BIN:$PATH exec bash -c "${@: -1}"
"#;

fn bash(path: &Path, body: &str) {
    shim::write_executable(path, format!("#!/usr/bin/env bash\n{body}"));
}

struct Rig {
    _scratch: tempfile::TempDir,
    root: PathBuf,
}

impl Rig {
    fn new() -> Self {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path().to_path_buf();
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::create_dir_all(root.join("home")).unwrap();
        bash(&root.join("bin/gh"), GH);
        fs::write(root.join("orchestration.toml"), "").unwrap();
        Rig { _scratch: scratch, root }
    }

    fn db(&self) -> PathBuf {
        self.root.join("threads.db")
    }

    fn command(&self, program: &str, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_north"));
        command
            .arg(program)
            .args(args)
            .env("HOME", self.root.join("home"))
            .env("THREADS_DB", self.db())
            .env("GH_TRACE", self.root.join("gh.trace"))
            .env("AGENTS_ORCHESTRATION", self.root.join("orchestration.toml"))
            .env("NORTH_BIN", env!("CARGO_BIN_EXE_north"))
            .env("NORTH_HOME", env!("CARGO_MANIFEST_DIR"))
            .env("THREADS_ALIVE_OWNERS", r#"["alice","bob","fixture"]"#)
            .env("PATH", format!("{}:{}", self.root.join("bin").display(), std::env::var("PATH").unwrap_or_default()))
            .env_remove("THREADS_NO_NATIVE")
            .env_remove("CLAUDE_CODE_REMOTE")
            .env_remove("THREADS_ACTOR")
            .env_remove("XDG_STATE_HOME");
        command
    }

    fn feed(mut command: Command, stdin: &str) -> (i32, String, String) {
        let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
        let output = child.wait_with_output().unwrap();
        (output.status.code().unwrap_or(-1), String::from_utf8_lossy(&output.stdout).into_owned(), String::from_utf8_lossy(&output.stderr).into_owned())
    }

    fn threads(&self, args: &[&str]) -> (i32, String, String) {
        Self::feed(self.command("threads", args), "")
    }

    /// stdout then stderr, as `2>&1` collects a quick command's output.
    fn t(&self, args: &[&str]) -> String {
        let (_, out, err) = self.threads(args);
        out + &err
    }

    fn ingest(&self, rows: &[Value]) {
        let batch: String = rows.iter().map(|row| format!("{row}\n")).collect();
        let (code, _, err) = Self::feed(self.command("threads", &["ingest"]), &batch);
        assert_eq!(code, 0, "{err}");
    }

    fn sql(&self, query: &str) -> String {
        let db = Connection::open(self.db()).unwrap();
        db.query_row(query, [], |row| row.get::<_, rusqlite::types::Value>(0)).map(|value| match value {
            rusqlite::types::Value::Integer(n) => n.to_string(),
            rusqlite::types::Value::Text(text) => text,
            rusqlite::types::Value::Null => String::new(),
            other => format!("{other:?}"),
        }).unwrap()
    }
}

fn has(text: &str, pattern: &str) -> bool {
    Regex::new(&format!("(?m){pattern}")).unwrap().is_match(text)
}

fn line(text: &str, wanted: &str) -> bool {
    text.lines().any(|line| line == wanted)
}

/// `count` passing then `failing` failing first-assignment runs of a tier under a brief.
fn runs(tier: &str, brief: &str, passing: usize, failing: usize, serial: &mut usize) -> Vec<Value> {
    let mut rows = Vec::new();
    for (outcome, landed, count) in [("done", 1, passing), ("not done", 0, failing)] {
        for _ in 0..count {
            *serial += 1;
            rows.push(json!({"id": format!("{tier}-{serial}"), "ended": "2026-10-09T00:00:00Z", "tier": tier, "actual_min": 5, "outcome": outcome, "landed": landed, "brief": brief}));
        }
    }
    rows
}

#[test]
fn ingested_runs_give_handoffs_closure_recommendations_and_hysteresis() {
    let rig = Rig::new();
    let mut serial = 0;
    rig.t(&["claim", "makgora#3", "--by", "alice"]);
    rig.t(&["release", "makgora#3", "--by", "alice", "--to", "bob"]);
    let brief = "Goal: x\nItem: tompassarelli/makgora#3. Follows: a111. Category: Tooling. Arm: escalate.\nETA 45 min.";
    rig.ingest(&[
        json!({"id": "a111", "ended": "2026-10-08T01:00:00Z", "tier": "high", "actual_min": 30, "eta_min": 20, "tokens": 1000, "outcome": "handoff", "note": "~/h/a111.md", "brief": "Item: makgora#3"}),
        json!({"id": "a222", "ended": "2026-10-08T02:00:00Z", "tier": "xhigh", "actual_min": 50, "eta_min": 45, "tokens": 2000, "outcome": "done", "brief": brief}),
    ]);
    let out = rig.t(&["show", "makgora#3"]) + &rig.t(&["summary"]);
    assert!(out.contains("a222 xhigh follows a111  50m/45m ETA  done") && out.contains("handoff  a111 -> a222") && out.contains("note ~/h/a111.md") && has(&out, r"^tooling +xhigh +escalate +1 +2.0 +1 +100%"), "runs take Item, Follows, Category and Arm from the brief; a new agent is a change of hands:\n{out}");

    rig.ingest(&[
        json!({"id": "b1", "ended": "2026-10-08T04:00:00Z", "tier": "medium", "actual_min": 10, "tokens": 3000, "outcome": "not done", "brief": "Item: makgora#5 Category: bug-known-cause"}),
        json!({"id": "b2", "ended": "2026-10-08T05:00:00Z", "tier": "high", "actual_min": 10, "tokens": 5000, "outcome": "done", "brief": "Item: makgora#5 Follows: b1 Category: frobnicate"}),
    ]);
    let out = rig.t(&["summary"]);
    assert!(has(&out, r"^unknown +high +1 "), "a category outside the fixed list counts as unknown:\n{out}");
    assert!(has(&out, r"^bug-known-cause +medium +1 +1 +2.0 +8k +100%"), "closure groups by the first run:\n{out}");

    rig.ingest(&[
        json!({"id": "s0", "ended": "2026-10-08T06:00:00Z", "tier": "medium", "actual_min": 10, "outcome": "split", "brief": "Category: feature. Arm: split."}),
        json!({"id": "s1", "ended": "2026-10-08T06:20:00Z", "tier": "medium", "actual_min": 15, "outcome": "done", "brief": "Follows: s0 Category: feature"}),
        json!({"id": "s2", "ended": "2026-10-08T06:30:00Z", "tier": "medium", "actual_min": 20, "outcome": "done", "brief": "Follows: s0 Category: feature"}),
        json!({"id": "s3", "ended": "2026-10-08T06:40:00Z", "tier": "medium", "actual_min": 25, "outcome": "not done", "brief": "Follows: s0 Category: feature"}),
    ]);
    let out = rig.t(&["summary"]);
    assert!(has(&out, r"^feature +1 +3 +67% +60 +40$") && has(&out, r"^feature +medium +split +3 +- +2 +66%") && !has(&out, r"^feature +medium +- "), "a Split run and its 3 parts show parts closure and an Arm: split row:\n{out}");

    let mut rows = runs("haiku", "Category: research. Spec: measured.", 5, 0, &mut serial);
    rows.extend(runs("medium", "Category: research. Spec: measured. Scope: one-file.", 2, 0, &mut serial));
    rig.ingest(&rows);
    let out = rig.t(&["recommend", "Category: research, Spec: measured, Scope: one-file"]) + &rig.t(&["summary", "--group", "category,spec,scope"]);
    assert!(line(&out, "tier claude:haiku  cell [Category: research, Spec: measured]  n 7") && line(&out, "success threshold 80%  rule rank (one tier passes)") && has(&out, r"^research +measured +one-file +medium +- +2 "), "recommend backs off to the finest cell with 5 runs:\n{out}");

    rig.ingest(&[json!({"id": "eta-coarse", "ended": "2026-10-09T00:00:00Z", "tier": "medium", "actual_min": 20, "outcome": "done", "brief": "Category: research. Spec: measured. Scope: cross-module."})]);
    let out = rig.t(&["recommend", "Category: research, Spec: measured, Scope: one-file"]);
    assert!(out.lines().any(|line| line.starts_with("eta wall 5m (p75 of n=5, cell Category: research, Spec: measured, tier claude:haiku, uncalibrated: no eta_pred calibration; too few runs with active minutes)")), "ETA p75 backs off to the five haiku runs:\n{out}");
    let out = rig.t(&["recommend", "Category: balance-tuning"]);
    assert!(line(&out, "success threshold 80%  rule workers-skill default for balance-tuning (no tier passes; no first-assignment tier has 15 priced runs)"), "a category with no runs gets the workers-skill default:\n{out}");

    let mut rows = runs("haiku", "Category: performance", 24, 6, &mut serial);
    rows.extend(runs("medium", "Category: performance", 26, 4, &mut serial));
    rig.ingest(&rows);
    let first = rig.t(&["recommend", "Category: performance"]).lines().next().unwrap_or_default().to_owned();
    rig.ingest(&runs("haiku", "Category: performance", 0, 6, &mut serial));
    let held = rig.t(&["recommend", "Category: performance"]).lines().next().unwrap_or_default().to_owned();
    rig.ingest(&runs("haiku", "Category: performance", 0, 6, &mut serial));
    let moved = rig.t(&["recommend", "Category: performance"]).lines().next().unwrap_or_default().to_owned();
    assert_eq!(
        [first.as_str(), held.as_str(), moved.as_str()],
        ["tier claude:haiku  cell [Category: performance]  n 60", "tier claude:haiku  cell [Category: performance]  n 66", "tier claude:opus-medium  cell [Category: performance]  n 72"],
        "hysteresis: haiku wins by rank, holds at 67% against medium 87%, and yields at 57%"
    );

    let native = |id: &str, tier: &str, landed: i64, expects: Option<i64>, brief: &str, ended: &str| {
        let mut row = json!({"id": id, "ended": ended, "tier": tier, "actual_min": 5, "outcome": "done", "landed": landed, "brief": brief});
        if let Some(expects) = expects {
            row["expects_landing"] = json!(expects);
        }
        row
    };
    rig.ingest(&[
        native("n1", "haiku", 0, Some(0), "Category: native-check", "2026-10-09T00:00:00Z"),
        native("n2", "haiku", 0, Some(0), "Category: native-check", "2026-10-09T00:00:00Z"),
        native("n3", "haiku", 0, Some(0), "Category: native-check", "2026-10-09T00:00:00Z"),
        native("n4", "haiku", 0, Some(1), "Category: native-check", "2026-10-09T00:00:00Z"),
        native("n5", "haiku", 1, Some(1), "Category: native-check", "2026-10-09T00:00:00Z"),
        native("n6", "medium", 1, None, "Category: native-check Follows: n5", "2026-10-09T01:00:00Z"),
    ]);
    let out = rig.t(&["recommend", "Category: native-check"]);
    assert!(line(&out, "  claude:haiku       60% of 5  shrunk 60%  $ n/a (priced n=0)"), "success is Done with no later Follows: run, and landed only when the run committed:\n{out}");
    rig.ingest(&[
        json!({"id": "u1", "ended": "2026-10-09T00:00:00Z", "tier": "haiku", "actual_min": 5, "outcome": "unclear", "brief": "Category: native-check"}),
        json!({"id": "u2", "ended": "2026-10-09T00:00:00Z", "tier": "haiku", "actual_min": 5, "outcome": "unclear", "brief": "Item: makgora#7 Category: native-check"}),
    ]);
    let out = rig.t(&["recommend", "Category: native-check"]) + &rig.t(&["summary"]);
    assert!(line(&out, "  claude:haiku       50% of 6  shrunk 50%  $ n/a (priced n=0)") && has(&out, r"^native-check +haiku +- +6 +1.0 +5 +83% +50%"), "an unclear run with no Item is unjudged; a briefed unclear run is a failure:\n{out}");
}

#[test]
fn worker_ledger_recommend_keeps_split_output_and_codex_only_uses_the_codex_cell() {
    let rig = Rig::new();
    let config = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("harness/lib/fixtures/orchestration.toml")).unwrap();
    rig.ingest(&[]);
    let db = Connection::open(rig.db()).unwrap();
    for (tier, minutes) in [("haiku", 5), ("gpt-6.1-sol medium", 11)] {
        for index in 0..5 {
            db.execute(
                "INSERT INTO runs(agent,tier,category,ended,minutes,outcome,spec,scope,surface,verify) VALUES (?1,?2,'bug-known-cause','2026-10-09T00:00:00Z',?3,'done','exact','module','shell','local-test')",
                params![format!("{tier}-{index}"), tier, minutes + index],
            ).unwrap();
        }
    }
    drop(db);
    let facets = "Category: bug-known-cause, Spec: exact, Scope: module, Surface: shell, Verify: local-test";
    let recommend = |mode: &str, facets: &str| {
        fs::write(rig.root.join("orchestration.toml"), config.replacen("mode = \"split\"", &format!("mode = \"{mode}\""), 1)).unwrap();
        let (code, out, err) = Rig::feed(rig.command("worker-ledger", &["--recommend", facets]), "");
        assert_eq!(code, 0, "{err}");
        out
    };
    let split = recommend("split", facets);
    let expected = "tier claude:haiku  cell [Category: bug-known-cause, Spec: exact, Scope: module, Verify: local-test, Surface: shell]  n 10
success threshold 80%  rule rank (uncalibrated: thin $ claude:haiku n=0, codex:sol-medium n=0 < 15)
success claude:haiku 100% shrunk (n=5, 5 ok, toward the category's pooled 100% with a prior of 15 runs)
  claude:haiku       100% of 5  shrunk 100%  $ n/a (priced n=0)
  codex:sol-medium   100% of 5  shrunk 100%  $ n/a (priced n=0)
eta wall 8m (p75 of n=5, cell Category: bug-known-cause, Spec: exact, Scope: module, Verify: local-test, Surface: shell, tier claude:haiku, uncalibrated: no eta_pred calibration; too few runs with active minutes)  $ n/a (uncalibrated: no usd calibration)
E n/a (fewer than 5 priced runs of tier claude:haiku in any cell; fallback: no dollar figure, the workers-skill default tier stands)
";
    assert_eq!(split, expected);
    assert_eq!(recommend("claude-only", facets), split);
    let codex = recommend("codex-only", facets);
    assert!(!codex.contains("claude:") && codex.starts_with("tier codex:sol-medium ") && codex.contains("eta 13m (actual_med of n=5, cell Category: bug-known-cause, Spec: exact, Scope: module, Verify: local-test, Surface: shell, tier codex:sol-medium)"), "{codex}");
    let none = recommend("codex-only", "Category: balance-tuning");
    assert!(none.starts_with("tier claude:opus-medium  cell [Category: balance-tuning]  n 0\n") && none.contains("rule workers-skill default for balance-tuning (no tier passes; no first-assignment tier has 15 priced runs)\n") && none.contains("unmeasured in this cell (n=0)\n"), "{none}");
}

/// A seeded generator for the drifting calibration fixture: uniform in [0, 1) and log-normal draws.
struct Draws(u64);

impl Draws {
    fn uniform(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) >> 11) as f64 / (1_u64 << 53) as f64
    }

    fn lognormal(&mut self, sigma: f64) -> f64 {
        let (u, v) = (1.0 - self.uniform(), self.uniform());
        (sigma * (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()).exp()
    }
}

#[test]
fn a_seeded_drifting_fixture_falls_back_on_thin_data_and_calibrates_each_kind_with_quartiles() {
    let rig = Rig::new();
    let mut draws = Draws(35);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    let rows: Vec<Value> = (0..720).map(|index| {
        let progress = f64::from(index) / 720.0;
        let minutes = (30.0 * 2f64.powf(-1.5 * progress) * draws.lognormal(0.5)).round().max(1.0) as i64;
        let ended = now - ((30.0 - f64::from(index) / 24.0) * 86_400.0) as i64 - 7_200;
        let ended = north_core::foundation::clock::Timestamp::from_unix_seconds(ended).iso();
        let outcome = if draws.uniform() < 0.85 { "done" } else { "not done" };
        json!({"id": format!("c{index}"), "ended": ended, "tier": "medium", "actual_min": minutes, "eta_min": 30, "outcome": outcome, "brief": "Category: mechanical. Spec: measured."})
    }).collect();
    rig.ingest(&rows);
    Connection::open(rig.db()).unwrap().execute("UPDATE runs SET cost_usd = minutes * 0.04", []).unwrap();
    rig.t(&["calibrate", "--since", "1h", "--write"]);
    let thin = rig.t(&["recommend", "Category: mechanical, Spec: measured"]);
    assert!(thin.contains("uncalibrated: eta_pred all n=0 < 100"), "{thin}");
    let (code, out, err) = rig.threads(&["calibrate", "--since", "7d", "--json", "--write"]);
    assert_eq!(code, 0, "{err}");
    let doc: Value = serde_json::from_str(&out).unwrap();
    for kind in ["eta", "eta_pred", "usd"] {
        let row = doc["rows"].as_array().unwrap().iter().find(|row| row["kind"] == kind && row["cell"] == "all").unwrap_or_else(|| panic!("no {kind} all row: {out}"));
        assert!(row["n"].as_i64().unwrap() > 0 && ["p25", "median", "p75"].iter().all(|key| !row[key].is_null()), "{row}");
    }
    let good = rig.t(&["recommend", "Category: mechanical, Spec: measured"]);
    assert!(has(&good, r" eta_pred claude n=\d+ ") && !good.contains("uncalibrated") && good.contains('$'), "{good}");
}

#[test]
fn native_refs_and_titles_never_reach_the_gh_argv() {
    let rig = Rig::new();
    rig.ingest(&[]);
    let db = Connection::open(rig.db()).unwrap();
    let mut draws = Draws(7);
    let namespaces = ["life", "accounts", "travel", "ops"];
    let mut items = Vec::new();
    for index in 0..1000 {
        let namespace = namespaces[(draws.uniform() * 4.0) as usize];
        let word = |draws: &mut Draws| (0..3 + (draws.uniform() * 7.0) as usize).map(|_| (b'a' + (draws.uniform() * 26.0) as u8) as char).collect::<String>();
        let title = (0..3 + (draws.uniform() * 4.0) as usize).map(|_| word(&mut draws)).collect::<Vec<_>>().join(" ");
        db.execute("INSERT INTO items(ns, number, title, body, created) VALUES (?1, ?2, ?3, '', '2026-11-05T00:00:00Z')", params![namespace, 1000 + index, title]).unwrap();
        items.push((format!("{namespace}#{}", 1000 + index), title));
    }
    drop(db);
    for (reference, _) in items.iter().step_by(20) {
        assert_eq!(rig.threads(&["show", reference]).0, 0, "show {reference}");
        assert_eq!(rig.threads(&["need", reference, "makgora#3"]).0, 0, "need {reference}");
    }
    assert_eq!(rig.threads(&["ready"]).0, 0);
    let trace = fs::read_to_string(rig.root.join("gh.trace")).unwrap_or_default();
    let leaked: Vec<_> = items.iter().filter(|(reference, title)| trace.contains(reference.as_str()) || trace.contains(title.as_str())).collect();
    assert!(leaked.is_empty(), "{leaked:?}");
}

#[test]
fn parallel_writers_keep_every_claim() {
    let rig = Rig::new();
    rig.ingest(&[]);
    let (writers, claims) = (8, 25);
    std::thread::scope(|scope| {
        for writer in 1..=writers {
            let rig = &rig;
            scope.spawn(move || {
                for claim in 0..claims {
                    let item = format!("wisp#{}", writer * 1000 + claim);
                    let (code, _, err) = rig.threads(&["claim", &item, "--by", &format!("w{writer}")]);
                    assert_eq!(code, 0, "{item}: {err}");
                }
            });
        }
    });
    assert_eq!(rig.sql("select count(*) from claims where item like 'wisp#%'"), (writers * claims).to_string());
}

struct Remote<'a> {
    rig: &'a Rig,
    home: PathBuf,
}

impl Remote<'_> {
    fn new(rig: &Rig) -> Remote<'_> {
        let home = rig.root.join("remote-home");
        fs::create_dir_all(home.join(".config/threads")).unwrap();
        fs::write(home.join(".config/threads/remote"), "nexus\n").unwrap();
        fs::create_dir_all(rig.root.join("remote")).unwrap();
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_north"), rig.root.join("remote/north")).unwrap();
        bash(&rig.root.join("bin/ssh"), SSH);
        Remote { rig, home }
    }

    fn run(&self, args: &[&str], stdin: &str) -> (i32, String) {
        let mut command = self.rig.command("threads", args);
        command
            .env_remove("THREADS_DB")
            .env("HOME", &self.home)
            .env("SSH_DOWN", self.rig.root.join("down"))
            .env("SSH_TRACE", self.rig.root.join("ssh.trace"))
            .env("REMOTE_DB", self.rig.root.join("remote.db"))
            .env("REMOTE_BIN", self.rig.root.join("remote"));
        let (code, out, err) = Rig::feed(command, stdin);
        (code, out + &err)
    }

    fn spooled(&self) -> usize {
        fs::read_dir(self.home.join(".local/state/threads/spool")).map_or(0, |entries| entries.filter_map(Result::ok).filter(|entry| !entry.file_name().to_string_lossy().starts_with('.')).count())
    }

    fn refreshes(&self) -> usize {
        fs::read_to_string(self.rig.root.join("ssh.trace")).unwrap_or_default().lines().filter(|line| *line == "north threads snapshot-pages").count()
    }
}

#[test]
fn the_ledger_host_over_ssh_spools_while_down_serves_a_stale_snapshot_and_flushes_in_order() {
    let rig = Rig::new();
    let remote = Remote::new(&rig);
    fs::write(rig.root.join("down"), "").unwrap();
    for id in ["s1", "s2"] {
        let row = json!({"id": id, "ended": "2026-10-09T01:00:00Z", "tier": "medium", "actual_min": 5, "outcome": "done", "brief": "Category: tooling"});
        assert_eq!(remote.run(&["ingest"], &format!("{row}\n")).0, 0);
    }
    let (status, _) = remote.run(&["claim", "wisp#9", "--by", "alice"], "");
    assert_eq!((status, remote.spooled()), (255, 2), "a claim fails while the host is down and both batches wait in the spool");
    fs::remove_file(rig.root.join("down")).unwrap();
    assert_eq!(remote.run(&["claim", "wisp#7", "--by", "bob"], "").0, 0);
    assert_eq!(remote.spooled(), 0);
    fs::write(rig.root.join("ssh.trace"), "").unwrap();
    let goal = remote.run(&["calibrate", "--write", "--goal-row", r#"{"kind":"goal_eta","cell":"wisp","n":1,"value":5,"source":"fixture"}"#], "");
    assert_eq!((goal.0, remote.refreshes()), (0, 0), "a goal row writes without refreshing the snapshot: {}", goal.1);
    assert_eq!(remote.run(&["calibrate", "--write"], "").0, 0);
    assert_eq!(remote.refreshes(), 1, "other calibration writes refresh it");
    fs::write(rig.root.join("down"), "").unwrap();
    let (status, out) = remote.run(&["list"], "");
    assert!(status == 0 && has(&out, "^STALE: ledger snapshot from .*; nexus unreachable") && has(&out, r"^wisp#7 +bob"), "{out}");
    let order: String = Connection::open(rig.root.join("remote.db")).unwrap().query_row("select group_concat(agent) from (select agent from runs order by rowid)", [], |row| row.get(0)).unwrap();
    assert_eq!(order, "s1,s2");

    fs::remove_file(rig.root.join("down")).unwrap();
    fs::write(remote.home.join(".config/threads/ssh-config"), "Host nexus\n").unwrap();
    assert_eq!(remote.run(&["claim", "wisp#8", "--by", "configured-namespace-transport"], "").0, 0);
    assert_eq!(remote.run(&["calibrate", "--write"], "").0, 0);
    let held: i64 = Connection::open(rig.root.join("remote.db")).unwrap().query_row("select count(*) from claims where item='wisp#8' and owner='configured-namespace-transport'", [], |row| row.get(0)).unwrap();
    assert_eq!(held, 1, "the configured SSH namespace route keeps canonical writes");
}

#[test]
fn a_requeued_job_goes_back_ahead_of_later_jobs() {
    let rig = Rig::new();
    let add = |word: &str| rig.threads(&["queue", "add", "reqq", "-", "--", "echo", word]).1.trim().to_owned();
    let (first, second) = (add("a"), add("b"));
    rig.t(&["queue", "take", "reqq"]);
    rig.t(&["queue", "msg", &first, "requeued", "rc=143"]);
    let took: Value = serde_json::from_str(&rig.threads(&["queue", "take", "reqq"]).1).unwrap();
    assert_eq!(took["id"].to_string(), first);
    assert_eq!(rig.sql(&format!("select state from job_queue where id = {second}")), "queued");
}

const ARMS: [(&str, &str, &str, &str); 3] = [
    ("baseline", "2026-10-05T12:27:27Z", "2026-10-12T12:27:27Z", "2026-10-10T00:00:00Z"),
    ("codex-only", "2026-10-12T12:27:27Z", "2026-10-15T02:00Z", "2026-10-13T00:00:00Z"),
    ("split-after", "2026-10-15T02:00Z", "2026-10-15T16:00Z", "2026-10-15T02:00:00Z"),
];

fn later(base: &str, minutes: i64) -> String {
    let base = north_core::foundation::clock::Timestamp::parse_loose(base).unwrap();
    north_core::foundation::clock::Timestamp::from_unix_seconds(base.unix_seconds() + minutes * 60).iso()
}

#[allow(clippy::too_many_arguments)]
fn routed(db: &Connection, agent: &str, item: Option<&str>, category: &str, begin: &str, end: &str, landed: bool, sha: Option<String>, outcome: &str, follows: Option<String>) {
    db.execute(
        "INSERT INTO runs(agent,item,category,tier,account,staffed,started,ended,landed_at,landed,landed_sha,minutes,outcome,follows) VALUES(?1,?2,?3,'gpt-6.1-sol medium','codex',?4,?4,?5,?6,?7,?8,10,?9,?10)",
        params![agent, item, category, begin, end, landed.then_some(end), i64::from(landed), sha, outcome, follows],
    ).unwrap();
    db.execute("INSERT INTO run_costs VALUES('codex',?1,?2,?3,0.5)", params![begin, end, agent]).unwrap();
}

fn compare(rig: &Rig, operator: &Path) -> String {
    let before = fs::read(rig.db()).unwrap();
    let mut command = rig.command("threads", &["routing-compare"]);
    command.env("AGENTS_OPERATOR_DIR", operator);
    let (code, out, err) = Rig::feed(command, "");
    assert_eq!(code, 0, "{err}");
    assert!(fs::read(rig.db()).unwrap() == before, "the readout wrote to its ledger");
    out
}

fn arm_rows<'a>(out: &'a str, arm: &str) -> Vec<Vec<&'a str>> {
    let gap = Regex::new(r"\s{2,}").unwrap();
    out.lines().filter(|line| line.starts_with(&format!("{arm} "))).map(|line| gap.split(line.trim()).collect()).collect()
}

fn landed_items(db: &Connection, start: &str, end: &str) -> i64 {
    db.query_row("SELECT count(DISTINCT item) FROM runs WHERE role='worker' AND landed=1 AND item IS NOT NULL AND julianday(coalesce(landed_at,ended)) >= julianday(?1) AND julianday(coalesce(landed_at,ended)) < julianday(?2)", params![start, end], |row| row.get(0)).unwrap()
}

#[test]
fn routing_compare_matches_direct_sql_landed_counts_wilson_rates_and_points_without_writing_its_ledger() {
    let rig = Rig::new();
    rig.ingest(&[]);
    let operator = rig.root.join("operator");
    fs::create_dir_all(&operator).unwrap();
    fs::write(operator.join("costs.toml"), "[[service]]\nid=\"codex\"\nkind=\"ai-plan\"\nusd_per_week=50.0\n").unwrap();
    let db = Connection::open(rig.db()).unwrap();
    for (number, (arm, _, _, base)) in ARMS.iter().enumerate() {
        let number = number as i64;
        let item = |offset: i64| format!("north#{}", 100 + number * 10 + offset);
        for index in 0..7 {
            let category = if index < 5 { "feature" } else { "tooling" };
            let follows = (index == 0).then(|| format!("{arm}-retry"));
            routed(&db, &format!("{arm}-{index}"), Some(&item(index)), category, base, &later(base, 60 + index), true, Some(format!("{:040x}", number * 10 + index + 1)), "done", follows);
        }
        routed(&db, &format!("{arm}-retry"), Some(&item(0)), "feature", base, &later(base, 20), false, None, "not done", None);
        routed(&db, &format!("{arm}-duplicate"), Some(&item(1)), "feature", base, &later(base, 62), true, Some(format!("{:040x}", number * 10 + 2)), "done", None);
        routed(&db, &format!("{arm}-future"), Some(&item(2)), "feature", &later(base, 1), &later(base, 120), false, None, "not done", None);
        routed(&db, &format!("{arm}-itemless"), None, "unknown", base, &later(base, 30), true, None, "done", None);
        routed(&db, &format!("{arm}-unlanded"), Some(&item(9)), "feature", base, &later(base, 30), false, None, "done", None);
    }
    routed(&db, "outside", Some("north#999"), "feature", "2026-10-15T16:00:00Z", "2026-10-15T16:00:00Z", true, Some("f".repeat(40)), "done", None);
    db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)").unwrap();
    let out = compare(&rig, &operator);
    for (arm, start, end, _) in ARMS {
        let rows = arm_rows(&out, arm);
        let find = |label: &str| rows.iter().find(|row| row[1] == label && row.len() == 9).unwrap_or_else(|| panic!("{arm} {label}: {out}")).clone();
        assert_eq!((find("overall")[2].parse::<i64>().unwrap(), landed_items(&db, start, end)), (7, 7), "{arm}");
        let feature = find("feature");
        assert_eq!((feature[2], feature[5], &feature[7..]), ("5", "80.00% [37.55,96.38] (4/5)", &["n/a (n=0)", "0/5"][..]), "{arm}");
        let tooling = find("tooling");
        assert_eq!((tooling[2], tooling[5]), ("2", "n/a (n=2)"), "{arm}");
        let points = rows.iter().find(|row| row[1..3] == ["feature", "codex"]).unwrap_or_else(|| panic!("{arm} points: {out}"));
        assert!(points[3..5] == ["5", "5"] && points[5].starts_with("1.40 ["), "{points:?}");
        assert!(out.contains(&format!("no Item runs {arm} n=1 landed=1")), "{out}");
    }
    for (number, (arm, _, _, base)) in ARMS.iter().enumerate() {
        let item = |offset: usize| format!("north#{}", 100 + number * 10 + offset);
        routed(&db, &format!("{arm}-unclear-failed"), Some(&item(0)), "feature", base, &later(base, 10), false, None, "unclear", None);
        routed(&db, &format!("{arm}-unclear-escalated"), Some(&item(5)), "tooling", base, &later(base, 10), false, None, "unclear", None);
        routed(&db, &format!("{arm}-escalated"), Some(&item(5)), "tooling", base, &later(base, 20), false, None, "escalated", None);
        routed(&db, &format!("{arm}-unclear"), Some(&item(6)), "tooling", base, &later(base, 10), false, None, "unclear", None);
    }
    db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)").unwrap();
    let quality = compare(&rig, &operator);
    for (arm, start, end, _) in ARMS {
        let rows = arm_rows(&quality, arm);
        let find = |label: &str| rows.iter().find(|row| row[1] == label && row.len() == 9).unwrap_or_else(|| panic!("{arm} {label}: {quality}")).clone();
        let overall = find("overall");
        assert_eq!((overall[2].parse::<i64>().unwrap(), landed_items(&db, start, end)), (7, 7), "{arm}");
        assert!(overall[5].ends_with("(4/6)") && overall[6].ends_with("(1/5)"), "{arm} {overall:?}");
        assert!(quality.contains(&format!("quality coverage {arm} overall first_try known=6/7 unknown=1/7 adverse known=5/7 unknown=2/7")), "{quality}");
        assert_eq!(find("feature")[5], "80.00% [37.55,96.38] (4/5)");
        assert_eq!(&find("tooling")[5..7], ["n/a (n=1)", "n/a (n=0)"]);
    }
}
