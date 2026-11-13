//! A host with no GitHub reader of its own reads the ledger host's mirror: `serve` writes a consistent copy of this
//! host's mirror to stdout, and `pull DEST` asks DEST for it over ssh and replaces this host's mirror with it whole.
use std::fs;
use std::io::Write;
use std::process::Command;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

use crate::platform::process::run_command_within;
use crate::store::Mirror;

const PULL_LIMIT: Duration = Duration::from_secs(60);

pub fn serve(mirror: &Mirror, out: &mut dyn Write) -> Result<(), String> {
    let copy = mirror.path().with_extension(format!("served.{}", std::process::id()));
    let _ = fs::remove_file(&copy);
    let source = Connection::open_with_flags(mirror.path(), OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|error| error.to_string())?;
    source.busy_timeout(Duration::from_secs(10)).map_err(|error| error.to_string())?;
    let copied = source.execute("VACUUM INTO ?1", [copy.to_string_lossy()]).map_err(|error| error.to_string()).and_then(|_| fs::read(&copy).map_err(|error| error.to_string()));
    let _ = fs::remove_file(&copy);
    out.write_all(&copied?).and_then(|()| out.flush()).map_err(|error| error.to_string())
}

pub fn pull(mirror: &Mirror, ssh: Command) -> Result<usize, String> {
    let output = run_command_within(ssh, None, PULL_LIMIT).ok_or("the ledger host did not answer within its bound")?;
    if !output.status.success() {
        return Err(format!("the ledger host refused: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    install(mirror, &output.stdout)
}

pub(crate) fn install(mirror: &Mirror, bytes: &[u8]) -> Result<usize, String> {
    let fresh = mirror.path().with_extension(format!("pulled.{}", std::process::id()));
    if let Some(parent) = mirror.path().parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(&fresh, bytes).map_err(|error| error.to_string())?;
    let rows = Connection::open(&fresh)
        .and_then(|copy| copy.query_row("PRAGMA journal_mode=DELETE", [], |_| Ok(())).and_then(|()| copy.query_row("SELECT count(*) FROM reads", [], |row| row.get::<_, i64>(0))))
        .map_err(|error| format!("the pulled mirror is unreadable: {error}"));
    let installed = rows.and_then(|rows| fs::rename(&fresh, mirror.path()).map(|()| rows as usize).map_err(|error| error.to_string()));
    if installed.is_err() {
        let _ = fs::remove_file(&fresh);
    }
    installed
}

pub fn ssh_to(dest: &str) -> Command {
    let mut command = Command::new("ssh");
    command.args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "-o", "LogLevel=QUIET", "--", dest, "north", "github-mirror", "serve"]);
    command
}

pub fn run(arguments: &[String]) -> Option<u8> {
    let outcome = match arguments {
        [verb] if verb == "serve" => serve(&Mirror::local(), &mut std::io::stdout().lock()).map(|()| None),
        [verb, dest] if verb == "pull" => pull(&Mirror::local(), ssh_to(dest)).map(Some),
        _ => return None,
    };
    Some(match outcome {
        Ok(Some(rows)) => {
            println!("pulled the mirror: {rows} reads");
            0
        }
        Ok(None) => 0,
        Err(why) => {
            eprintln!("north github-mirror: {why}");
            1
        }
    })
}
