use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use north_base::foundation::exit::SLOTS_BUSY;
use north_base::foundation::offload;

use super::line_wait::{Line, LineClass, LineJob, deadline, expected_start, refuses_at_once};

pub const USAGE: &str = "usage: north capacity offload line wait --rank N [--name CHECKOUT] [--ticket T] [--slots N] [--within SECONDS] (prints `expected wait N s` and exits 0, or exits 96 when a landing or ordinary check would wait past its rank's deadline, or past SECONDS when that is sooner; CHECKOUT counts that checkout's slots and jobs, else the host's, and the asking job's own ticket T is not ahead of it) | line record queued|started|ended --ticket T --rank N [--check NAME] [--class land|background] [--exit CODE] [--deadline SECONDS] (appends one line to this host's slot line record) | line report [--window MIN] (prints the placement-to-start wait p50 and p90 of the jobs that started in the last MIN minutes, 60 by default)";

const RECORD: &str = "line.record";
const PREVIOUS_RECORD: &str = "previous-line.record";
const QUEUE: &str = "queue";
const ROTATE_BYTES: u64 = 4 << 20;
const HISTORY_SECONDS: u64 = 24 * 3600;
const BACKGROUND_SUFFIX: &str = ".background";
const WARM: &str = "warm";
const SETTING: &str = "capacity_line_wait";

pub fn run(words: &[&str]) -> u8 {
    let Some(root) = offload::root() else {
        eprintln!("north capacity offload line: no NORTH_OFFLOAD_ROOT and no HOME");
        return 2;
    };
    match words {
        ["wait", rest @ ..] => options(rest).and_then(|options| Ask::of(&options)).map_or_else(usage, |ask| {
            let (code, answer) = wait(&root, &ask, enabled(), now());
            println!("{answer}");
            code
        }),
        ["record", event @ ("queued" | "started" | "ended"), rest @ ..] => options(rest).and_then(|options| Event::of(event, &options)).map_or_else(usage, |event| record(&root, &event, now())),
        ["report", rest @ ..] => options(rest).and_then(|options| options.number_or("--window")).map_or_else(usage, |window| report(&root, window.unwrap_or(60), now())),
        _ => usage(),
    }
}

pub fn or_offload(arguments: &[String], offload: fn(&[String]) -> u8) -> u8 {
    match arguments.split_first() {
        Some((verb, rest)) if verb == "line" => run(&rest.iter().map(String::as_str).collect::<Vec<_>>()),
        _ => offload(arguments),
    }
}

fn usage() -> u8 {
    eprintln!("{USAGE}");
    2
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

fn enabled() -> bool {
    north_base::platform::harness::document(north_base::platform::harness::DEFAULTS).get(SETTING).and_then(|entry| entry.get("value")).and_then(|value| value.as_bool()).unwrap_or(true)
}

struct Options<'a>(BTreeMap<&'a str, &'a str>);

fn options<'a>(words: &[&'a str]) -> Option<Options<'a>> {
    let pairs = words.chunks(2).map(|pair| match pair {
        [flag, value] if flag.starts_with("--") => Some((*flag, *value)),
        _ => None,
    });
    pairs.collect::<Option<BTreeMap<_, _>>>().map(Options)
}

impl<'a> Options<'a> {
    fn number<T: std::str::FromStr>(&self, flag: &str) -> Option<T> {
        self.0.get(flag)?.parse().ok()
    }

    fn number_or<T: std::str::FromStr>(&self, flag: &str) -> Option<Option<T>> {
        self.0.get(flag).map_or(Some(None), |value| value.parse().ok().map(Some))
    }

    fn file_name(&self, flag: &str) -> Option<&'a str> {
        Path::new(*self.0.get(flag)?).file_name()?.to_str()
    }
}

struct Ask<'a> {
    rank: u8,
    slots: Option<u32>,
    ticket: Option<&'a str>,
    checkout: Option<&'a str>,
    host_cap: Option<usize>,
    within: Option<u64>,
}

impl<'a> Ask<'a> {
    fn of(options: &Options<'a>) -> Option<Self> {
        Some(Self { rank: options.number("--rank")?, slots: options.number_or("--slots")?, ticket: options.file_name("--ticket"), checkout: options.0.get("--name").copied(), within: options.number_or("--within")?, host_cap: std::env::var("NORTH_OFFLOAD_MAX_JOBS").ok().and_then(|cap| cap.parse().ok()) })
    }

    fn class(&self) -> LineClass {
        if self.rank >= 4 { LineClass::Background } else { LineClass::LandCheck }
    }
}

fn wait(root: &Path, ask: &Ask<'_>, enabled: bool, now: u64) -> (u8, String) {
    let admit = |seconds: u64| (0, format!("expected wait {seconds} s"));
    if !enabled {
        return admit(0);
    }
    let Ok(text) = fs::read_to_string(root.join(RECORD)) else { return admit(0) };
    let tickets: Vec<String> = live_tickets(&root.join(QUEUE)).into_iter().filter(|name| Some(name.as_str()) != ask.ticket).collect();
    let slots = ask.slots.unwrap_or_else(|| slots_here(root, ask));
    let line = line_of(slots, &tickets, &text, ask.class(), ask.checkout, now);
    let seconds = expected_start(&line, ask.rank, now);
    let limit = ask.within.map_or(deadline(ask.rank), |within| within.min(deadline(ask.rank)));
    if refuses_at_once(ask.rank) && seconds > limit {
        return (SLOTS_BUSY, format!("expected wait {seconds} s, over the rank's deadline {limit} s"));
    }
    admit(seconds)
}

fn slots_here(root: &Path, ask: &Ask<'_>) -> u32 {
    let class = ask.class();
    let stems = fs::read_dir(root).into_iter().flatten().flatten().filter_map(|entry| entry.file_name().into_string().ok()?.strip_suffix(".lock").map(str::to_owned));
    let counted = stems.filter(|stem| stem != WARM && is_slot(stem, class, ask.checkout)).count();
    let cap = ask.host_cap.filter(|_| class == LineClass::LandCheck);
    u32::try_from(cap.map_or(counted, |cap| counted.min(cap)).max(1)).unwrap_or(u32::MAX)
}

fn is_slot(stem: &str, class: LineClass, checkout: Option<&str>) -> bool {
    let (owner, background) = stem.strip_suffix(BACKGROUND_SUFFIX).map_or((stem, false), |owner| (owner, true));
    let indexed = |name: &str| owner == name || owner.strip_prefix(name).and_then(|rest| rest.strip_prefix('-')).is_some_and(|index| !index.is_empty() && index.bytes().all(|byte| byte.is_ascii_digit()));
    background == (class == LineClass::Background) && checkout.is_none_or(indexed)
}

fn live_tickets(queue: &Path) -> Vec<String> {
    let names = fs::read_dir(queue).into_iter().flatten().flatten().filter_map(|entry| entry.file_name().into_string().ok());
    names.filter(|name| held(&queue.join(name))).collect()
}

fn held(ticket: &Path) -> bool {
    use rustix::fs::{FlockOperation, flock};
    fs::File::open(ticket).is_ok_and(|file| flock(&file, FlockOperation::NonBlockingLockShared).is_err())
}

struct Ticket {
    rank: u8,
    queued_at: u64,
}

fn ticket(name: &str) -> Option<Ticket> {
    let (rank, rest) = name.split_once('-')?;
    let (nanos, _) = rest.split_once('-')?;
    Some(Ticket { rank: rank.parse().ok()?, queued_at: nanos.parse::<u64>().ok()? / 1_000_000_000 })
}

#[derive(Default)]
struct Entry {
    rank: u8,
    check: String,
    class: Option<LineClass>,
    queued_at: Option<u64>,
    started_at: Option<u64>,
    ended_at: Option<u64>,
    deadline: Option<u64>,
}

fn entries(text: &str) -> BTreeMap<String, Entry> {
    let mut entries: BTreeMap<String, Entry> = BTreeMap::new();
    for fields in text.lines().map(|line| line.split('\t').collect::<Vec<_>>()) {
        let deadline = fields.get(7).and_then(|seconds| seconds.parse::<u64>().ok());
        let [event, at, rank, class, _exit, ticket, check] = fields.get(..7).filter(|_| fields.len() <= 8).unwrap_or_default() else { continue };
        let (Ok(at), Ok(rank)) = (at.parse::<u64>(), rank.parse::<u8>()) else { continue };
        let entry = entries.entry((*ticket).to_owned()).or_default();
        entry.rank = rank;
        entry.check = (*check).to_owned();
        entry.class = Some(if *class == "background" { LineClass::Background } else { LineClass::LandCheck });
        match *event {
            "queued" => entry.queued_at = Some(at),
            "started" => {
                entry.started_at = Some(at);
                entry.deadline = deadline;
            }
            "ended" => entry.ended_at = Some(at),
            _ => {}
        }
    }
    entries
}

fn queued(tickets: &[String], entries: &mut BTreeMap<String, Entry>, ours: &impl Fn(&str) -> bool) -> Vec<LineJob> {
    let mut jobs = Vec::new();
    for name in tickets {
        let Some(ticket) = ticket(name) else { continue };
        if !ours(name) || entries.get(name).is_some_and(|entry| entry.started_at.is_some() || entry.ended_at.is_some()) {
            continue;
        }
        let entry = entries.remove(name).unwrap_or_default();
        let job_class = entry.class.unwrap_or(if ticket.rank >= 4 { LineClass::Background } else { LineClass::LandCheck });
        jobs.push(LineJob { rank: ticket.rank, check: entry.check, class: job_class, queued_at: ticket.queued_at, started_at: None, ended_at: None, deadline: None });
    }
    jobs
}

fn line_of(slots: u32, tickets: &[String], text: &str, class: LineClass, checkout: Option<&str>, now: u64) -> Line {
    let ours = |name: &str| checkout.is_none_or(|checkout| name.rsplit_once('@').is_some_and(|(_, owner)| owner == checkout));
    let mut entries = entries(text);
    let mut jobs = queued(tickets, &mut entries, &ours);
    let recorded = entries.into_iter().filter_map(|(name, entry)| {
        let started_at = entry.started_at?;
        let current = match entry.ended_at {
            Some(ended_at) => now.saturating_sub(ended_at) <= HISTORY_SECONDS,
            None => ours(&name) && now.saturating_sub(started_at) <= offload::RUN_SECONDS_CEILING,
        };
        let queued_at = ticket(&name).map(|ticket| ticket.queued_at).or(entry.queued_at)?;
        current.then_some(LineJob { rank: entry.rank, check: entry.check, class: entry.class?, queued_at, started_at: Some(started_at), ended_at: entry.ended_at, deadline: entry.deadline })
    });
    jobs.extend(recorded);
    jobs.retain(|job| job.class == class);
    Line { slots, jobs }
}

struct Event<'a> {
    kind: &'a str,
    ticket: String,
    rank: u8,
    check: String,
    class: &'static str,
    exit: Option<i32>,
    deadline: Option<u64>,
}

impl<'a> Event<'a> {
    fn of(kind: &'a str, options: &Options<'_>) -> Option<Self> {
        let ticket = Path::new(options.0.get("--ticket")?).file_name()?.to_str()?.to_owned();
        let class = match options.0.get("--class").copied() {
            None | Some("land") => "land",
            Some("background") => "background",
            Some(_) => return None,
        };
        let check = options.0.get("--check").map_or("-".to_owned(), |check| check.replace(['\t', '\n', '\r'], " "));
        Some(Self { kind, ticket, rank: options.number("--rank")?, check, class, exit: options.number_or("--exit")?, deadline: options.number_or("--deadline")? })
    }
}

fn record(root: &Path, event: &Event<'_>, now: u64) -> u8 {
    let path = root.join(RECORD);
    if fs::metadata(&path).is_ok_and(|meta| meta.len() > ROTATE_BYTES) {
        let _ = fs::rename(&path, root.join(PREVIOUS_RECORD));
    }
    let exit = event.exit.map_or("-".to_owned(), |code| code.to_string());
    let deadline = event.deadline.map_or(String::new(), |seconds| format!("\t{seconds}"));
    let line = format!("{}\t{now}\t{}\t{}\t{exit}\t{}\t{}{deadline}\n", event.kind, event.rank, event.class, event.ticket, event.check);
    let written = fs::create_dir_all(root).and_then(|()| fs::OpenOptions::new().create(true).append(true).open(&path)).and_then(|mut file| file.write_all(line.as_bytes()));
    match written {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("north capacity offload line: {}: {error}", path.display());
            1
        }
    }
}

fn report(root: &Path, window_minutes: u64, now: u64) -> u8 {
    let paths: [PathBuf; 2] = [root.join(PREVIOUS_RECORD), root.join(RECORD)];
    let text: String = paths.iter().filter_map(|path| fs::read_to_string(path).ok()).collect();
    let since = now.saturating_sub(window_minutes.saturating_mul(60));
    let mut waits: Vec<u64> = entries(&text)
        .into_iter()
        .filter_map(|(name, entry)| {
            let started = entry.started_at.filter(|started| *started >= since)?;
            Some(started.saturating_sub(ticket(&name).map(|ticket| ticket.queued_at).or(entry.queued_at)?))
        })
        .collect();
    waits.sort_unstable();
    let quantile = |share: usize| waits.get((waits.len() * share).div_ceil(100).saturating_sub(1)).copied().unwrap_or(0);
    println!("placement-to-start wait over {window_minutes} min: p50 {} s, p90 {} s ({} jobs)", quantile(50), quantile(90), waits.len());
    0
}

#[cfg(test)]
#[path = "line_tests.rs"]
mod tests;

