use std::cell::RefCell;
use std::path::Path;

use super::model::{Credential, Kind, Renewal};
use super::renew::{Effects, tick};
use super::{DAY, World, registry};

const NOW: u64 = 1_800_000_000;
const SECRET: &str = "sk-ant-oat01-NEVER-PRINT-ME";

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn world(directory: &Path) -> World {
    let south = directory.join("south");
    write(&south.join("secrets/github-app.yaml"), "app_id: ENC[x]\nprivate_key: ENC[y]\nsops:\n    age:\n        - enc: z\n");
    write(&south.join("secrets/nexus/github.yaml"), "nexus-work: ENC[x]\nsops:\n    mac: m\n");
    write(&south.join("secrets/new-thing.yaml"), "token: ENC[x]\nsops:\n    mac: m\n");
    write(
        &south.join("agents/credentials.toml"),
        r#"
[[credential]]
source = "secrets/github-app.yaml"
use = "App key"
kind = "machine"
renewal = "never"

[[credential]]
source = "secrets/nexus/"
use = "fine-grained token"
kind = "human-session"
renewal = "tom: create a new token"
expires = "2027-01-01"
"#,
    );
    let home = directory.join("home");
    write(&home.join("tokens/claude-a"), SECRET);
    write(
        &home.join("profiles/claude-a/.credentials.json"),
        &format!(r#"{{"claudeAiOauth":{{"accessToken":"{SECRET}","refreshToken":"{SECRET}-r","expiresAt":1,"refreshTokenExpiresAt":{}}}}}"#, (NOW + 3 * DAY) * 1000),
    );
    write(&home.join("profiles/claude-b/.credentials.json"), &format!(r#"{{"claudeAiOauth":{{"refreshToken":"{SECRET}-r","refreshTokenExpiresAt":{}}}}}"#, (NOW + 9 * DAY) * 1000));
    let orchestration = directory.join("orchestration.toml");
    write(
        &orchestration,
        &format!(
            r#"
[accounts.claude.profiles.a]
here = {{ path = "{h}/profiles/claude-a", token_file = "{h}/tokens/claude-a" }}
far = {{ path = "~/p", token_file = "~/t" }}

[accounts.claude.profiles.b]
here = {{ path = "{h}/profiles/claude-b" }}
"#,
            h = home.display()
        ),
    );
    World { south, state: directory.join("state"), orchestration, chrome_profile: directory.join("chrome"), host: "here".into(), now: NOW, home }
}

fn named<'a>(credentials: &'a [Credential], name: &str) -> &'a Credential {
    credentials.iter().find(|credential| credential.name == name).unwrap_or_else(|| panic!("no {name} in {:?}", credentials.iter().map(|c| &c.name).collect::<Vec<_>>()))
}

#[test]
fn registry_lists_every_source_by_kind_and_never_prints_a_secret() {
    let directory = tempfile::tempdir().unwrap();
    let world = world(directory.path());
    let credentials = registry::gather(&world).unwrap();
    assert_eq!(named(&credentials, "sops:github-app").kind, Kind::Machine);
    assert_eq!(named(&credentials, "sops:nexus/github").kind, Kind::HumanSession);
    assert_eq!(named(&credentials, "sops:new-thing").kind, Kind::Undeclared);
    let token = named(&credentials, "claude:a:token");
    assert_eq!((token.kind, token.expires.is_some()), (Kind::Machine, true));
    assert!(token.usage.contains("far"), "other hosts are named, not read: {}", token.usage);
    assert_eq!(named(&credentials, "claude:a:login").kind, Kind::HumanSession);
    assert_eq!(named(&credentials, "claude:a:login").expires, Some(NOW + 3 * DAY));
    let shown = registry::lines(&credentials, NOW).join("\n") + &registry::json(&credentials, NOW).to_string();
    assert!(!shown.contains(SECRET), "{shown}");
    check_fails_on_a_human_session_an_undeclared_file_or_an_expired_machine_credential();
}

fn check_fails_on_a_human_session_an_undeclared_file_or_an_expired_machine_credential() {
    let directory = tempfile::tempdir().unwrap();
    let credentials = registry::gather(&world(directory.path())).unwrap();
    for name in ["claude:a:login", "claude:b:login"] {
        assert!(named(&credentials, name).note.as_deref().is_some_and(|note| note.contains("copied login")), "{name}");
    }
    let failing: Vec<&str> = registry::failing(&credentials, NOW).iter().map(|(credential, _)| credential.name.as_str()).collect();
    assert_eq!(failing, ["claude:a:login", "claude:b:login", "sops:new-thing", "sops:nexus/github"]);
    let machine = credentials.iter().filter(|credential| credential.kind == Kind::Machine).cloned().collect::<Vec<_>>();
    assert!(registry::failing(&machine, NOW).is_empty());
    assert_eq!(registry::failing(&machine, NOW + 400 * DAY).len(), 1);
}

#[test]
fn browser_sessions_read_the_cookie_store_expiry() {
    let directory = tempfile::tempdir().unwrap();
    let mut world = world(directory.path());
    let database = directory.path().join("chrome/Default/Cookies");
    std::fs::create_dir_all(database.parent().unwrap()).unwrap();
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection.execute("CREATE TABLE cookies (host_key TEXT, expires_utc INTEGER, has_expires INTEGER)", []).unwrap();
    let chrome_micros = ((NOW + 5 * DAY + 11_644_473_600) * 1_000_000) as i64;
    connection.execute("INSERT INTO cookies VALUES ('.github.com', ?1, 1), ('tracker.example', ?1, 1)", [chrome_micros]).unwrap();
    drop(connection);
    write(
        &world.south.join("agents/credentials.toml"),
        "[[browser_session]]\nhost = \"github.com\"\nuse = \"settings\"\nrenewal = \"tom: sign in\"\n[[browser_session]]\nhost = \"claude.ai\"\nuse = \"usage\"\nrenewal = \"tom: sign in\"\n",
    );
    world.orchestration = directory.path().join("none.toml");
    write(&world.orchestration, "");
    let credentials = registry::gather(&world).unwrap();
    assert_eq!(named(&credentials, "browser:github.com").expires, Some(NOW + 5 * DAY));
    assert_eq!(named(&credentials, "browser:claude.ai").expires, Some(NOW), "no cookie reads as signed out");
    assert!(credentials.iter().all(|credential| credential.name != "browser:tracker.example"));
}

struct Recording {
    ran: RefCell<Vec<Vec<String>>>,
    alerts: RefCell<Vec<String>>,
    renew_ok: bool,
    dry: bool,
}

impl Recording {
    fn new(renew_ok: bool, dry: bool) -> Self {
        Recording { ran: RefCell::default(), alerts: RefCell::default(), renew_ok, dry }
    }
}

impl Effects for Recording {
    fn run(&self, command: &[String]) -> Result<(), String> {
        self.ran.borrow_mut().push(command.to_vec());
        self.renew_ok.then_some(()).ok_or_else(|| "exit 1".into())
    }
    fn alert(&self, text: &str) -> Result<(), String> {
        self.alerts.borrow_mut().push(text.to_owned());
        Ok(())
    }
    fn dry_run(&self) -> bool {
        self.dry
    }
}

fn credential(name: &str, kind: Kind, days: u64, renewal: Renewal) -> Credential {
    Credential { name: name.into(), source: name.into(), usage: "test".into(), kind, fingerprint: None, expires: Some(NOW + days * DAY), renewal, note: None }
}

fn fleet() -> Vec<Credential> {
    vec![
        credential("auto", Kind::Machine, 10, Renewal::Command(vec!["renew-auto".into(), "--now".into()])),
        credential("far", Kind::Machine, 200, Renewal::Command(vec!["renew-far".into()])),
        credential("token", Kind::Machine, 20, Renewal::Tom("setup-token".into())),
        credential("pat", Kind::HumanSession, 25, Renewal::Tom("new token".into())),
        credential("later", Kind::HumanSession, 90, Renewal::Tom("sign in".into())),
    ]
}

#[test]
fn renew_runs_machine_renewals_in_the_window_and_batches_what_needs_tom_into_one_notice() {
    let directory = tempfile::tempdir().unwrap();
    let record = directory.path().join("check.json");
    let effects = Recording::new(true, false);
    let outcome = tick(&fleet(), "here", NOW, 30, &record, &effects);
    assert!(!outcome.failed, "{:?}", outcome.lines);
    assert_eq!(*effects.ran.borrow(), [vec!["renew-auto".to_owned(), "--now".to_owned()]]);
    let alerts = effects.alerts.borrow();
    assert_eq!(alerts.len(), 1, "one batched notice");
    assert!(alerts[0].contains("token (") && alerts[0].contains("pat (") && !alerts[0].contains("later"), "{}", alerts[0]);
    let written: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&record).unwrap()).unwrap();
    assert_eq!((written["renewed"][0].as_str(), written["at"].as_u64()), (Some("auto"), Some(NOW)));

    let again = Recording::new(true, false);
    tick(&fleet(), "here", NOW + DAY, 30, &record, &again);
    assert!(again.alerts.borrow().is_empty(), "the same list inside a week is not sent again");
    let week = Recording::new(true, false);
    tick(&fleet(), "here", NOW + 8 * DAY, 30, &record, &week);
    assert_eq!(week.alerts.borrow().len(), 1, "a week later it is sent again");
    a_failed_renewal_joins_the_notice_and_fails_the_tick();
    dry_run_changes_nothing();
}

fn a_failed_renewal_joins_the_notice_and_fails_the_tick() {
    let directory = tempfile::tempdir().unwrap();
    let effects = Recording::new(false, false);
    let outcome = tick(&fleet(), "here", NOW, 30, &directory.path().join("check.json"), &effects);
    assert!(outcome.failed);
    let alerts = effects.alerts.borrow();
    assert_eq!(alerts.len(), 1);
    assert!(alerts[0].contains("auto (") && alerts[0].contains("token ("), "{}", alerts[0]);
}

fn dry_run_changes_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let record = directory.path().join("check.json");
    let effects = Recording::new(true, true);
    let outcome = tick(&fleet(), "here", NOW, 30, &record, &effects);
    assert!(effects.ran.borrow().is_empty() && effects.alerts.borrow().is_empty() && !record.exists());
    assert!(outcome.lines.iter().any(|line| line.starts_with("would renew auto")), "{:?}", outcome.lines);
}

#[test]
fn declarations_reject_a_renewal_that_is_not_never_tom_or_run() {
    let source = "[[credential]]\nsource = \"secrets/a.yaml\"\nuse = \"x\"\nkind = \"machine\"\nrenewal = \"sometimes\"\n";
    assert!(super::declared::parse(source).unwrap_err().contains("not never"));
}
