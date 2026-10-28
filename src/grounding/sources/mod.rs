use super::*;
use rustix::fs::{FlockOperation, flock};
use store::{Doc, Sqlite, quote};
use crate::threads::catalog::CATALOG;
mod history;
mod issues;
mod policy;
mod repos;
mod settings;
#[cfg(test)]
mod tests;
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
pub fn refresh(only: &[String], force: bool, wait: bool) -> Result<Value> {
    let Some(lock) = refresh_lock(&state_dir(), wait)? else {
        return Ok(json!({"busy":true}));
    };
    let result = (|| {
        let mut db = store::open(false)?;
        let mut counts = serde_json::Map::new();
        let modules = if let Some(dir) = env::var_os("NORTH_GROUNDING_SOURCES") {
            entries(Path::new(&dir))
                .iter()
                .filter_map(|p| {
                    let name = p.file_name()?.to_str()?;
                    if name.ends_with(".ts") && !name.contains(".test.") && !name.starts_with('_') {
                        Some(name.trim_end_matches(".ts").to_owned())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        } else {
            SOURCES.iter().map(|(n, _, _, _)| n.to_string()).collect()
        };
        for name in modules {
            if !only.is_empty() && !only.contains(&name) {
                continue;
            }
            let Some((_, complete, seconds, retention)) =
                SOURCES.iter().find(|(n, _, _, _)| *n == name)
            else {
                continue;
            };
            let started = Timestamp::now().unix_seconds_f64();
            let marks = db.query(&format!(
                "select at,refreshed,rows from watermarks where source={};",
                quote(&name)
            ))?;
            let mark = marks.first();
            if !force && started - mark.map(|r| num(r, "refreshed")).unwrap_or(0.0) <= *seconds {
                counts.insert(
                    name,
                    json!(format!(
                        "fresh {}",
                        mark.map(|r| num(r, "rows") as usize).unwrap_or(0)
                    )),
                );
                continue;
            }
            let since = if *complete || force || mark.is_none() {
                None
            } else {
                mark.map(|r| num(r, "at"))
            };
            let docs = match iter_docs(&name, since) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("grounding: source {name} failed: {e}");
                    continue;
                }
            };
            let mut sql = String::from("BEGIN;\n");
            if since.is_none() {
                sql.push_str(&format!(
                    "delete from docs where source={};\n",
                    quote(&name)
                ));
            }
            let mut newest = mark.map(|r| num(r, "at")).unwrap_or(0.0);
            for d in &docs {
                if since.is_some() {
                    sql.push_str(&format!(
                        "delete from docs where source={} and id={};\n",
                        quote(&d.source),
                        quote(&d.id)
                    ));
                }
                sql.push_str(&format!("insert into docs(source,id,ref,title,text,at,decays) values ({},{},{},{},{},{},{});\n",quote(&d.source),quote(&d.id),quote(&d.reference),quote(&redact(&d.title)),quote(&redact(&cut(&d.text,4000))),d.at,usize::from(d.decays)));
                newest = newest.max(d.at);
            }
            if let Some(days) = retention {
                sql.push_str(&format!(
                    "delete from docs where source={} and at < {};\n",
                    quote(&name),
                    started - days * 86400.0
                ));
            }
            sql.push_str(&format!("insert or replace into watermarks values ({},{},{},(select count(*) from docs where source={}));\nCOMMIT;",quote(&name),newest,started,quote(&name)));
            db.exec(&sql)?;
            counts.insert(name, json!(docs.len()));
        }
        Ok(Value::Object(counts))
    })();
    drop(lock);
    result
}
pub fn spawn_refresh_if_stale() {
    let run = || -> Result<()> {
        let marker = state_dir().join("refresh.started");
        if marker.exists() && Timestamp::now().unix_seconds_f64() - mtime(&marker) < 120.0 {
            return Ok(());
        }
        if store::db_path().exists() {
            let mut db = store::open(true)?;
            let rows = db.query("select min(refreshed) m from watermarks;")?;
            if Timestamp::now().unix_seconds_f64() - rows.first().map(|r| num(r, "m")).unwrap_or(0.0) < 300.0 {
                return Ok(());
            }
        }
        fs::create_dir_all(state_dir())?;
        fs::write(marker, "")?;
        let log = fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(state_dir().join("refresh.log"))?;
        let exe = env::current_exe()?;
        let ledger = state_dir().join("ledger.jsonl");
        let stale = !ledger.exists() || Timestamp::now().unix_seconds_f64() - mtime(&ledger) > 900.0;
        let script = if stale {
            "\"$1\" grounding distill run --no-refresh --if-idle; \"$1\" grounding refresh --if-idle"
        } else {
            "\"$1\" grounding refresh --if-idle"
        };
        let _ = Command::new("nice")
            .args([
                "-n",
                "19",
                "ionice",
                "-c",
                "3",
                "bash",
                "-c",
                script,
                "grounding-refresh",
            ])
            .arg(exe)
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
