//! `north runtime lag --processes`: each long-lived north process on this host, the runtime it runs and whether that
//! is the runtime its link names (docs/rationale/runtime-follow.md); `--json`: for every declared host and process, the
//! seconds it has run older than the runtime the settling host pinned (docs/rationale/propagation.md).
mod ask;
mod behind;
mod page;
pub mod samples;
#[cfg(test)]
mod json_tests;

use super::failure::Failure;
use super::layout::Layout;
use behind::Process;
use north_runtime_follow::{named, runtime_of};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub use page::publish;

const RESIDENTS: &[(&[&str], &str)] = &[
    (&["land", "serve"], "land serve"),
    (&["session-host"], "session-host"),
    (&["session-link"], "session-link"),
    (&["fleet", "watchdog"], "fleet watchdog"),
    (&["capacity", "watchdog", "sample"], "capacity watchdog"),
    (&["capacity", "offload", "take"], "offload taker"),
    (&["gh-broker", "serve"], "gh-broker"),
    (&["broker", "serve"], "broker"),
    (&["--worker-sandbox", "--broker"], "worker-sandbox broker"),
    (&["--tool-server"], "tool-server"),
];
const USER_HZ: u64 = 100;

struct Resident {
    pid: u32,
    kind: &'static str,
    subject: String,
    pages: bool,
    runtime: Option<String>,
    linked: Option<String>,
}

impl Resident {
    fn line(&self) -> String {
        let short = |name: &str| name.get(..10).unwrap_or(name).to_owned();
        let runs = self.runtime.as_deref().map_or_else(|| "an older runtime".to_owned(), short);
        let verdict = match (&self.linked, &self.runtime) {
            (None, _) => "no link".to_owned(),
            (Some(linked), Some(runtime)) if linked == runtime => "current".to_owned(),
            (Some(linked), _) => format!("behind {}", short(linked)),
        };
        format!("{} {}{} runs {runs}: {verdict}", self.pid, self.kind, self.subject)
    }
}

pub(super) fn command(layout: &Layout, flags: &[String]) -> Option<Result<(), Failure>> {
    let flags: Vec<&str> = flags.iter().map(String::as_str).collect();
    match flags.as_slice() {
        ["--processes"] => print!("{}", report(Path::new("/proc"), &layout.state)),
        ["--processes", "--json"] => println!("{}", serde_json::to_string(&processes(Path::new("/proc"), &layout.state)).unwrap_or_default()),
        ["--json"] => println!("{:#}", serde_json::Value::Array(page::objects(layout, &[]))),
        ["--sample"] => return Some(page::sample(layout)),
        _ => return None,
    }
    Some(Ok(()))
}

fn report(proc_root: &Path, state: &Path) -> String {
    residents(proc_root, state, wall_clock()).iter().map(|resident| resident.line() + "\n").collect()
}

fn processes(proc_root: &Path, state: &Path) -> Vec<Process> {
    residents(proc_root, state, wall_clock()).into_iter().filter(|resident| resident.pages).map(|resident| Process { pid: resident.pid, kind: resident.kind.to_owned(), process: format!("{}{}", resident.kind, resident.subject), runtime: resident.runtime }).collect()
}

fn residents(proc_root: &Path, state: &Path, now: u64) -> Vec<Resident> {
    let lander = state.join("lander");
    let lander = if fs::symlink_metadata(&lander).is_ok() { lander } else { state.join("current") };
    let current = state.join("current");
    let boot = boot_seconds(proc_root);
    let mut residents: Vec<Resident> = fs::read_dir(proc_root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let pid = entry.file_name().to_str()?.parse::<u32>().ok()?;
            resident(&entry.path(), pid, boot, now, [lander.as_path(), current.as_path()])
        })
        .collect();
    residents.sort_by(|left, right| (left.kind, left.pid).cmp(&(right.kind, right.pid)));
    residents
}

fn resident(process: &Path, pid: u32, boot: Option<u64>, now: u64, [lander, current]: [&Path; 2]) -> Option<Resident> {
    if fs::read_to_string(process.join("comm")).ok()?.trim() != "north" {
        return None;
    }
    let cmdline = fs::read(process.join("cmdline")).ok()?;
    let argv: Vec<String> = cmdline.split(|byte| *byte == 0).filter(|word| !word.is_empty()).map(|word| String::from_utf8_lossy(word).into_owned()).collect();
    let rest: Vec<&str> = argv.iter().skip(1).map(String::as_str).collect();
    let (words, kind) = RESIDENTS.iter().find(|(words, _)| rest.starts_with(words))?;
    let link = if *kind == "land serve" { lander } else { current };
    let linked = named(link);
    let argument = rest.get(words.len()..).unwrap_or_default().iter().find(|word| !word.starts_with('-')).copied();
    let scratch = *kind == "land serve" && argument.is_some_and(|checkout| scratch_checkout(Path::new(checkout)));
    let young = *kind == "fleet watchdog" && age_seconds(process, boot, now).is_some_and(|age| age < page::PAGE_AFTER_S as u64);
    let subject = argument.map_or_else(String::new, |word| format!(" {word}"));
    let built = |path: &Path| runtime_of(path).map(|(name, _)| name);
    let runtime = argv.first().and_then(|program| built(Path::new(program))).or_else(|| fs::read_link(process.join("exe")).ok().and_then(|exe| built(&exe))).or_else(|| started_after(process, boot, link).then(|| linked.clone()).flatten());
    Some(Resident { pid, kind, subject, pages: !(scratch || young), runtime, linked })
}

/// A checkout a test started a lander in: under the temporary directory or inside a cargo target directory.
fn scratch_checkout(checkout: &Path) -> bool {
    checkout.starts_with("/tmp") || checkout.starts_with(std::env::temp_dir()) || checkout.components().any(|part| part.as_os_str() == "target")
}

fn started_seconds(process: &Path, boot: Option<u64>) -> Option<u64> {
    let ticks = fs::read_to_string(process.join("stat")).ok().and_then(|stat| stat.rsplit_once(')')?.1.split_whitespace().nth(19)?.parse::<u64>().ok())?;
    Some(boot? + ticks / USER_HZ)
}
fn wall_clock() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}


fn age_seconds(process: &Path, boot: Option<u64>, now: u64) -> Option<u64> {
    Some(now.saturating_sub(started_seconds(process, boot)?))
}

/// Whether a process started after its link last moved, so the link resolved to the runtime it names when it ran.
fn started_after(process: &Path, boot: Option<u64>, link: &Path) -> bool {
    let moved = fs::symlink_metadata(link).and_then(|link| link.modified()).ok().and_then(|at| at.duration_since(UNIX_EPOCH).ok());
    match (started_seconds(process, boot), moved) {
        (Some(started), Some(moved)) => started >= moved.as_secs(),
        _ => false,
    }
}

fn boot_seconds(proc_root: &Path) -> Option<u64> {
    fs::read_to_string(proc_root.join("stat")).ok()?.lines().find_map(|line| line.strip_prefix("btime ")?.trim().parse().ok())
}

#[cfg(test)]
mod tests {
    use super::{report, residents};
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::Path;

    const STALE: &str = "8917932b11111111111111111111111111111111";
    const LIVE: &str = "9a896708b01683bd4523100a6c79fa0562e34aac";

    fn process(proc: &Path, pid: u32, argv: &[&str]) {
        let dir = proc.join(pid.to_string());
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("comm"), "north\n").unwrap();
        fs::write(dir.join("cmdline"), argv.iter().map(|word| format!("{word}\0")).collect::<String>()).unwrap();
    }

    #[test]
    fn a_session_host_and_its_watchdog_on_a_runtime_two_promotions_old_read_behind() {
        let root = tempfile::tempdir().unwrap();
        let (proc, state) = (root.path().join("proc"), root.path().join("state/north"));
        fs::create_dir_all(state.join("runtimes")).unwrap();
        symlink(format!("runtimes/{LIVE}"), state.join("current")).unwrap();
        let north = |sha: &str| state.join("runtimes").join(sha).join("north").display().to_string();
        process(&proc, 11, &[&north(STALE), "session-host", "--name", "worker-1", "--generation", "1", "--", "claude"]);
        process(&proc, 12, &[&north(STALE), "fleet", "watchdog", "worker-1"]);
        process(&proc, 13, &[&north(LIVE), "fleet", "watchdog", "worker-2"]);
        process(&proc, 14, &[&north(STALE), "capacity", "host", "run"]);
        assert_eq!(
            report(&proc, &state),
            "12 fleet watchdog worker-1 runs 8917932b11: behind 9a896708b0\n13 fleet watchdog worker-2 runs 9a896708b0: current\n11 session-host worker-1 runs 8917932b11: behind 9a896708b0\n"
        );
    }

    #[test]
    fn a_scratch_lander_and_a_young_watchdog_are_not_paged_while_a_host_lander_is() {
        const NOW: u64 = 2_000_000_000;
        const SERVE: &str = "land serve";
        let root = tempfile::tempdir().unwrap();
        let (proc, state) = (root.path().join("proc"), root.path().join("state/north"));
        fs::create_dir_all(state.join("runtimes")).unwrap();
        symlink(format!("runtimes/{LIVE}"), state.join("current")).unwrap();
        fs::write(proc.join("stat"), format!("btime {}\n", NOW - 1000)).unwrap_or_else(|_| {
            fs::create_dir_all(&proc).unwrap();
            fs::write(proc.join("stat"), format!("btime {}\n", NOW - 1000)).unwrap();
        });
        let started = |pid: u32, seconds_after_boot: u64| {
            fs::write(proc.join(pid.to_string()).join("stat"), format!("{pid} (north) S {}{}\n", "0 ".repeat(18), seconds_after_boot * 100)).unwrap();
        };
        let north = state.join("runtimes").join(LIVE).join("north").display().to_string();
        let target = root.path().join("target/tmp/hand-in/home/code/project/main").display().to_string();
        let launch = |pid: u32, words: &str| process(&proc, pid, &std::iter::once(north.as_str()).chain(words.split(' ')).collect::<Vec<_>>());
        launch(21, &format!("{SERVE} /tmp/.tmpKX0cAd/proj/main"));
        launch(22, &format!("{SERVE} {target}"));
        launch(23, &format!("{SERVE} /home/tom/code/project/main"));
        launch(24, "fleet watchdog young");
        launch(25, "fleet watchdog old");
        started(24, 990);
        started(25, 10);
        let residents = residents(&proc, &state, NOW);
        let paged: Vec<String> = residents.iter().filter(|resident| resident.pages).map(|resident| format!("{}{}", resident.kind, resident.subject)).collect();
        assert_eq!(paged, ["fleet watchdog old", "land serve /home/tom/code/project/main"]);
        assert_eq!(residents.len(), 5);
    }
}
