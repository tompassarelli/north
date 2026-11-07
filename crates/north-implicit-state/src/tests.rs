use crate::{Config, Finding, Shape, Source, added, scan};

const SNAPSHOT: &str = r#"
pub fn take_turn(snapshot: &Path, force: bool) -> io::Result<bool> {
    let tried = suffixed(snapshot, ".tried");
    let newest = [snapshot, tried.as_path()].iter().filter_map(|path| fs::metadata(path).and_then(|meta| meta.modified()).ok()).max();
    if !force && newest.is_some_and(|modified| {
        SystemTime::now().duration_since(modified).map_or(true, |age| age.as_secs_f64() < SNAPSHOT_SECONDS as f64)
    }) {
        return Ok(false);
    }
    OpenOptions::new().create(true).append(true).open(&tried)?.set_modified(SystemTime::now())?;
    Ok(true)
}

fn sweep(entries: ReadDir, name: &str) {
    for entry in entries.flatten() {
        let file = entry.file_name();
        let Some(pid) = file.to_str().and_then(|file| file.strip_prefix(name)) else { continue };
        if !Path::new("/proc").join(pid).exists() {
            let _ = fs::remove_file(entry.path());
        }
    }
}
"#;

const PAUSE: &str = r#"
use north_base::dispatch_pause::{RECORD, TIMER};

fn record() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state/north").join(RECORD)
}

pub fn standing() -> Option<String> {
    fs::read_to_string(record()).ok().map(|reason| reason.trim().to_owned())
}

fn timer(action: &str) -> u8 {
    match Command::new("systemctl").args(["--user", action, TIMER]).status() {
        Ok(status) if status.success() => 0,
        _ => 1,
    }
}

pub fn pause(reason: &str) -> u8 {
    let path = record();
    if let Err(error) = path.parent().map_or(Ok(()), fs::create_dir_all).and_then(|()| fs::write(&path, reason.trim())) {
        return 1;
    }
    timer("stop")
}

pub fn resume() -> u8 {
    let path = record();
    match fs::remove_file(&path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return 1,
        _ => {}
    }
    timer("start")
}
"#;

const PAUSE_NAMES: &str = "pub const RECORD: &str = \"dispatch-paused\";\npub const TIMER: &str = \"north-dispatch.timer\";\n";

const SPEND: &str = r#"
pub fn mark_spent(fleet: &Fleet, org: &Org, record: &Record, home: &Path) {
    let marks = home.join(".local/state/north/api-exhausted");
    match std::fs::create_dir_all(marks).and_then(|()| std::fs::write(marks.join(&org.id), format!("{} {}\n", record.name, today()))) {
        Ok(()) => {}
        Err(error) => eprintln!("{error}"),
    }
}
"#;

const CREDIT: &str = r#"
fn sources(state: &Path, id: String) -> bool {
    let exhausted = state.join("north/api-exhausted").join(&id).exists();
    exhausted
}
"#;

fn found(files: &[Source], context: &[Source]) -> Vec<(Shape, usize, String)> {
    let mut out: Vec<(Shape, usize, String)> = scan(&Config::default(), files, context).expect("scan").into_iter().map(|Finding { shape, line, item, .. }| (shape, line, item)).collect();
    out.sort();
    out
}

fn source<'a>(path: &'a str, text: &'a str) -> Source<'a> {
    Source { path, text }
}

#[test]
fn replays_the_snapshot_marker_of_2026_11_07() {
    let found = found(&[source("crates/north-threads/src/threads/snapshot.rs", SNAPSHOT)], &[]);
    let shapes: Vec<Shape> = found.iter().map(|(shape, ..)| *shape).collect();
    assert!(found.contains(&(Shape::MarkerStamp, 10, "OpenOptions::new().create(true).append(true).open(&tried)?.set_modified(SystemTime::now())".to_owned())), "{found:?}");
    assert!(found.contains(&(Shape::PhaseSuffix, 3, "\".tried\"".to_owned())), "{found:?}");
    assert!(found.contains(&(Shape::PidLiveness, 18, "Path::new(\"/proc\").join(pid).exists()".to_owned())), "{found:?}");
    assert!(shapes.contains(&Shape::MtimeRead) && !shapes.contains(&Shape::PresenceFlag), "{found:?}");
}

#[test]
fn replays_the_dispatch_pause_record_of_2026_11_07() {
    let names = source("crates/north-base/src/dispatch_pause.rs", PAUSE_NAMES);
    let found = found(&[source("crates/north-dispatch/src/dispatch/pause.rs", PAUSE)], &[names]);
    let debt: Vec<&(Shape, usize, String)> = found.iter().filter(|(shape, ..)| shape.debt()).collect();
    assert_eq!(debt, [&(Shape::PresenceFlag, 9, "dispatch-paused".to_owned()), &(Shape::UnitIntent, 24, "\"stop\"".to_owned())], "{found:?}");
}

#[test]
fn replays_the_api_exhausted_marks_of_2026_11_07() {
    let writer = source("crates/north-providers/src/providers/claude/api_pool.rs", SPEND);
    let found = found(&[source("crates/north-pace/src/credit.rs", CREDIT)], &[writer]);
    assert_eq!(found, [(Shape::PresenceFlag, 3, "api-exhausted/*".to_owned())]);
    let unwritten = self::found(&[source("crates/north-pace/src/credit.rs", CREDIT)], &[]);
    assert_eq!(unwritten, []);
}

#[test]
fn replays_the_fleet_sessions_status_column_and_a_status_compare() {
    let schema = r#"
fn migrate(tx: &Transaction) -> Result<()> {
    tx.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS fleet_sessions(name TEXT PRIMARY KEY, host TEXT NOT NULL, status TEXT NOT NULL, created TEXT NOT NULL) STRICT;
         CREATE TABLE IF NOT EXISTS checked(name TEXT PRIMARY KEY, state TEXT NOT NULL CHECK (state IN ('open', 'shut'))) STRICT;",
    ))
}

fn drain(state: &Laptop, self_: &Host) -> bool {
    state.state == "draining" && self_.jobs() == Some(0) && property(&timer, "ActiveState") == Some("active")
}
"#;
    let found = found(&[source("crates/north-fleet/src/fleet/store/schema.rs", schema)], &[]);
    let status: Vec<&str> = found.iter().filter(|(shape, ..)| *shape == Shape::StatusString).map(|(_, _, item)| item.as_str()).collect();
    assert_eq!(status, ["column status TEXT with no CHECK", "state.state == \"draining\""], "{found:?}");
}

#[test]
fn flags_two_lifecycle_booleans_and_counts_only_added_occurrences() {
    let base = "pub struct Lane {\n    pub name: String,\n    pub landed: bool,\n    pub verbose: bool,\n}\n";
    let head = "pub struct Lane {\n    pub name: String,\n    pub landed: bool,\n    pub verbose: bool,\n    pub is_held: bool,\n}\n";
    let config = Config::default();
    let path = "crates/north-lanes/src/lane.rs";
    let new = added(&config, &[source(path, head)], &[source(path, base)], &[]).expect("added");
    assert_eq!(new, [Finding { shape: Shape::LifecycleBools, path: path.to_owned(), line: 1, item: "Lane { landed, is_held }".to_owned() }]);
    assert_eq!(added(&config, &[source(path, head)], &[source(path, head)], &[]).expect("added"), []);
    let allowed = Config::parse("[[implicit_state.allow]]\nshape = \"lifecycle-bools\"\npath = \"crates/north-lanes/src/lane.rs\"\nreason = \"a lane's flags are read together by one owner\"\n").expect("config");
    assert_eq!(added(&allowed, &[source(path, head)], &[source(path, base)], &[]).expect("added"), []);
}
