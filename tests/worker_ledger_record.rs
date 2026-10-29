use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use regex::Regex;
use serde_json::{Value, json};

const REPO: &str = env!("CARGO_MANIFEST_DIR");
const AGY: &str = "0a0a0a0a-0000-4000-8000-000000000001";
const GH: &str = "#!/bin/sh\nif [ \"$1\" = issue ]; then printf '%s\\n' '[{\"number\":446,\"labels\":[{\"name\":\"theme:balance\"}]}]'; exit; fi\nprintf '%s\\n' '{\"data\":{\"repository\":{\"issues\":{\"pageInfo\":{\"hasNextPage\":false,\"endCursor\":null},\"nodes\":[]}}}}'\n";

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn jsonl(path: &Path, rows: &[Value]) {
    write(path, &rows.iter().map(|row| format!("{row}\n")).collect::<String>());
}

fn script(path: &Path, body: &str) {
    write(path, body);
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn sqlite(db: &Path, sql: &str) -> String {
    let output = Command::new("sqlite3").arg(db).arg(sql).output().unwrap();
    assert!(output.status.success(), "{sql}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn fixture_rows(name: &str) -> Vec<Value> {
    let text = fs::read_to_string(Path::new(REPO).join("tests/fixtures").join(name)).unwrap();
    text.lines().map(|line| serde_json::from_str(line).unwrap()).collect()
}

struct Rig {
    dir: tempfile::TempDir,
    orchestration: PathBuf,
}

impl Rig {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let rig = Self { orchestration: Path::new(REPO).join("harness/lib/fixtures/orchestration.toml"), dir };
        script(
            &rig.path("bin/north"),
            &format!("#!/bin/sh\n[ \"$1 $2 $3\" = \"fleet list --json\" ] || exec {} \"$@\"\ncat \"$FLEET_TEST_SESSIONS\"\n", env!("CARGO_BIN_EXE_north")),
        );
        script(&rig.path("bin/gh"), GH);
        fs::create_dir_all(rig.path("home")).unwrap();
        fs::copy(Path::new(REPO).join("harness/lib/fixtures/worker-ledger-fleet.json"), rig.path("fleet-sessions.json")).unwrap();
        rig.sql("create table usage(ts,provider,account,kind,name,used_pct,resets_at,window_min); create table model_events(model,kind,at)");
        rig
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn sql(&self, query: &str) -> String {
        sqlite(&self.path("threads.db"), query)
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_north"));
        command
            .arg("worker-ledger")
            .env("HOME", self.path("home"))
            .env("XDG_STATE_HOME", self.path("home/.local/state"))
            .env("THREADS_DB", self.path("threads.db"))
            .env("CLAUDE_CONFIG_DIR", self.path("claude"))
            .env("CODEX_CONFIG_DIR", self.path("codex"))
            .env("AGY_CONFIG_DIR", self.path("agy"))
            .env("FLEET_TEST_SESSIONS", self.path("fleet-sessions.json"))
            .env("AGENTS_ORCHESTRATION", &self.orchestration)
            .env("AGENTS_OPERATOR_DIR", Path::new(REPO).join("harness/lib/fixtures"))
            .env("LEDGER_REPOS", self.path("repo"))
            .env("NORTH_BIN", self.path("bin/north"))
            .env("NORTH_HOME", REPO)
            .env("PATH", format!("{}:{}", self.path("bin").display(), std::env::var("PATH").unwrap_or_default()));
        command
    }

    fn ledger(&self, arguments: &[&str]) -> Output {
        let output = self.command().args(arguments).output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        output
    }

    fn summary(&self) -> String {
        String::from_utf8(self.ledger(&["--summary"]).stdout).unwrap()
    }
}

fn matches(text: &str, pattern: &str) {
    assert!(Regex::new(pattern).unwrap().is_match(text), "{pattern} not in\n{text}");
}

fn agy(rig: &Rig) {
    let root = rig.path("agy");
    write(
        &root.join("log/cli-20261008_000000.log"),
        &format!("I1008 printmode.go:202] Print mode: starting (promptLength=9, model=\"gemini-3.1-pro-high\", conversationID=\"\")\nI1008 server.go:1278] Created conversation {AGY}\n"),
    );
    jsonl(
        &root.join("brain").join(AGY).join(".system_generated/logs/transcript_full.jsonl"),
        &[
            json!({"type":"USER_INPUT","status":"DONE","created_at":"2026-10-08T00:00:00Z","content":"<USER_REQUEST>\nItem: wisp#71\nCategory: research\nLens: break it\n</USER_REQUEST>"}),
            json!({"type":"PLANNER_RESPONSE","status":"DONE","created_at":"2026-10-08T00:00:00Z","content":"Accepted flaws: 2","input_tokens":800,"output_tokens":120,"cache_read_tokens":100}),
        ],
    );
    let workspace = "[\"file:///tmp/claude-1000/p/5e55e55e-0000-4000-8000-000000000002/scratchpad\"]";
    sqlite(
        &root.join("conversation_summaries.db"),
        &format!("create table conversation_summaries(conversation_id text, workspace_uris text, last_modified_time datetime); insert into conversation_summaries values('{AGY}', '{workspace}', '2026-10-08 00:07:00.123456789+00:00')"),
    );
}

fn at(kind: &str, payload: Value, minute: u32) -> Value {
    json!({"type": kind, "timestamp": format!("2026-10-08T00:{minute:02}:00Z"), "payload": payload})
}

fn message(role: &str, phase: &str, text: &str, minute: u32) -> Value {
    at("response_item", json!({"type":"message","role":role,"phase":phase,"content":[{"type":"output_text","text":text}]}), minute)
}

fn worker_meta(name: &str) -> Value {
    at("session_meta", json!({"id":name,"timestamp":"2026-10-08T00:00:00Z","source":{"subagent":{"thread_spawn":{"parent_thread_id":"parent","agent_path":format!("/root/{name}")}}}}), 0)
}

fn rollout(rig: &Rig, name: &str, rows: &[Value]) {
    jsonl(&rig.path(&format!("codex/sessions/2026/10/08/rollout-{name}.jsonl")), rows);
}

fn exec_call(id: &str, command: &str, minute: u32) -> [Value; 2] {
    let call = format!("text(await tools.exec_command({{cmd:{},workdir:\"/w\"}}))", Value::String(command.into()));
    [
        at("response_item", json!({"type":"custom_tool_call","name":"exec","call_id":id,"input":call}), minute),
        at("response_item", json!({"type":"custom_tool_call_output","call_id":id,"output":[{"type":"input_text","text":"{\"exit_code\":0,\"output\":\"ok\"}"}]}), minute + 1),
    ]
}

fn codex(rig: &Rig) {
    let brief = "Item: firn#5\nFollows: prior\nCategory: tooling\nETA: 10 minutes";
    let spawn = json!({"message": brief, "model": "gpt-6.1-sol", "reasoning_effort": "medium"}).to_string();
    rollout(
        rig,
        "parent",
        &[
            at("session_meta", json!({"id":"parent","timestamp":"2026-10-08T00:00:00Z","source":"cli"}), 0),
            at("response_item", json!({"type":"function_call","name":"spawn_agent","call_id":"spawn","arguments":spawn}), 0),
            at("response_item", json!({"type":"function_call_output","call_id":"spawn","output":json!({"task_name":"/root/finished"}).to_string()}), 0),
        ],
    );
    let worker = |name: &str, brief: &str| -> Vec<Value> {
        let mut rows = vec![worker_meta(name), at("turn_context", json!({"model":"gpt-6.1-sol","effort":"medium"}), 0)];
        if !brief.is_empty() {
            rows.push(message("assistant", "commentary", brief, 0));
        }
        rows.push(at("event_msg", json!({"type":"token_count","info":{"total_token_usage":{"output_tokens":321},"last_token_usage":{"input_tokens":900}}}), 5));
        rows.push(message("assistant", "final", "Done: landed.", 5));
        rows
    };
    rollout(rig, "finished", &worker("finished", ""));
    rollout(rig, "running", &worker("running", "Item: wisp#70\nCategory: native-check\nETA: 20 minutes"));
    rollout(rig, "hinted", &worker("hinted", ""));
    let mut untracked = worker("untracked", "");
    untracked.splice(2..2, exec_call("u1", "git commit -m \"Derive spawn (firn#9)\"", 1));
    rollout(rig, "untracked", &untracked);
    let mut escaped = untracked.clone();
    escaped[0] = worker_meta("escaped");
    let lane = json!({"exit_code":0,"output":"[lane deadbee] Repair \"quoted\" \\ path (firn#9)\n"}).to_string();
    escaped[3]["payload"]["output"] = json!([{"type":"input_text","text":"Successful command output follows"},{"type":"input_text","text":lane}]);
    rollout(rig, "escaped", &escaped);
    let farm = "bun wisp farm balance \"wren 120\" --wait";
    let mut repeater = vec![
        worker_meta("repeater"),
        at("turn_context", json!({"model":"gpt-6.1-sol","effort":"high"}), 0),
        message("assistant", "commentary", "Item: smashcraft#250\nCategory: balance-tuning\nETA: 20 minutes", 0),
    ];
    for n in 1..=3 {
        repeater.extend(exec_call(&format!("c{n}"), farm, n));
    }
    repeater.push(message("assistant", "final", "Done: measured.", 5));
    rollout(rig, "repeater", &repeater);
}

fn claude_worker(user: &str, assistants: Vec<Value>) -> Vec<Value> {
    let mut rows = vec![json!({"type":"user","timestamp":"2026-10-08T00:00:00Z","sessionId":"session","message":{"content":user}})];
    rows.extend(assistants);
    rows
}

fn assistant(minute: u32, request: &str, usage: Value, content: Value) -> Value {
    json!({"type":"assistant","timestamp":format!("2026-10-08T00:{minute:02}:00Z"),"requestId":request,"message":{"model":"claude-haiku-5-5","usage":usage,"content":content}})
}

fn claude(rig: &Rig) {
    let subagents = rig.path("claude/projects/-home-tom/session/subagents");
    let meta = r#"{"agentType":"worker"}"#;
    let put = |name: &str, rows: &[Value]| {
        jsonl(&subagents.join(format!("agent-{name}.jsonl")), rows);
        write(&subagents.join(format!("agent-{name}.meta.json")), meta);
    };
    put(
        "haiku1",
        &claude_worker("Item: smashcraft#255\nCategory: mechanical\nETA: 15 minutes", vec![assistant(4, "r1", json!({"output_tokens":50,"input_tokens":700}), json!([{"type":"text","text":"Done: landed."}]))]),
    );
    for (name, command) in [
        ("commit1", "cd /w && git -c gc.auto=0 commit -qm fix"),
        ("commit2", "cd /w && git -c gc.auto=0 commit -m \"Unlanded change\""),
        ("commit3", "cd /w && git -c gc.auto=0 commit -q -F - <<EOF\nfix\nEOF"),
    ] {
        let rows = vec![
            json!({"type":"user","timestamp":"2026-10-08T00:00:00Z","sessionId":"session","message":{"content":"Item: smashcraft#256\nCategory: mechanical"}}),
            assistant(1, "r1", json!({"output_tokens":5}), json!([{"type":"tool_use","id":"t1","name":"Bash","input":{"command":command}}])),
            json!({"type":"user","timestamp":"2026-10-08T00:02:00Z","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}),
            assistant(3, "r2", json!({"output_tokens":5}), json!([{"type":"text","text":"Done: committed."}])),
        ];
        put(name, &rows);
    }
}

fn profiles(rig: &mut Rig) {
    let host = fs::read_to_string("/proc/sys/kernel/hostname").unwrap().trim().to_owned();
    let (primary, secondary) = (rig.path("claude"), rig.path("home/.claude"));
    let base = fs::read_to_string(&rig.orchestration).unwrap();
    let config = format!(
        "{base}\n[accounts.claude.profiles.a.\"{host}\"]\npath = \"{p}\"\n[accounts.claude.profiles.b.\"{host}\"]\n[accounts.claude.profiles.overlap.\"{host}\"]\npath = \"{p}/.\"\n",
        p = primary.display()
    );
    write(&rig.path("orchestration.toml"), &config);
    rig.orchestration = rig.path("orchestration.toml");
    write(&primary.join(".claude.json"), &json!({"oauthAccount":{"accountUuid":"fixture-primary"}}).to_string());
    fs::create_dir_all(&secondary).unwrap();
    write(&rig.path("home/.claude.json"), &json!({"oauthAccount":{"accountUuid":"fixture-secondary"}}).to_string());
    let source = fs::read_to_string(primary.join("projects/-home-tom/session/subagents/agent-haiku1.jsonl")).unwrap();
    let rows: Vec<Value> = source.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    let cases = [
        ("profile2", "-home-tom", "session", None, ""),
        ("profile-probe", "-tmp-profile", "session", None, ""),
        ("profile-routine", "-home-tom", "session", None, "[routine:fixture] "),
        ("profile-old", "-home-tom", "session", Some("2026-10-07T00:00:00Z"), ""),
        ("profile-other", "-home-tom", "other-session", None, ""),
    ];
    for (name, project, session, timestamp, prefix) in cases {
        let mut altered = rows.clone();
        altered[0]["sessionId"] = json!(session);
        let content = format!("{prefix}{}", altered[0]["message"]["content"].as_str().unwrap());
        altered[0]["message"]["content"] = json!(content);
        for row in altered.iter_mut().filter(|_| timestamp.is_some()) {
            row["timestamp"] = json!(timestamp);
        }
        let directory = secondary.join(format!("projects/{project}/{session}/subagents"));
        jsonl(&directory.join(format!("agent-{name}.jsonl")), &altered);
        write(&directory.join(format!("agent-{name}.meta.json")), r#"{"agentType":"worker"}"#);
    }
}

fn git(repo: &Path, date: &str, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(arguments)
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn origin_repo(rig: &Rig) -> String {
    let repo = rig.path("repo");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, "2026-10-08T00:00:00Z", &["init", "-q", "."]);
    git(&repo, "2026-10-08T00:00:00Z", &["commit", "-q", "--allow-empty", "-m", "fix"]);
    git(&repo, "2026-10-08T00:02:00Z", &["commit", "-q", "--allow-empty", "-m", "Repair \"quoted\" \\ path (firn#9)"]);
    git(&repo, "2026-10-08T00:02:00Z", &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    git(&repo, "2026-10-08T00:02:00Z", &["rev-parse", "origin/main"])
}

fn lines(text: &str) -> Vec<String> {
    let mut found: Vec<String> = text.lines().map(str::to_owned).collect();
    found.sort();
    found
}

#[test]
fn every_provider_is_recorded_with_its_brief_stages_landing_and_roles_and_the_summary_groups_them() {
    let mut rig = Rig::new();
    agy(&rig);
    codex(&rig);
    claude(&rig);
    profiles(&mut rig);
    let head = origin_repo(&rig);

    rig.ledger(&["--session", "session", "--since", "2026-10-08"]);
    assert_eq!(
        rig.sql("select agent,account,role from runs where agent in ('haiku1','profile2','profile-probe','profile-routine','profile-old','profile-other') order by agent"),
        "haiku1|claude-a|worker\nprofile-probe|claude-b|probe\nprofile-routine|claude-b|routine\nprofile2|claude-b|worker",
        "configured Claude profiles respect session and since, account attribution and probe and routine roles"
    );

    rig.ledger(&["--since", "2026-10-08"]);
    rig.ledger(&["--since", "2026-10-08"]);
    assert_eq!(rig.sql("select agent || '=' || expects_landing from runs where agent in ('commit1','commit2','haiku1','repeater') order by agent"), "commit1=1\ncommit2=1\nhaiku1=0\nrepeater=0");
    assert_eq!(rig.sql("select agent || '=' || landed from runs where agent like 'commit%' order by agent"), "commit1=1\ncommit2=0\ncommit3=1");
    assert_eq!(rig.sql("select length(landed_sha) from runs where agent='commit1'"), "40");
    assert_eq!(rig.sql("select landed_sha is null from runs where agent='commit2'"), "1");
    assert_eq!(rig.sql("select landed_sha from runs where agent='escaped'"), head, "a commit whose subject is on origin/main counts as landed with its SHA though the run never pushed");

    let codex_rows = rig.sql("select agent,item,follows,tier,category,minutes,eta_min,tokens,peak_ctx,outcome from runs where agent not in ('haiku1','commit1','commit2','hinted') and tier like 'gpt%'");
    let wanted = [
        "finished|firn#5|prior|gpt-6.1-sol medium|tooling|5|10|321|900|done",
        "running|wisp#70||gpt-6.1-sol medium|native-check|5|20|321|900|done",
        "untracked|firn#9||gpt-6.1-sol medium|unknown|5||321|900|done",
        "escaped|firn#9||gpt-6.1-sol medium|unknown|5||321|900|done",
        "repeater|smashcraft#250||gpt-6.1-sol high|balance-tuning|5|20|0|0|done",
    ]
    .join("\n");
    assert_eq!(lines(&codex_rows), lines(&wanted), "a Codex spawn brief records fields, model, effort, timing and tokens once; an unreadable brief derives its Item from the commit");

    assert_eq!(rig.sql("select agent || '|' || account || '|' || ifnull(role,'') || '|' || count(*) from runs where agent in ('haiku1','profile2') group by agent order by agent"), "haiku1|claude-a|worker|1\nprofile2|claude-b|worker|1");
    assert_eq!(rig.sql("select count(*) from runs where agent='profile-old'"), "0");
    assert_eq!(rig.sql("select account from runs where agent='profile-other'"), "claude-b");
    assert_eq!(rig.sql("select ifnull(repeats,'none') || '|' || landed from runs where agent='repeater'"), "none|0", "a Codex worker that reran a passing farm check and pushed nothing stores no repeats and no landing");
    assert_eq!(rig.sql("select count(*) from claims"), "0", "finished workers release their holds");

    let columns = "item,session,tier,category,minutes,tokens,peak_ctx,flaws,lens,staffed,review_requested";
    assert_eq!(
        rig.sql(&format!("select {columns} from runs where agent='{AGY}'")),
        "wisp#71|5e55e55e-0000-4000-8000-000000000002|gemini-3.1-pro high|research|7|120|900|2|break it|2026-10-08T00:00:00Z|2026-10-08T00:07:00Z"
    );
    assert_eq!(rig.sql("select tier is not null and category is not null and minutes is not null and tokens is not null and staffed is not null and review_requested is not null from runs where agent='commit1'"), "1");
    assert_eq!(
        rig.sql("select staffed,ifnull(first_commit,''),review_requested,landed_at is not null,ifnull(turns,'') from runs where agent in ('commit1','untracked') order by agent"),
        "2026-10-08T00:00:00Z||2026-10-08T00:03:00Z|1|\n2026-10-08T00:00:00Z||2026-10-08T00:05:00Z|0|",
        "stage times come from the transcript and origin/main: staffed, first report, landed"
    );

    let out = rig.summary();
    matches(&out, r"tooling\s+gpt-6.1-sol\s+medium\s+1\s+1");
    matches(&out, r"native-check\s+gpt-6.1-sol\s+medium\s+1\s+1");
    matches(&out, r"tokens_med\s+no_landing");
    matches(&out, r"balance-tuning\s+gpt-6.1-sol\s+high\s.*\s0k\s+0/0");
}

fn context_rig() -> Rig {
    let rig = Rig::new();
    let (project, workers) = (rig.path("claude/projects/recorded"), rig.path("claude/projects/recorded/recorded-context/subagents"));
    let rows = fixture_rows("worker-ledger-context.jsonl");
    jsonl(&workers.join("agent-context.jsonl"), &rows);
    write(&workers.join("agent-context.meta.json"), r#"{"agentType":"worker"}"#);
    for role in ["parent", "parent-two", "proxy"] {
        let mut altered = rows.clone();
        for row in altered.iter_mut() {
            row["sessionId"] = json!(format!("recorded-{role}"));
        }
        altered[0]["message"]["content"] = json!("Category: tooling\nFollows: context\n[recorded fleet parent context]");
        jsonl(&project.join(format!("recorded-{role}.jsonl")), &altered);
    }
    let tokens = fixture_rows("worker-ledger-codex-context.jsonl");
    let stamp = tokens[0]["timestamp"].as_str().unwrap().to_owned();
    let record = |kind: &str, payload: Value| json!({"type": kind, "timestamp": stamp, "payload": payload});
    let mut rollout = vec![
        record("session_meta", json!({"id":"recorded-codex","timestamp":stamp,"source":{"subagent":{"thread_spawn":{"parent_thread_id":"recorded-parent","agent_path":"/root/recorded-codex"}}}})),
        record("turn_context", json!({"model":"gpt-6.1-sol","effort":"high"})),
        record("response_item", json!({"type":"message","role":"user","content":[{"type":"input_text","text":"Category: tooling\n[recorded child prompt]"}]})),
    ];
    rollout.extend(tokens);
    rollout.push(record("response_item", json!({"type":"message","role":"assistant","phase":"final","content":[{"type":"output_text","text":"[recorded response omitted]"}]})));
    jsonl(&rig.path("codex/sessions/2026/10/10/rollout-recorded.jsonl"), &rollout);
    rig
}

fn context_file(rig: &Rig) -> PathBuf {
    rig.path("claude/projects/recorded/recorded-context/subagents/agent-context.jsonl")
}

fn read_rows(path: &Path) -> Vec<Value> {
    fs::read_to_string(path).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect()
}

fn backfill(rig: &Rig) {
    rig.ledger(&["--backfill", "--since", "2026-10-03"]);
}

#[test]
fn recorded_context_telemetry_counts_compactions_hook_fires_and_ignored_reminders_once_per_request() {
    let rig = context_rig();
    backfill(&rig);
    assert_eq!(rig.sql("select compactions,handoff_fires,idle_gap_misses,peak_ctx from runs where agent='context'"), "1|1|1|664423", "recorded usage and compaction with the seeded hook yield 1/1/1, streamed duplicates count once");
    assert_eq!(rig.sql("select input_tokens,cache_read_tokens,peak_ctx from runs where agent='recorded-codex'"), "18495|12160|18495", "recorded Codex token_count includes cached input and duplicate events count once");
    assert_eq!(rig.sql("select role || '=' || count(*) from runs group by role order by role"), "parent=2\nproxy=1\nworker=2");
    assert_eq!(rig.sql("select count(*) from claims"), "0");
    assert_eq!(rig.sql("select count(*) from runs where role!='worker' and item is not null"), "0");
    let out = rig.summary();
    matches(&out, r"peak_ctx_p50.*peak_ctx_p90.*compactions/run.*handoff_fires.*cache_read_share.*idle_gap_misses");
    matches(&out, r"tooling\s+medium\s+-\s+1");
    matches(&out, r"parent\s+tooling");

    let path = context_file(&rig);
    let mut rows = read_rows(&path);
    for row in rows.iter_mut().filter(|row| row["fixtureOrigin"] == "seeded worker-handoff emission") {
        row["message"]["content"] = json!("Quoted source: Your context is 400k tokens, past the 350k handoff point.");
    }
    jsonl(&path, &rows);
    backfill(&rig);
    assert_eq!(rig.sql("select compactions,handoff_fires,idle_gap_misses from runs where agent='context'"), "1|0|1", "a quoted hook source counts zero fires");
    assert_eq!(rig.sql("select count(*) from runs where role!='worker' and item is not null"), "0");

    let mut rows = fixture_rows("worker-ledger-context.jsonl");
    for row in rows.iter_mut().filter(|row| row["fixtureOrigin"] == "seeded worker-handoff emission") {
        row.as_object_mut().unwrap().remove("message");
        row["type"] = json!("attachment");
        row["attachment"] = json!({"type":"hook_additional_context","content":["Your context is 400k tokens, past the 350k handoff point. [seeded instruction omitted]"]});
    }
    let stamp = rows.last().unwrap()["timestamp"].clone();
    let turn = |identity: &str, content: Value| json!({"type":"assistant","timestamp":stamp,"sessionId":"recorded-context","requestId":identity,"message":{"id":identity,"content":content}});
    rows.push(turn("second-turn", json!([{"type":"thinking","thinking":"[omitted]"}])));
    rows.push(turn("second-turn", json!([{"type":"text","text":"[seeded second turn]"}])));
    rows.push(turn("third-turn", json!([{"type":"text","text":"HANDOFF /recorded/handoff.md"}])));
    jsonl(&path, &rows);
    backfill(&rig);
    assert_eq!(rig.sql("select handoff_fires,handoff_ignored from runs where agent='context'"), "1|0", "a native attachment fire followed by HANDOFF on the third assistant turn is effective");

    let mut rows = read_rows(&path);
    let last = rows.len() - 1;
    rows[last]["message"]["content"][0]["text"] = json!("[seeded third turn ignores the reminder]");
    let mut fourth = rows[last].clone();
    fourth["requestId"] = json!("fourth-turn");
    fourth["message"]["id"] = json!("fourth-turn");
    fourth["message"]["content"] = json!([{"type":"text","text":"HANDOFF /recorded/handoff.md"}]);
    rows.push(fourth);
    jsonl(&path, &rows);
    backfill(&rig);
    assert_eq!(rig.sql("select handoff_fires,handoff_ignored from runs where agent='context'"), "1|1", "an ignored reminder trips even with HANDOFF on the fourth assistant turn");
}

fn usage_rows() -> Vec<Value> {
    [(0, 10), (10, 14), (20, 20)]
        .iter()
        .map(|(minute, used)| json!({"type":"usage","ts":format!("2026-10-11T00:{minute:02}:00Z"),"provider":"claude","account":"b","kind":"window","name":"weekly_all","used_pct":used,"resets_at":"2026-10-15T00:00:00Z"}))
        .collect()
}

fn costed(agent: &str, fast: u8, samples: &[(&str, f64)], brief: &str) -> Value {
    let samples: Vec<Value> = samples.iter().enumerate().map(|(n, (ts, weight))| json!({"request":n.to_string(),"ts":ts,"weight":weight})).collect();
    json!({"id":agent,"account":"claude-b","model":"claude-opus-4-6","effort":"high","fast":fast,"usage_samples":samples,"tier":"high","actual_min":20,"started":"2026-10-11T00:00:00Z","ended":"2026-10-11T00:20:00Z","outcome":"done","landed":1,"staffed":"2026-10-11T00:00:00Z","landed_at":"2026-10-11T00:20:00Z","brief":brief})
}

fn ingest(rig: &Rig, rows: &[Value]) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_north"))
        .args(["threads", "ingest"])
        .env("HOME", rig.path("home"))
        .env("THREADS_DB", rig.path("threads.db"))
        .env("NORTH_HOME", REPO)
        .env("NORTH_BIN", rig.path("bin/north"))
        .env("AGENTS_ORCHESTRATION", &rig.orchestration)
        .env("AGENTS_OPERATOR_DIR", Path::new(REPO).join("harness/lib/fixtures"))
        .env("PATH", format!("{}:{}", rig.path("bin").display(), std::env::var("PATH").unwrap_or_default()))
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(&mut child.stdin.take().unwrap(), rows.iter().map(|row| format!("{row}\n")).collect::<String>().as_bytes()).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn a_measured_interval_cost_is_allocated_to_tracked_runs_once_and_replays_idempotently() {
    let rig = Rig::new();
    fs::remove_file(rig.path("threads.db")).unwrap();
    let first = costed("a", 1, &[("2026-10-11T00:05:00Z", 730.0)], "Item: north#2");
    let second = costed("b", 0, &[("2026-10-11T00:08:00Z", 270.0), ("2026-10-11T00:15:00Z", 100.0)], "Item: wisp#3");
    let mut readouts = usage_rows();
    ingest(&rig, &readouts);
    assert_eq!(rig.sql("select count(*) from usage"), "3");
    let running = {
        let mut running = second.clone();
        running["status"] = json!("running");
        running
    };
    ingest(&rig, &[first.clone(), running]);
    assert_eq!(rig.sql("select count(*) from runs"), "1", "a running record is not stored as a run");
    assert!((rig.sql("select sum(cost_usd) from run_costs").parse::<f64>().unwrap() - 5.0).abs() < 1e-9, "the whole interval cost is allocated, a running run included");
    ingest(&rig, std::slice::from_ref(&second));
    assert!((rig.sql("select sum(cost_usd) from runs").parse::<f64>().unwrap() - 5.0).abs() < 1e-9);
    assert_eq!(rig.sql("select fast || '=' || count(*) from runs group by fast order by fast"), "0=1\n1=1");
    readouts.extend([first.clone(), second.clone()]);
    ingest(&rig, &readouts);
    assert_eq!(rig.sql("select count(*) from run_usage"), "3", "a replay adds no sample twice");
    let before = rig.sql("select fast || '|' || ifnull(role,'') || '|' || outcome || '|' || minutes || '|' || ifnull(landed_at,'') from runs where agent='a'");
    let mut telemetry = first.clone();
    telemetry["telemetry_only"] = json!(true);
    telemetry["actual_min"] = json!(999);
    telemetry["outcome"] = json!("unclear");
    telemetry["fast"] = json!(0);
    telemetry["role"] = json!(null);
    ingest(&rig, &[telemetry]);
    let after = rig.sql("select fast || '|' || ifnull(role,'') || '|' || outcome || '|' || minutes || '|' || ifnull(landed_at,'') from runs where agent='a'");
    assert_eq!(after.split('|').skip(1).collect::<Vec<_>>(), before.split('|').skip(1).collect::<Vec<_>>(), "a telemetry-only record leaves outcome, minutes and landing");
    assert_eq!(after.split('|').next(), Some("0"));
}

fn calibration_batches(rig: &Rig, comparison: &str) -> i64 {
    rig.sql(&format!("select count(distinct at) from calibration where kind {comparison} 'prior'")).parse().unwrap()
}

fn lax(rig: &Rig, extra: &[(&str, &Path)]) {
    let mut command = rig.command();
    command.args(["--since", "2026-10-08"]);
    for (name, value) in extra {
        command.env(name, value);
    }
    let output = command.output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

fn monday() -> String {
    let now = time::OffsetDateTime::now_utc();
    let start = now.date().midnight().assume_utc() - time::Duration::days(i64::from(now.weekday().number_days_from_monday()));
    format!("{:04}-{:02}-{:02}T00:00:00Z", start.year(), u8::from(start.month()), start.day())
}

fn prior_names() -> Vec<String> {
    let document: toml_edit::DocumentMut = fs::read_to_string(Path::new(REPO).join("harness/defaults.toml")).unwrap().parse().unwrap();
    document.iter().filter(|(_, entry)| entry.get("source").and_then(|source| source.as_str()).is_some_and(|source| source.starts_with("prior: "))).map(|(name, _)| name.to_owned()).collect()
}

#[test]
fn a_recording_run_recalibrates_a_stale_ledger_once_and_records_priors_once_a_week_where_codex_rollouts_exist() {
    let rig = context_rig();
    backfill(&rig);
    let stale = "update calibration set at='2000-01-01T00:00:00Z' where kind!='prior'";
    rig.sql(stale);
    let before = calibration_batches(&rig, "!=");
    lax(&rig, &[]);
    assert_eq!(calibration_batches(&rig, "!="), before + 1);
    assert_eq!(rig.sql("select max(at) > datetime('now','-5 minutes') from calibration where kind!='prior'"), "1");
    lax(&rig, &[]);
    assert_eq!(calibration_batches(&rig, "!="), before + 1, "a fresh calibration is not rewritten");
    rig.sql("update calibration set at=strftime('%Y-%m-%dT%H:%M:%SZ','now','-54 minutes') where kind!='prior'");
    let inside = calibration_batches(&rig, "!=");
    lax(&rig, &[]);
    assert_eq!(calibration_batches(&rig, "!="), inside, "a calibration 54 minutes old is inside the 55-minute window");
    rig.sql("update calibration set at=strftime('%Y-%m-%dT%H:%M:%SZ','now','-56 minutes') where kind!='prior'");
    let outside = calibration_batches(&rig, "!=");
    lax(&rig, &[]);
    assert_eq!(calibration_batches(&rig, "!="), outside + 1, "a calibration 56 minutes old is recalibrated");
    rig.sql(stale);
    write(&rig.path("home/.config/threads/remote"), "nexus\n");
    lax(&rig, &[]);
    assert_eq!(calibration_batches(&rig, "!="), before, "a host configured with a remote ledger skips calibration");
    fs::remove_file(rig.path("home/.config/threads/remote")).unwrap();

    let names = prior_names();
    rig.sql("create table if not exists usage(ts, provider, kind, name, used_pct, resets_at); create table if not exists model_events(model, kind, at)");
    let measured = Command::new(env!("CARGO_BIN_EXE_north")).args(["defaults-measure", "--priors"]).arg(rig.path("threads.db")).output().unwrap();
    assert!(measured.status.success(), "{}", String::from_utf8_lossy(&measured.stderr));
    let printed = String::from_utf8(measured.stdout).unwrap();
    let row = Regex::new(r"^(SETTLED|WAIT) (\S+) n=(\d+)(?:/\d+)?(?: value=(\S+))?$").unwrap();
    let mut seen = Vec::new();
    for line in printed.lines() {
        let found = row.captures(line).unwrap_or_else(|| panic!("{line}"));
        assert!(found.get(4).is_none_or(|value| value.as_str().parse::<f64>().is_ok_and(f64::is_finite)), "{line}");
        seen.push(found[2].to_owned());
    }
    seen.sort();
    let mut expected = names.clone();
    expected.sort();
    assert_eq!(seen, expected, "every advertised prior evaluates with a nonnegative integer count and optional finite measurement");

    rig.sql("update calibration set at='2000-01-01T00:00:00Z' where kind='prior'");
    let before = calibration_batches(&rig, "=");
    lax(&rig, &[]);
    assert_eq!(calibration_batches(&rig, "="), before + 1);
    assert_eq!(rig.sql("select count(*) from calibration where kind='prior' and at=(select max(at) from calibration where kind='prior') and value > 0"), names.len().to_string());
    rig.sql(&format!("update calibration set at='{}' where kind='prior'", monday()));
    lax(&rig, &[]);
    assert_eq!(calibration_batches(&rig, "="), before, "no second prior batch in the same week");
    rig.sql(&format!("update calibration set at=datetime('{}','-1 minute') where kind='prior'", monday()));
    let none = rig.path("no-codex");
    lax(&rig, &[("CODEX_CONFIG_DIR", &none)]);
    assert_eq!(calibration_batches(&rig, "="), before, "no priors without Codex rollouts");
    lax(&rig, &[]);
    assert_eq!(calibration_batches(&rig, "="), before + 1, "the first run after Monday 00:00Z records the week's priors");
}
