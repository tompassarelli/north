use super::*;
use crate::grounding::common::SQLITE_BUSY;
use crate::platform::sqlite;

const SESSIONS: &[(&str, &[(&str, &str)])] = &[
    ("proxy", &[
        ("user", "Read /h/.local/state/agents/handoffs/proxy-handoff-2026-10-13-c.md and take over as the proxy"),
        ("user", "its fine if we burn codex today, we have the windfall of 2 anthropic accounts.."),
        ("assistant", "Staffing now.\nDECIDED 06:20 codex lead takes the netcode box"),
        ("user", "Delegation: role=worker budget=0\nCategory: tooling. You must burn codex today."),
        ("user", "<task-notification>we need to land everything</task-notification>"),
        ("user", "you think all the songs should be 140bpm?"),
        ("user", "why did u do (Wallet) instead of (wallet)"),
        ("assistant", "Should I close waygate#1 with gh?"),
        ("user", "yes go ahead"),
        ("user", "I prefer anthropic to stay the dominant way: Claude first on every account, codex only for tightly specified work so we burn usage evenly."),
        ("user", "we need a new rule change that everyone has to respond within 2-5 minutes"),
        ("user", "we need a new rule change that everyone has to respond within 2-5 minutes"),
        ("user", "you keep answering questions without checking skills and transcripts"),
        ("user", "can we update policy durably to stop making this class of errors"),
        ("user", "Can you have a worker set up Gemini on my computer free tier if that's available?"),
        ("user", "Why do we keep re-discovering that we need to batch and queue issues?"),
        ("user", "what happened to the build, did you restart it?"),
        ("user", "why do you keep losing the battle.net connection?"),
    ]),
    ("lead", &[
        ("user", "Read /h/.local/state/agents/handoffs/smashcraft-lead-brief.md and follow it."),
        ("user", "you keep asking; we need to land everything now"),
    ]),
    ("oneshot", &[("user", "Do NOT regenerate the token. We need to keep the scope.")]),
];

fn q(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

struct Fixture {
    _root: tempfile::TempDir,
    paths: Paths,
    n: usize,
}

fn fixture() -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let db = root.path().join("index.db");
    let mut script = String::from(
        "CREATE TABLE files(id INTEGER PRIMARY KEY, path TEXT, provider TEXT, session_id TEXT, agent TEXT);\n\
         CREATE TABLE msg(id INTEGER PRIMARY KEY, file_id INTEGER, line INTEGER, ts TEXT, role TEXT, tool TEXT, body INTEGER, dup INTEGER NOT NULL DEFAULT 0);\n\
         CREATE TABLE body(id INTEGER PRIMARY KEY, text TEXT);\n",
    );
    let start = Timestamp::now().unix_millis() as f64 - 3.0 * 3_600_000.0;
    let mut n = 0;
    for (f, (name, messages)) in SESSIONS.iter().enumerate() {
        script += &format!("INSERT INTO files VALUES ({}, {}, 'anthropic', {}, NULL);\n", f + 1, q(&format!("/t/{name}.jsonl")), q(&format!("sess-{name}")));
        for (line, (role, text)) in messages.iter().enumerate() {
            n += 1;
            script += &format!("INSERT INTO body VALUES ({n}, {});\n", q(text));
            script += &format!("INSERT INTO msg VALUES ({n}, {}, {}, {}, {}, NULL, {n}, 0);\n", f + 1, line + 1, q(&Timestamp::from_unix_millis_f64(start + n as f64 * 60_000.0).iso_millis()), q(role));
        }
    }
    sqlite::execute(&db, SQLITE_BUSY, &script).unwrap();
    let handoffs = root.path().join("handoffs");
    fs::create_dir(&handoffs).unwrap();
    fs::write(handoffs.join("galileo-lead-status.md"), "# status\nDECIDED 10:49 GAL44: grounding is a lead-local choice\nAPPROVED proxy 14:52Z: range lands\nnotes DECIDED inline is not a record\n").unwrap();
    fs::write(handoffs.join("brief.md"), "DECIDED 01:00 not a status file\n").unwrap();
    let state = root.path().join("state");
    let paths = Paths { db, ledger: state.join("ledger.jsonl"), watermark: state.join("distill-state.json"), state, handoffs };
    Fixture { _root: root, paths, n }
}

impl Fixture {
    fn run(&self, since: Option<&str>) -> Value {
        let line = run(&self.paths, since, true, false).unwrap().unwrap();
        serde_json::from_str(&line).unwrap()
    }

    fn rows(&self) -> Vec<Value> {
        fs::read_to_string(&self.paths.ledger).unwrap().trim().split('\n').map(|l| serde_json::from_str(l).unwrap()).collect()
    }
}

#[test]
fn the_ledger_keeps_toms_decisions_links_an_override_takes_lead_decided_lines_and_adds_only_new_rows() {
    let fx = fixture();
    let first = fx.run(Some("1d"));
    let all = fx.rows();
    for r in &all {
        let mut keys: Vec<&String> = r.as_object().unwrap().keys().collect();
        keys.sort();
        assert_eq!(keys, ["at", "id", "kind", "quote", "source", "superseded_by", "topic"]);
    }
    let tom: HashMap<String, &Value> = all
        .iter()
        .filter(|r| r["source"].as_str().unwrap().starts_with("tom convo:"))
        .map(|r| (r["quote"].as_str().unwrap().to_string(), r))
        .collect();
    let prefer = "I prefer anthropic to stay the dominant way: Claude first on every account, codex only for tightly specified work so we burn usage evenly.";
    let burn = "its fine if we burn codex today, we have the windfall of 2 anthropic accounts..";
    let kinds: HashMap<&str, &str> = tom.iter().map(|(q, r)| (q.as_str(), r["kind"].as_str().unwrap())).collect();
    let expected: HashMap<&str, &str> = [
        (burn, "decision"),
        ("yes go ahead", "approval"),
        (prefer, "preference"),
        ("we need a new rule change that everyone has to respond within 2-5 minutes", "decision"),
        ("you keep answering questions without checking skills and transcripts", "correction"),
        ("can we update policy durably to stop making this class of errors", "decision"),
        ("Can you have a worker set up Gemini on my computer free tier if that's available?", "direction"),
        ("Why do we keep re-discovering that we need to batch and queue issues?", "correction"),
        ("why do you keep losing the battle.net connection?", "correction"),
    ]
    .into_iter()
    .collect();
    assert_eq!(kinds, expected);
    assert!(tom["yes go ahead"]["topic"].as_str().unwrap().contains("waygate"));

    assert_eq!(tom[burn]["superseded_by"], tom[prefer]["id"]);
    assert_eq!(tom.values().filter(|r| !r["superseded_by"].is_null()).count(), 1);

    let mut others: Vec<(String, String, String)> = all
        .iter()
        .filter(|r| !tom.contains_key(r["quote"].as_str().unwrap()))
        .map(|r| {
            let source = r["source"].as_str().unwrap();
            (source.split(' ').next().unwrap().to_string(), r["kind"].as_str().unwrap().to_string(), r["at"].as_str().unwrap()[11..16].to_string())
        })
        .collect();
    others.sort();
    let pairs: Vec<(&str, &str)> = others.iter().map(|(b, k, _)| (b.as_str(), k.as_str())).collect();
    assert_eq!(pairs, [("galileo-lead", "approval"), ("galileo-lead", "decision"), ("proxy", "decision")]);
    assert_eq!(others.iter().find(|(b, k, _)| b == "galileo-lead" && k == "decision").unwrap().2, "10:49");

    let n = fx.n + 1;
    sqlite::execute(&fx.paths.db, SQLITE_BUSY, &format!(
        "INSERT INTO body VALUES ({n}, 'from now on only give me times in my local time');\nINSERT INTO msg VALUES ({n}, 1, 99, {}, 'user', NULL, {n}, 0);",
        q(&Timestamp::now().iso_millis())
    ))
    .unwrap();
    let second = fx.run(None);
    assert_eq!(
        [first["added"].as_u64().unwrap(), second["added"].as_u64().unwrap(), second["total"].as_u64().unwrap()],
        [all.len() as u64, 1, all.len() as u64 + 1]
    );
    assert_eq!(fx.run(None)["added"], 0);
}

#[test]
fn distill_run_if_idle_exits_at_once_while_another_run_holds_the_lock() {
    let fx = fixture();
    fs::create_dir_all(fx.paths.state.join("distill.lock.d")).unwrap();
    let outcome = run(&fx.paths, None, true, true).unwrap();
    assert!(outcome.unwrap_err().contains("busy"));
}

#[test]
fn rid_and_iso_match_the_typescript_forms() {
    assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
    assert_eq!(rid("convo:sess:/t/a.jsonl:3"), format!("L{}", &sha1_hex(b"convo:sess:/t/a.jsonl:3")[..10]));
    assert_eq!(Timestamp::parse_loose("2026-10-15T10:52:55.752Z").unwrap().iso(), "2026-10-15T10:52:55Z");
    assert_eq!(Timestamp::parse_loose("2026-10-15T10:52:55.752Z").unwrap().iso_millis(), "2026-10-15T10:52:55.752Z");
}
