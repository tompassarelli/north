use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, params};
use serde_json::{Value, json};

use super::alerts::{alert_rows, location_config};
use super::geometry::Location;
use super::host::{Finished, Host};
use super::models::{Row, Source, SCHEMA, date, model_names, persist, release_candidates};
use super::{Options, digest, execute, news, sources};
use crate::foundation::clock::Timestamp;

const NOW: &str = "2026-10-27T12:00:00Z";

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ops/model_watch/fixtures").join(name)).unwrap()
}

fn recorded() -> Value {
    serde_json::from_slice(&fixture("expected.json")).unwrap()
}

fn source(name: &str, parser: &str) -> Source {
    Source { name: name.into(), parser: parser.into(), endpoint: format!("https://example.com/{name}") }
}

fn rows_json<T: serde::Serialize>(rows: &T) -> Value {
    serde_json::to_value(rows).unwrap()
}

fn pairs_json(rows: &digest::Rows) -> Value {
    Value::Object(rows.iter().map(|(key, (title, url))| (key.clone(), json!({"title": title, "url": url}))).collect())
}

#[test]
fn recorded_sources_parse_to_the_rows_the_python_tool_produced() {
    let recorded = recorded();
    let cases = [
        ("claude-models", "claude-models", "claude-models.html"),
        ("rss", "rss", "rss.xml"),
        ("atom", "atom", "atom.xml"),
        ("news", "anthropic-news", "news.html"),
        ("changelog", "changelog", "changelog.md"),
        ("manifold", "manifold", "manifold.json"),
        ("polymarket", "polymarket", "polymarket.json"),
        ("kalshi", "kalshi", "kalshi.json"),
    ];
    for (name, parser, file) in cases {
        let (source, payload) = (source(name, parser), fixture(file));
        let rows = sources::normalize(&source, &payload).unwrap();
        assert_eq!(rows_json(&rows), recorded["normalize"][name], "rows of {name}");
        assert_eq!(pairs_json(&release_candidates(&source, &payload, &rows).unwrap()), recorded["candidates"][name], "candidates of {name}");
    }
    for (name, parser, file) in [("hn", "hn-front-page", "hn.json"), ("feed", "feed", "feed.xml")] {
        assert_eq!(pairs_json(&digest::feed_rows(parser, &fixture(file)).unwrap()), recorded["feeds"][name], "digest rows of {name}");
    }
    for (text, names) in recorded["names"].as_object().unwrap() {
        assert_eq!(json!(model_names(text)), *names, "names in {text:?}");
    }
    for (input, expected) in recorded["dates"].as_object().unwrap() {
        let value: Value = match input.trim_matches('\'') {
            number if number.chars().all(|c| c.is_ascii_digit()) && !number.is_empty() => json!(number.parse::<i64>().unwrap()),
            text => json!(text),
        };
        assert_eq!(json!(date(&value)), *expected, "date of {input}");
    }
    assert!(sources::normalize(&source("bad", "nonsense"), b"{}").is_err());
    assert!(sources::normalize(&source("none", "claude-models"), b"<p>nothing</p>").is_err());
}

fn location() -> Location {
    location_config(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ops/model_watch/fixtures/location.toml")).unwrap()
}

fn alert_json(rows: &[super::alerts::Alert], name: &str) -> Value {
    Value::Array(rows.iter().map(|row| {
        let mut value = json!({"key": row.key, "source": name, "title": row.title, "url": row.url});
        if let Some(level) = row.level {
            value["level"] = json!(level);
            value["country"] = json!(row.country);
        }
        value
    }).collect())
}

#[test]
fn recorded_location_feeds_select_the_alerts_the_python_tool_selected() {
    let recorded = recorded();
    let location = location();
    for (name, file) in [("usgs", "usgs.json"), ("gdacs", "gdacs.xml"), ("travel", "travel.xml"), ("weather", "weather.atom")] {
        let source = source(name, name);
        let rows = alert_rows(&source, &fixture(file), &location, NOW).unwrap();
        assert_eq!(alert_json(&rows, name), recorded["alerts"][name], "alerts of {name}");
    }
    let past = alert_rows(&source("weather", "weather"), &fixture("weather.atom"), &location, "2100-01-01T00:00:00Z").unwrap();
    assert!(past.is_empty(), "an expired warning must not alert");
    assert!(alert_rows(&source("x", "unknown"), b"<a/>", &location, NOW).is_err());
    assert!(alert_rows(&source("usgs", "usgs"), br#"{"features":[{"id":"x","properties":{"mag":5,"title":"t","url":"https://e.org"},"geometry":{"coordinates":[500,0]}}]}"#, &location, NOW).is_err());
}

#[derive(Default)]
struct Fake {
    pages: RefCell<BTreeMap<String, Vec<u8>>>,
    sent: RefCell<Vec<String>>,
    ssh_code: Cell<i32>,
}

impl Fake {
    fn page(&self, endpoint: &str, body: impl AsRef<[u8]>) {
        self.pages.borrow_mut().insert(endpoint.into(), body.as_ref().to_vec());
    }

    fn messages(&self) -> Vec<String> {
        self.sent.borrow().clone()
    }
}

impl Host for Fake {
    fn fetch(&self, source: &Source, _timeout: f64) -> Result<Vec<u8>, String> {
        let pages = self.pages.borrow();
        pages.iter().find(|(endpoint, _)| source.endpoint.starts_with(endpoint.as_str())).map(|(_, body)| body.clone()).ok_or_else(|| "HTTP 404".to_owned())
    }

    fn command(&self, program: &str, args: &[String], input: Option<&str>, _timeout: Duration, _env: &[(&str, &str)]) -> Result<Finished, String> {
        let ok = |stdout: String| Ok(Finished { code: 0, stdout });
        match (program, args.first().map(String::as_str)) {
            ("claude", Some("auth")) => ok(json!({"loggedIn": true, "authMethod": "claude.ai", "subscriptionType": "max"}).to_string()),
            ("claude", Some("-p")) => {
                let prompt: Value = serde_json::from_str(input.unwrap().split_once('\n').unwrap().1).unwrap();
                let kept: Vec<Value> = prompt["candidates"].as_array().unwrap().iter().map(|row| json!({"id": row["id"], "score": 0.9, "why": "Useful engine release for game development."})).collect();
                ok(json!({"result": Value::Array(kept).to_string(), "session_id": "recorded-ranking"}).to_string())
            }
            ("ssh", _) => {
                let remote = args.last().unwrap().clone();
                if self.ssh_code.get() == 0 {
                    self.sent.borrow_mut().push(remote);
                }
                Ok(Finished { code: self.ssh_code.get(), stdout: String::new() })
            }
            _ => ok(String::new()),
        }
    }

    fn now(&self) -> Timestamp {
        Timestamp::parse(NOW).unwrap()
    }

    fn account(&self) -> Option<String> {
        Some("a".into())
    }
}

fn options(dir: &Path) -> Options {
    let mut options = Options::defaults();
    options.db = dir.join("threads.db");
    options.usage_db = options.db.clone();
    options.interests = dir.join("interests.md");
    options.feedback = dir.join("feedback.jsonl");
    options.proposals = dir.join("proposals");
    options.location = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ops/model_watch/fixtures/location.toml");
    options.topic = "socrates-test".into();
    options.north = "north-test".into();
    options.external_billing = false;
    options.ntfy_url = None;
    options.ntfy_topic = "urgent".into();
    options
}

fn seed_usage(path: &Path) {
    let db = Connection::open(path).unwrap();
    db.execute_batch("CREATE TABLE usage(ts TEXT, provider TEXT, account TEXT, kind TEXT, name TEXT, used_pct REAL, window_min INTEGER, resets_at TEXT, amount REAL, source TEXT) STRICT").unwrap();
    for (kind, name, amount) in [("window", "week", None), ("billing", "extra_usage", Some(0.0)), ("billing", "spend", Some(0.0))] {
        db.execute("INSERT INTO usage VALUES(?,?,?,?,?,?,?,?,?,?)", params![NOW, "claude", "a", kind, name, 10.0, 10080, None::<String>, amount, "recorded scenario"]).unwrap();
    }
}

fn count(path: &Path, sql: &str) -> i64 {
    Connection::open(path).unwrap().query_row(sql, [], |row| row.get(0)).unwrap_or(0)
}

fn release_feed(with_new: bool) -> String {
    let mut entries = String::from(r#"<entry><id>baseline</id><title>baseline release</title><link href="https://example.com/baseline"/></entry>"#);
    if with_new {
        entries.push_str(r#"<entry><id>new</id><title>Introducing GPT-6.1</title><link href="https://example.com/release"/></entry>"#);
    }
    format!(r#"<feed xmlns="http://www.w3.org/2005/Atom">{entries}</feed>"#)
}

#[test]
fn a_recorded_watch_ranks_new_candidates_once_and_news_agrees_with_the_sent_digest() {
    let dir = tempfile::tempdir().unwrap();
    let mut options = options(dir.path());
    std::fs::write(&options.interests, "- Likes: game engine releases.\n").unwrap();
    let sources_file = dir.path().join("sources.toml");
    std::fs::write(&sources_file, "market_move_threshold=0.1\n[[sources]]\nname=\"recorded-release\"\nparser=\"atom\"\nendpoint=\"https://feed.test/releases\"\n[[digest_sources]]\nname=\"recorded-hn\"\nparser=\"hn-front-page\"\nendpoint=\"https://feed.test/hn\"\n").unwrap();
    options.sources = Some(sources_file);
    seed_usage(&options.db);
    let host = Fake::default();
    host.page("https://feed.test/releases", release_feed(false));
    host.page("https://feed.test/hn", r#"{"hits":[]}"#);

    assert_eq!(execute(&host, &options), 0);
    assert_eq!(execute(&host, &options), 0);
    let notes = || count(&options.db, "SELECT count(*) FROM events WHERE kind='note'");
    assert!(host.messages().is_empty() && notes() == 0, "a quiet repeat ranks nothing and sends nothing");

    host.page("https://feed.test/releases", release_feed(true));
    host.page("https://feed.test/hn", json!({"hits": [{"objectID": "scenario-godot-45", "title": "Godot 4.5 released: new rendering, physics, and game development tools", "url": "https://godotengine.org/releases/4.5/"}]}).to_string());
    assert_eq!(execute(&host, &options), 0);
    assert_eq!((host.messages().len(), notes()), (1, 1));
    assert_eq!(count(&options.db, "SELECT count(*) FROM model_events"), 1);
    assert_eq!(std::fs::read_dir(&options.proposals).unwrap().count(), 1);
    assert_eq!(count(&options.db, "SELECT count(*) FROM watch_digest_items"), 2);
    let detail: String = Connection::open(&options.db).unwrap().query_row("SELECT detail FROM model_events", [], |row| row.get(0)).unwrap();
    let detail: Value = serde_json::from_str(&detail).unwrap();
    assert_eq!((detail["digest_status"].as_str(), detail["action"].as_str().map(|text| text.starts_with("new model"))), (Some("complete"), Some(true)));

    let shown = news::render(&[], &host, &options.db).unwrap();
    let message = host.messages()[0].clone();
    let digest_id = "nfa5d14827daffec1";
    let kept = digest::kept(&Connection::open(&options.db).unwrap(), 7, host.now()).unwrap();
    assert_eq!(kept.len(), 2);
    for item in &kept {
        let block = format!("[{}] {}\n{}\n{}", item.id, item.title, item.why, item.url);
        assert!(message.contains(&block) && shown.contains(&block), "digest and news disagree on {}", item.id);
    }
    assert!(kept.iter().any(|item| item.id == digest_id));

    let args = |parts: &[&str]| parts.iter().map(|part| (*part).to_owned()).collect::<Vec<_>>();
    let feedback = args(&["--noise", digest_id, "--feedback", options.feedback.to_str().unwrap()]);
    assert_eq!(news::render(&feedback, &host, &options.db).unwrap(), format!("Recorded noise: {digest_id}\n"));
    assert!(news::render(&args(&["--noise", "missing", "--feedback", options.feedback.to_str().unwrap()]), &host, &options.db).is_err());
    let rows = digest::feedback_rows(&options.feedback).unwrap();
    let similar = "Godot rendering and game development improvements";
    assert!(digest::pre_score("recorded-hn", similar, &rows) < digest::pre_score("recorded-hn", similar, &[]));

    assert_eq!(execute(&host, &options), 0);
    assert_eq!((host.messages().len(), notes()), (1, 1), "an already ranked candidate is never ranked again");
}

fn feedback_pre_scores_only_shared_terms_of_the_same_source_and_stays_inside_the_clamp() {
    let noise = |id: &str, title: &str| json!({"id": id, "source": "hn", "title": title, "feedback": "noise"});
    let more = json!({"id": "c", "source": "hn", "title": "godot rendering", "feedback": "more"});
    assert_eq!(digest::pre_score("hn", "godot rendering news", &[]), 0.0);
    assert!((digest::pre_score("hn", "godot rendering news", &[noise("a", "godot rendering")]) + 0.25).abs() < 1e-9);
    assert!((digest::pre_score("hn", "godot news", &[noise("a", "godot rendering")]) + 0.125).abs() < 1e-9);
    assert_eq!(digest::pre_score("other", "godot rendering news", &[noise("a", "godot rendering")]), 0.0);
    assert!((digest::pre_score("hn", "godot rendering", &[noise("a", "godot rendering"), more.clone()]) - -0.15).abs() < 1e-9);
    assert!((digest::pre_score("hn", "godot rendering", &[noise("a", "godot rendering"), json!({"id": "a", "source": "hn", "title": "godot rendering", "feedback": "more"})]) - 0.1).abs() < 1e-9);
    let many: Vec<Value> = (0..5).map(|index| noise(&format!("n{index}"), "godot rendering")).collect();
    assert_eq!(digest::pre_score("hn", "godot rendering", &many), -0.75);
}

fn watch_candidates_are_added_for_unseen_keys_only_and_keep_a_stable_identity() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(&format!("{SCHEMA}{}", digest::SCHEMA)).unwrap();
    let rows = |keys: &[&str]| -> digest::Rows { keys.iter().map(|key| ((*key).to_owned(), (format!("title {key}"), format!("https://e.org/{key}")))).collect() };
    assert_eq!(digest::observe(&db, "recorded-hn", &rows(&["a"]), NOW).unwrap(), 0, "the first sight is a baseline");
    assert_eq!(digest::observe(&db, "recorded-hn", &rows(&["a", "scenario-godot-45"]), NOW).unwrap(), 1);
    assert_eq!(digest::observe(&db, "recorded-hn", &rows(&["a", "scenario-godot-45"]), NOW).unwrap(), 0);
    let id: String = db.query_row("SELECT id FROM watch_candidates", [], |row| row.get(0)).unwrap();
    assert_eq!(id, "nfa5d14827daffec1");
    let snapshot: i64 = db.query_row("SELECT count(*) FROM watch_snapshot WHERE source='digest:recorded-hn'", [], |row| row.get(0)).unwrap();
    assert_eq!(snapshot, 3, "the baseline marker plus both keys");
}

fn market(p: f64, title: &str) -> Row {
    Row { model: "openai:gpt-7".into(), kind: "market".into(), title: title.into(), at: None, p: Some(p), url: "https://m.test".into(), basis: "b".into() }
}

fn snapshots_report_first_seen_and_changed_rows_and_ignore_small_market_moves() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(SCHEMA).unwrap();
    let one = |key: &str, row: Row| -> super::models::Rows { [(key.to_owned(), row)].into_iter().collect() };
    assert!(persist(&db, "s", &one("k|a", market(0.40, "Will GPT-7 launch")), NOW, 0.10).unwrap().is_empty(), "a first fetch is a baseline");
    assert!(persist(&db, "s", &one("k|a", market(0.45, "Will GPT-7 launch")), NOW, 0.10).unwrap().is_empty(), "a move under the threshold is not news");
    let moved = persist(&db, "s", &one("k|a", market(0.60, "Will GPT-7 launch")), NOW, 0.10).unwrap();
    assert_eq!(moved.iter().map(|event| event.change).collect::<Vec<_>>(), ["changed"]);
    let retitled = persist(&db, "s", &one("k|a", market(0.60, "Will GPT-7 launch soon")), NOW, 0.10).unwrap();
    assert_eq!(retitled.len(), 1, "a changed title is news even at the same probability");
    let mut both = one("k|a", market(0.60, "Will GPT-7 launch soon"));
    both.insert("k|b".into(), market(0.2, "Another"));
    let fresh = persist(&db, "s", &both, NOW, 0.10).unwrap();
    assert_eq!((fresh.len(), fresh[0].change), (1, "first-seen"));
    let total: i64 = db.query_row("SELECT count(*) FROM model_events", [], |row| row.get(0)).unwrap();
    let stored: (String, f64, String) = db.query_row("SELECT model, p, detail FROM model_events WHERE rowid=(SELECT max(rowid) FROM model_events)", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
    assert_eq!((total, stored.0.as_str(), stored.1), (3, "openai:gpt-7", 0.2));
    assert_eq!(serde_json::from_str::<Value>(&stored.2).unwrap()["change"], "first-seen");
    let snapshot: i64 = db.query_row("SELECT count(*) FROM watch_snapshot", [], |row| row.get(0)).unwrap();
    assert_eq!(snapshot, 3);
}

fn alert_config(sources: &[(&str, &str, &str)]) -> Value {
    json!({"alert_sources": sources.iter().map(|(name, parser, endpoint)| json!({"name": name, "parser": parser, "endpoint": endpoint})).collect::<Vec<_>>()})
}

fn travel_feed(guid: &str, level: u8) -> String {
    format!("<rss version=\"2.0\"><channel><item><title>Freedonia - Level {level}: Advisory</title><link>https://travel.state.gov/{guid}</link><guid>{guid}</guid><category domain=\"Country-Tag\">Freedonia</category></item></channel></rss>")
}

#[test]
fn location_alerts_send_once_hold_back_low_advisories_and_retry_after_a_failed_delivery() {
    let dir = tempfile::tempdir().unwrap();
    let options = options(dir.path());
    let host = Fake::default();
    for (endpoint, file) in [("https://usgs.test", "usgs.json"), ("https://gdacs.test", "gdacs.xml"), ("https://travel.test", "travel.xml"), ("https://alerts.example.gov/feed.atom", "weather.atom")] {
        host.page(endpoint, fixture(file));
    }
    let config = alert_config(&[("usgs", "usgs", "https://usgs.test/q"), ("gdacs", "gdacs", "https://gdacs.test/x"), ("travel", "travel", "https://travel.test/x"), ("weather", "weather", "https://weather.test/default")]);
    host.ssh_code.set(255);
    assert_eq!(super::alerts::run(&host, &options, &config), 1, "a failed delivery fails the run");
    assert_eq!(count(&options.db, "SELECT count(*) FROM watch_alerts"), 0, "a failed delivery records nothing, so the next run retries");
    host.ssh_code.set(0);
    assert_eq!(super::alerts::run(&host, &options, &config), 0);
    let sent = host.messages();
    assert_eq!(sent.len(), 5, "{sent:?}");
    assert!(sent.iter().any(|message| message.contains("M 5.2 - 10 km NW of Town") && message.contains("https://earthquake.usgs.gov/e/us1") && message.contains("'Socrates: location warning'") && message.contains("nexus-alert urgent")), "{sent:?}");
    assert_eq!(super::alerts::run(&host, &options, &config), 0);
    assert_eq!(host.messages().len(), 5, "an alert already sent is not sent again");

    let travel = alert_config(&[("travel", "travel", "https://travel.test/x")]);
    let before = host.messages().len();
    for (guid, level, total) in [("adv-a", 1, 0), ("adv-b", 2, 1), ("adv-c", 2, 1), ("adv-d", 3, 2)] {
        host.page("https://travel.test", travel_feed(guid, level));
        assert_eq!(super::alerts::run(&host, &options, &travel), 0);
        assert_eq!(host.messages().len() - before, total, "{guid} at level {level}");
    }
}

#[test]
fn the_earthquake_query_looks_back_one_day_from_now_and_encodes_the_location() {
    let now = Timestamp::parse(NOW).unwrap();
    let mut source = source("usgs", "usgs");
    source.endpoint = "https://earthquake.usgs.gov/q".into();
    let location = location();
    assert!(super::alerts::prepare(&mut source, &location, now));
    assert_eq!(source.endpoint, "https://earthquake.usgs.gov/q?format=geojson&latitude=37.5&longitude=-122.0&maxradiuskm=100.0&minmagnitude=4.5&starttime=2026-10-26T12%3A00%3A00Z");
    let mut bare = Location { weather_feed: None, advisory_country: String::new(), ..location };
    let mut weather = Source { name: "w".into(), parser: "weather".into(), endpoint: "https://x".into() };
    assert!(!super::alerts::prepare(&mut weather, &bare, now), "no runtime weather feed means no weather source");
    let mut travel = Source { name: "t".into(), parser: "travel".into(), endpoint: "https://x".into() };
    assert!(!super::alerts::prepare(&mut travel, &bare, now));
    bare.advisory_country = "Freedonia".into();
    assert!(super::alerts::prepare(&mut travel, &bare, now));
}

fn the_last_digest_is_shown_for_seven_days_only() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(digest::SCHEMA).unwrap();
    db.execute("INSERT INTO watch_digests(id,ts,body,sent) VALUES(1,'2026-10-20T12:00:01Z','b',1)", []).unwrap();
    db.execute("INSERT INTO watch_digest_items VALUES(1,'n1','s','t','https://e.org','w',0.9)", []).unwrap();
    let at = |text: &str| Timestamp::parse(text).unwrap();
    assert_eq!(digest::kept(&db, 7, at("2026-10-27T12:00:00Z")).unwrap().len(), 1);
    assert!(digest::kept(&db, 7, at("2026-10-27T12:00:01Z")).unwrap().is_empty());
}

#[test]
fn feedback_candidates_snapshots_and_the_kept_window_follow_the_python_rules() {
    feedback_pre_scores_only_shared_terms_of_the_same_source_and_stays_inside_the_clamp();
    watch_candidates_are_added_for_unseen_keys_only_and_keep_a_stable_identity();
    snapshots_report_first_seen_and_changed_rows_and_ignore_small_market_moves();
    the_last_digest_is_shown_for_seven_days_only();
}

#[test]
fn the_ranking_runs_on_the_first_listed_claude_account_that_has_a_profile() {
    let profiles = tempfile::tempdir().unwrap();
    for label in ["b", "c"] {
        std::fs::create_dir(profiles.path().join(format!("claude-{label}"))).unwrap();
    }
    let listed = "claude\ta\nclaude\tc\nclaude\tb\ncodex\tpooled\n";
    assert_eq!(super::host::preferred_profile(listed, profiles.path()), Some(profiles.path().join("claude-c")));
    assert_eq!(super::host::preferred_profile("codex\tpooled\n", profiles.path()), None);
}
