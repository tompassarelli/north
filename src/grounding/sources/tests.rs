use super::*;
use std::collections::{BTreeMap, HashMap, HashSet};
fn assert_keys(d: &Doc) {
    let v = serde_json::to_value(d).unwrap();
    assert_eq!(
        v.as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["at", "decays", "id", "ref", "source", "text", "title"]
    );
}
fn ledger_fixture() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("threads.db");
    let mut db = Sqlite::open(&path, false).unwrap();
    db.exec(r#"CREATE TABLE events(seq INTEGER PRIMARY KEY, ts TEXT, item TEXT, kind TEXT, actor TEXT, data TEXT);
CREATE TABLE runs(item TEXT, ended TEXT, outcome TEXT, tier TEXT, report TEXT, note TEXT);
CREATE TABLE gh_cache(repo TEXT, fetched REAL, issues TEXT);
CREATE TABLE blocks(item TEXT, reason TEXT); CREATE TABLE claims(item TEXT, owner TEXT);
CREATE TABLE items(ns TEXT, number INTEGER, title TEXT, body TEXT, state TEXT, created TEXT, closed TEXT, due TEXT);
INSERT INTO items VALUES ('ops',1,'Codex routing trial','route overflow to codex','closed','2026-10-13T12:00:00Z','2026-10-13T13:00:00Z',NULL),('ops',2,'Laptop availability','','open','2026-10-14T12:00:00Z',NULL,NULL);
INSERT INTO events(ts,item,kind,actor,data) VALUES ('2026-10-14T13:00:00Z','ops#2','note','tick','{"text":"laptop draining"}'),('2026-10-12T10:00:00Z','north#7','claim','lead','{}'),('2026-10-12T10:05:00Z','north#9','claim','lead','{}');
INSERT INTO runs VALUES ('north#7','2026-10-14T09:00:00.000Z','done','opus','Done: grant age fixed',NULL);
INSERT INTO gh_cache VALUES ('north',0,'{"7":{"title":"Grant age telemetry","blockers":[]}}');"#).unwrap();
    (dir, path)
}
#[test]
fn threads_native_items_and_issue_threads_become_documents_and_since_returns_only_later_updates() {
    let (_dir, path) = ledger_fixture();
    let docs: BTreeMap<_, _> = issues::threads(None, &path, true)
        .unwrap()
        .into_iter()
        .map(|d| (d.id.clone(), d))
        .collect();
    assert_eq!(
        docs.keys().map(String::as_str).collect::<Vec<_>>(),
        ["north#7", "north#9", "ops#1", "ops#2"]
    );
    for d in docs.values() {
        assert_keys(d);
    }
    assert_eq!(docs["ops#1"].reference, "threads ops#1");
    assert!(docs["ops#1"].decays);
    assert!(!docs["ops#2"].decays);
    assert!(docs["ops#2"].text.contains("laptop draining"));
    assert_eq!(docs["north#7"].title, "Grant age telemetry");
    assert!(docs["north#7"].text.contains("Done: grant age fixed"));
    assert!(!docs["north#7"].decays);
    assert!(docs["north#9"].decays);
    let mut later: Vec<_> = issues::threads(Some(epoch("2026-10-14T08:00:00Z")), &path, true)
        .unwrap()
        .into_iter()
        .map(|d| d.id)
        .collect();
    later.sort();
    assert_eq!(later, ["north#7", "ops#2"]);
}
#[test]
fn threads_native_items_stay_hidden_where_threads_hides_them() {
    let (_dir, path) = ledger_fixture();
    let mut ids: Vec<_> = issues::threads(None, &path, false)
        .unwrap()
        .into_iter()
        .map(|d| d.id)
        .collect();
    ids.sort();
    assert_eq!(ids, ["north#7", "north#9"]);
}
#[test]
fn github_every_tracked_repo_pages_together_in_one_query_per_page_incrementally_with_trimmed_text()
{
    let mut calls = Vec::new();
    let mut collect = |since: f64| {
        issues::github_with(Some(since),|args|{calls.push(args.to_vec());let fields:BTreeMap<_,_>=args.iter().filter(|a|a.contains('=')&&!a.starts_with("query=")).filter_map(|a|a.split_once('=')).collect();let repos=args.iter().find(|a|a.starts_with("query=")).unwrap().matches("repository(").count();let mut data=serde_json::Map::new();for i in 0..repos{let first=!fields.contains_key(format!("c{i}").as_str());let number=if first{1}else{2};let labels=if first{vec![json!({"name":"priority:now"})]}else{vec![]};data.insert(format!("r{i}"),json!({"issues":{"pageInfo":{"hasNextPage":first,"endCursor":if first{Some("c1")}else{None}},"nodes":[{"number":number,"title":format!("Issue {number}"),"body":"body ".repeat(600),"state":if first{"CLOSED"}else{"OPEN"},"updatedAt":if first{"2026-10-14T00:00:00Z"}else{"2026-10-15T00:00:00Z"},"labels":{"nodes":labels}}]}}));}Ok(json!({"data":data}).to_string())}).unwrap()
    };
    let docs = collect(epoch("2026-10-13T00:00:00Z"));
    let later = collect(epoch("2026-10-14T00:00:00Z"));
    let (owner, repos) = (CATALOG.owner.clone(), CATALOG.repos.clone());
    assert_eq!(calls[..2].len(), 2);
    assert_eq!(calls.len(), 4);
    assert_eq!(docs.len(), 2 * repos.len());
    assert!(
        calls[..2]
            .iter()
            .all(|c| c.contains(&"since=2026-10-13T00:00:00Z".into()))
    );
    assert_keys(&docs[0]);
    assert_eq!(docs[0].reference, format!("github {owner}/{}#1", repos[0]));
    assert!(docs[0].decays);
    assert!(!docs.last().unwrap().decays);
    assert!(docs[0].text.contains("labels: priority:now"));
    assert!(docs.iter().map(|d| ulen(&d.text)).max().unwrap() <= 2000);
    assert_eq!(
        later
            .iter()
            .map(|d| d.id.split('#').nth(1).unwrap())
            .collect::<HashSet<_>>(),
        HashSet::from(["2"])
    );
}
#[test]
fn commits_a_commit_landed_inside_a_merged_lane_is_indexed_once_with_its_subject_and_paths() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("proj/main");
    fs::create_dir_all(&main).unwrap();
    let run = |args: &[&str]| {
        let mut cmd = vec![
            "-C",
            main.to_str().unwrap(),
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
        ];
        cmd.extend_from_slice(args);
        command("git", &cmd).unwrap().trim().to_owned()
    };
    run(&["init", "-q", "-b", "main"]);
    run(&[
        "remote",
        "add",
        "origin",
        "https://github.com/tompassarelli/proj",
    ]);
    fs::write(main.join("a.txt"), "a").unwrap();
    run(&["add", "a.txt"]);
    run(&["commit", "-q", "-m", "first on main"]);
    run(&["checkout", "-q", "-b", "lane"]);
    fs::write(main.join("bin-agent-contain"), "x").unwrap();
    run(&["add", "bin-agent-contain"]);
    run(&["commit", "-q", "-m", "inside the lane"]);
    run(&["checkout", "-q", "main"]);
    fs::write(main.join("b.txt"), "b").unwrap();
    run(&["add", "b.txt"]);
    run(&["commit", "-q", "-m", "diverge on main"]);
    run(&["merge", "-q", "--no-ff", "-m", "merge lane", "lane"]);
    let lane = run(&["rev-parse", "lane"]);
    let docs = history::commits(None, dir.path()).unwrap();
    let d = docs
        .iter()
        .find(|d| d.id == format!("proj@{lane}"))
        .unwrap();
    assert_eq!(d.reference, format!("proj@{}", cut(&lane, 8)));
    assert!(d.title.contains("inside the lane"));
    assert!(d.text.contains("bin-agent-contain"));
    assert_eq!(
        docs.iter().map(|d| &d.id).collect::<HashSet<_>>().len(),
        docs.len()
    );
    assert_eq!(docs.len(), 4);
}
#[test]
fn settings_every_registered_hook_becomes_one_document_carrying_its_event_matcher_file_name_and_header_comment()
 {
    let dir = tempfile::tempdir().unwrap();
    let hook = dir.path().join("block-thing.sh");
    fs::write(&hook,"#!/usr/bin/env bash\n# block-thing.sh: denies the thing\n# before it happens.\nset -e\n# not header\n").unwrap();
    let binary = dir.path().join("compiled");
    fs::write(&binary, [0x7f, 0x45, 0, 0x4c]).unwrap();
    let settings = dir.path().join("settings.json");
    let entry = |command: String| json!({"type":"command","command":command});
    fs::write(&settings,json!({"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[entry(format!("/usr/bin/env A=1 bash {}",hook.display())),entry(binary.to_string_lossy().into_owned())]}],"SessionStart":[{"hooks":[entry(format!("bash {}",hook.display()))]}]}}).to_string()).unwrap();
    let docs = settings::hook_docs(&settings, 5.0);
    assert_eq!(docs.len(), 3);
    for d in &docs {
        assert_keys(d);
    }
    let pre = docs
        .iter()
        .find(|d| d.id == "hook:PreToolUse:Bash:block-thing.sh")
        .unwrap();
    assert!(pre.title.contains("PreToolUse"));
    assert!(pre.text.contains("matcher Bash"));
    assert!(pre.text.contains("denies the thing\nbefore it happens."));
    assert!(!pre.text.contains("not header"));
    assert!(
        docs.iter()
            .find(|d| d.id == "hook:SessionStart::block-thing.sh")
            .unwrap()
            .text
            .contains("every tool")
    );
    assert!(
        !docs
            .iter()
            .find(|d| d.id.ends_with(":compiled"))
            .unwrap()
            .text
            .contains('\0')
    );
}
#[test]
fn settings_the_events_db_document_names_the_ledger_path_and_event_kinds_without_row_data() {
    let d = settings::events_doc(
        &std::env::current_dir()
            .unwrap()
            .join("harness/lib/durable_events.py"),
        5.0,
    )
    .unwrap();
    assert!(d.reference.contains("threads.db"));
    assert!(d.text.contains("events.db"));
    assert!(d.text.contains("LAND-READY"));
    assert_keys(&d);
}
#[test]
fn structured_hook_documents_preserve_inline_and_array_table_toml_forms() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("policy-owners.toml");
    let hook = dir.path().join("hooks/quokka.sh");
    fs::create_dir_all(hook.parent().unwrap()).unwrap();
    fs::write(
        &hook,
        "#!/usr/bin/env bash\n# Quokka denies drift before it happens.\nexit 0\n",
    )
    .unwrap();
    for text in [
        "version=1\nguard=[{unit='quokka',command='quokka.sh',claude=['UserPromptSubmit:'],codex=['SessionStart']}]\n",
        "version=1\n[[guard]]\nunit='quokka'\ncommand='quokka.sh'\nclaude=['UserPromptSubmit:']\ncodex=['SessionStart']\n",
    ] {
        fs::write(&path, text).unwrap();
        let docs = policy::file_docs(&path, "owners", &HashMap::new());
        let hooks: Vec<_> = docs
            .iter()
            .filter(|d| d.title.starts_with("hook "))
            .collect();
        assert_eq!(hooks.len(), 1);
        assert_eq!(
            hooks[0].title,
            "hook quokka (UserPromptSubmit:, codex SessionStart)"
        );
        assert_eq!(hooks[0].reference, display(&hook));
        assert!(
            hooks[0]
                .text
                .contains("Quokka denies drift before it happens.")
        );
    }
}
#[test]
fn transcripts_brief_shaped_user_text_is_never_indexed_as_tom_and_toms_own_messages_are_in_proxy_lead_and_worker_sessions()
 {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("convo.db");
    let mut db = Sqlite::open(&path, false).unwrap();
    db.exec("CREATE TABLE files(id INTEGER PRIMARY KEY,session_id TEXT,agent TEXT); CREATE TABLE body(id INTEGER PRIMARY KEY,text TEXT); CREATE TABLE msg(id INTEGER PRIMARY KEY,file_id INTEGER,line INTEGER,ts TEXT,role TEXT,body INTEGER,dup INTEGER DEFAULT 0);").unwrap();
    let briefs = [
        "Delegation: role=worker budget=0\nCategory: bug\nSetup: load skills",
        "Category: bug-known-cause\nScope: module\nSurface: ts\nVerify: run the thing",
        "Read /home/tom/.local/state/agents/handoffs/foo-brief.md and follow it.",
        "read /home/tom/.local/state/some-evidence/brief.md and follow it",
        "You are the north lead. Your goal is to land the fix.",
        "<task-notification><task-id>abc</task-id>done</task-notification>",
        "[routine:tick] check the queue",
        "Another Claude session sent a message: please look at this",
        "Coach reply from north-lead, relay verbatim: use the lane",
        "<system-reminder>hook says stop</system-reminder>",
    ];
    let sessions = [
        (
            1,
            "aaaaaaaa",
            "Read /home/tom/.local/state/agents/handoffs/proxy-handoff-2026-10-13-c.md and take over as the proxy.",
        ),
        (2, "bbbbbbbb", "You are the north lead. Run the campaign."),
        (
            3,
            "cccccccc",
            "Delegation: role=worker budget=0\nCategory: bug\nScope: module",
        ),
        (4, "dddddddd", "please fix the grounding recall"),
    ];
    let mut n = 0;
    let mut ids = HashMap::new();
    for (file, session, opening) in sessions {
        db.exec(&format!(
            "insert into files values ({file},{},NULL);",
            quote(session)
        ))
        .unwrap();
        let mut add = |role: &str, text: &str| {
            n += 1;
            db.exec(&format!("insert into body values ({n},{});insert into msg(id,file_id,line,ts,role,body) values ({n},{file},{n},{},{},{n});",quote(text),quote(&Timestamp::from_unix_seconds_f64(Timestamp::now().unix_seconds_f64()-n as f64).iso_millis()),quote(role))).unwrap();
            n
        };
        add("user", opening);
        for brief in briefs {
            add("user", brief);
        }
        ids.insert(
            format!("tom{file}"),
            add(
                "user",
                &format!("tom typed this into session {session} about retrieval"),
            ),
        );
        ids.insert(
            format!("reply{file}"),
            add(
                "assistant",
                &format!("assistant reply in session {session} with several words"),
            ),
        );
    }
    let docs = history::transcripts(None, &path).unwrap();
    let by_id: HashMap<_, _> = docs.iter().map(|d| (d.id.clone(), d)).collect();
    for d in &docs {
        if d.title.starts_with("Tom ") {
            assert!(re(r"^(tom typed this|please fix the grounding)").is_match(&d.text));
        }
    }
    for file in [1, 2, 3, 4] {
        assert!(
            by_id[&ids[&format!("tom{file}")].to_string()]
                .title
                .starts_with("Tom ")
        );
    }
    for file in [1, 4] {
        assert!(
            by_id[&ids[&format!("reply{file}")].to_string()]
                .title
                .starts_with("proxy ")
        );
    }
    for file in [2, 3] {
        assert!(!by_id.contains_key(&ids[&format!("reply{file}")].to_string()));
    }
    assert_eq!(
        docs.iter().filter(|d| d.title.starts_with("Tom ")).count(),
        5
    );
}
#[test]
fn refresh_lock_held_by_a_starting_refresh_keeps_an_idle_refresh_out_until_released() {
    let root = tempfile::tempdir().unwrap();
    let held = refresh_lock(root.path(), false)
        .unwrap()
        .expect("first refresh takes the lock");
    assert_eq!(
        fs::read_to_string(root.path().join("refresh.lock")).unwrap(),
        ""
    );
    assert!(refresh_lock(root.path(), false).unwrap().is_none());
    drop(held);
    assert!(refresh_lock(root.path(), false).unwrap().is_some());
}
