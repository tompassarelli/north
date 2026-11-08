use super::*;
use crate::foundation::redact::redact;
use rustix::fs::{FlockOperation, flock};
use store::Doc;
mod refresh;
pub use refresh::{import, refresh};
use crate::threads::catalog::CATALOG;
mod history;
mod issues;
mod policy;
mod repos;
mod settings;
fn doc(
    source: &str,
    id: String,
    reference: String,
    title: String,
    text: String,
    at: f64,
    decays: bool,
) -> Doc {
    Doc {
        source: source.into(),
        id,
        reference,
        title,
        text: text.trim().into(),
        at,
        decays,
    }
}
fn entries(path: &Path) -> Vec<PathBuf> {
    let mut files: Vec<_> = fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect();
    files.sort();
    files
}
fn clipped(text: &str, limit: usize) -> String {
    if ulen(text) <= limit {
        text.into()
    } else {
        format!("{}…", cut(text, limit - 1))
    }
}
fn tracked(repo: &Path, prefix: &str) -> Vec<PathBuf> {
    command(
        "git",
        &[
            "-C",
            &repo.to_string_lossy(),
            "ls-files",
            "-z",
            "--",
            prefix,
        ],
    )
    .unwrap_or_default()
    .split('\0')
    .filter(|p| !p.is_empty())
    .map(|p| repo.join(p))
    .collect()
}
const SOURCES: [(&str, bool, f64, Option<f64>); 9] = [
    ("builds", false, 600.0, Some(90.0)),
    ("commits", false, 900.0, Some(90.0)),
    ("github", false, 1800.0, None),
    ("ledger", true, 120.0, None),
    ("policy", true, 900.0, None),
    ("projects", true, 1800.0, None),
    ("settings", true, 900.0, None),
    ("threads", true, 900.0, None),
    ("transcripts", false, 300.0, Some(14.0)),
];
fn iter_docs(name: &str, since: Option<f64>) -> Result<Vec<Doc>> {
    match name {
        "builds" => history::builds(since),
        "commits" => history::commits(
            since,
            &env_path("NORTH_GROUNDING_CODE", home().join("code")),
        ),
        "github" => issues::github(since),
        "ledger" => Ok(ledger()),
        "policy" => Ok(policy::iter_docs()),
        "projects" => Ok(projects()),
        "settings" => Ok(settings::iter_docs()),
        "threads" => issues::threads(
            since,
            &issues::ledger_path(),
            env::var_os("THREADS_NO_NATIVE").is_none()
                && env::var_os("CLAUDE_CODE_REMOTE").is_none(),
        ),
        "transcripts" => history::transcripts(since, &convo_path()),
        _ => Err(format!("unknown grounding source {name}").into()),
    }
}
fn refresh_lock(dir: &Path, wait: bool) -> Result<Option<fs::File>> {
    fs::create_dir_all(dir)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("refresh.lock"))?;
    let operation = if wait {
        FlockOperation::LockExclusive
    } else {
        FlockOperation::NonBlockingLockExclusive
    };
    match flock(&lock, operation) {
        Ok(()) => Ok(Some(lock)),
        Err(rustix::io::Errno::WOULDBLOCK) => Ok(None),
        Err(e) => Err(std::io::Error::from(e).into()),
    }
}
pub fn spawn_refresh_if_stale() {
    let run = || -> Result<()> {
        let marker = state_dir().join("refresh.started");
        if marker.exists() && Timestamp::now().unix_seconds_f64() - mtime(&marker) < 120.0 {
            return Ok(());
        }
        let marks = refresh::marks();
        let oldest = marks.values().map(|m| m.refreshed).fold(f64::INFINITY, f64::min);
        if !marks.is_empty() && Timestamp::now().unix_seconds_f64() - oldest < 300.0 {
            return Ok(());
        }
        fs::create_dir_all(state_dir())?;
        fs::write(marker, "")?;
        let log = fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(state_dir().join("refresh.log"))?;
        let exe = env::current_exe()?;
        let _ = Command::new("nice")
            .args(["-n", "19", "ionice", "-c", "3"])
            .arg(exe)
            .args(["graph", "ground", "refresh", "--if-idle"])
            .stdin(std::process::Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .spawn()?;
        Ok(())
    };
    let _ = run();
}
fn ledger() -> Vec<Doc> {
    let path = env_path("NORTH_GROUNDING_LEDGER", state_dir().join("ledger.jsonl"));
    read(&path)
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|r| {
            r["superseded_by"].as_str().unwrap_or("").is_empty()
                && !s(r, "quote").is_empty()
                && !s(r, "id").is_empty()
        })
        .map(|r| {
            let source = s(&r, "source");
            let speaker = source.split_whitespace().next().unwrap_or("tom");
            let id = s(&r, "id");
            doc(
                "ledger",
                id.clone(),
                format!("ledger:{id}"),
                format!(
                    "{speaker} {} {} {} ({source})",
                    r["kind"].as_str().unwrap_or("decision"),
                    cut(&s(&r, "at"), 16),
                    s(&r, "topic")
                ),
                s(&r, "quote"),
                epoch(&s(&r, "at")),
                false,
            )
        })
        .collect()
}
fn projects() -> Vec<Doc> {
    let mut out = Vec::new();
    for main in repos::owned_mains(&env_path("NORTH_GROUNDING_CODE", home().join("code"))) {
        let name = main
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy();
        if name == "north" || name == "south" {
            continue;
        }
        let mut paths = tracked(&main, "docs");
        paths.extend([main.join("README.md"), main.join("AGENTS.md")]);
        for path in paths {
            if path.extension().is_some_and(|s| s == "md") && path.exists() {
                out.extend(policy::markdown_docs(
                    &path,
                    &format!(
                        "{name}:{}",
                        path.strip_prefix(&main).unwrap_or(&path).display()
                    ),
                    mtime(&path),
                    false,
                    "projects",
                ));
            }
        }
    }
    out
}
struct Db<'a> {
    path: &'a Path,
    busy: std::time::Duration,
}
impl Db<'_> {
    fn query(&self, sql: &str) -> Result<Vec<Value>> {
        Ok(crate::platform::sqlite::rows(self.path, crate::platform::sqlite::Access::ReadOnly, self.busy, sql)?)
    }
}
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\0', "").replace('\'', "''"))
}
