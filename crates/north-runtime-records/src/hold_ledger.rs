use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use north_base::platform::finish_record::verdict_store;
use serde::{Deserialize, Serialize};

use crate::failure::{Failure, failed};

pub const VERB: &str = "review-hold";
pub const COORDINATOR: &str = "proxy";
pub const REVIEWER: &str = "reviewer";
const DIR: &str = "holds";
pub const REPOSITORY: &str = "north";
const DESTINATION: &str = "main";
pub const PUSH_LEDGER: &str = "safe-push/pushes.tsv";
const WHY_LIMIT: usize = 2048;
pub const SHOWN_FOR: u64 = 24 * 60 * 60;
const USAGE: &str = "usage: north runtime hold COMMIT [--host HOST] [--until LANE] --why TEXT | north runtime hold --lift COMMIT --why TEXT  place a reviewer's hold on COMMIT and the lane that holds it, which keeps `north runtime order` and the slot and Live-fix promotions from taking live any head that contains COMMIT or a landing of that lane (with --host, only an order that promotes HOST is refused), or lift it; with --until, the hold lifts itself once a head containing a landing of LANE is screened that changes a file COMMIT changes and whose patch the placing session accepted with `north runtime accept`, both read in the trusted mirror; the hold is the calling fleet session's, and only that session lifts it; the coordinator role holds no hold; run it on the host that settles the fleet";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lift {
    pub by: String,
    pub why: String,
    pub at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hold {
    pub commit: String,
    pub lane: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
    pub session: String,
    pub why: String,
    pub at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifted: Option<Lift>,
}

/// The fleet session that asks, as its cgroup scope names it, and whether its record names it the coordinator or the reviewer role.
#[derive(Debug, PartialEq, Eq)]
pub struct Caller {
    pub session: String,
    pub coordinator: bool,
    pub reviewer: bool,
}

/// What `north runtime hold` asks, in the words the broker takes: `review-hold place COMMIT LANE HOST UNTIL WHY` (HOST empty for every host, UNTIL empty for a hold only its placer lifts) or `review-hold lift COMMIT WHY`.
#[derive(Debug, PartialEq, Eq)]
pub enum Ask {
    Place { commit: String, lane: String, host: Option<String>, until: Option<String>, why: String },
    Lift { commit: String, why: String },
}

pub fn full_sha(commit: &str) -> bool {
    matches!(commit.len(), 40 | 64) && commit.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn named(word: &str) -> bool {
    !word.is_empty() && word.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/'))
}

fn said(why: &str) -> bool {
    !why.trim().is_empty() && why.len() <= WHY_LIMIT
}

impl Ask {
    pub fn words(&self) -> Vec<String> {
        match self {
            Ask::Place { commit, lane, host, until, why } => [VERB, "place", commit, lane, host.as_deref().unwrap_or_default(), until.as_deref().unwrap_or_default(), why].map(str::to_owned).to_vec(),
            Ask::Lift { commit, why } => [VERB, "lift", commit, why].map(str::to_owned).to_vec(),
        }
    }

    /// The hold request in `arguments`: `None` for any other request, and why one in other words is refused.
    pub fn asked(arguments: &[String]) -> Option<Result<Self, String>> {
        let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
        let [VERB, rest @ ..] = words.as_slice() else { return None };
        let given = |word: &str| Some(word.to_owned()).filter(|word| !word.is_empty());
        Some(match rest {
            ["place", commit, lane, on, until, why] if full_sha(commit) && named(lane) && [on, until].iter().all(|word| word.is_empty() || named(word)) && said(why) => Ok(Ask::Place { commit: (*commit).to_owned(), lane: (*lane).to_owned(), host: given(on), until: given(until), why: why.trim().to_owned() }),
            ["lift", commit, why] if full_sha(commit) && said(why) => Ok(Ask::Lift { commit: (*commit).to_owned(), why: why.trim().to_owned() }),
            _ => Err(format!("a hold request is `{VERB} place COMMIT LANE HOST UNTIL WHY` or `{VERB} lift COMMIT WHY`, with a full commit id and a reason of at most {WHY_LIMIT} bytes")),
        })
    }
}

pub fn state_home() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"), PathBuf::from)
}

/// The text of `path`, empty when it does not exist yet; any other failure to read it is an error naming it.
fn read_or_empty(path: &Path) -> Result<String, String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(format!("{} cannot be read: {error}", path.display())),
    }
}

/// A reviewer's hold that stands, as `north holds` lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Standing {
    pub commit: String,
    pub lane: String,
    pub host: Option<String>,
    pub until: Option<String>,
    pub session: String,
    pub why: String,
    pub at: u64,
}

/// Every hold in the verdict store `environment` and `home` name that is not lifted, oldest first, or why the store cannot be read.
pub fn standing(environment: &[(String, String)], home: &Path) -> Result<Vec<Standing>, String> {
    let holds = Holds::of(environment, home).all()?;
    Ok(holds.into_iter().filter(|hold| hold.lifted.is_none()).map(|Hold { commit, lane, host, until, session, why, at, .. }| Standing { commit, lane, host, until, session, why, at }).collect())
}

pub struct Holds(pub PathBuf);

impl Holds {
    pub fn here() -> Self {
        let environment: Vec<(String, String)> = ["NORTH_VERDICT_CACHE_DIR", "XDG_STATE_HOME"].into_iter().filter_map(|name| Some((name.to_owned(), std::env::var(name).ok()?))).collect();
        Self::of(&environment, &PathBuf::from(std::env::var_os("HOME").unwrap_or_default()))
    }

    pub fn of(environment: &[(String, String)], home: &Path) -> Self {
        Self(verdict_store(environment, home).join(DIR))
    }

    fn file(hold: &Hold) -> String {
        format!("{}.{}.json", hold.commit, hold.session)
    }

    /// Every hold in the store, or an error naming what could not be read: a store that exists and cannot be listed, or an entry that cannot be read, does not parse as a hold, or is not named for the hold it holds.
    pub fn all(&self) -> Result<Vec<Hold>, String> {
        let entries = match fs::read_dir(&self.0) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(format!("the holds in {} cannot be listed: {error}", self.0.display())),
        };
        let mut holds = Vec::new();
        for entry in entries {
            let path = entry.map_err(|error| format!("an entry of {} cannot be read: {error}", self.0.display()))?.path();
            let text = fs::read_to_string(&path).map_err(|error| format!("{} cannot be read: {error}", path.display()))?;
            let hold = serde_json::from_str::<Hold>(&text).map_err(|error| format!("{} is no hold: {error}", path.display()))?;
            if path.file_name().is_none_or(|name| *name != *Self::file(&hold)) {
                return Err(format!("{} holds {} for {}, which is not the hold its name says", path.display(), hold.commit, hold.session));
            }
            holds.push(hold);
        }
        holds.sort_by_key(|hold| hold.at);
        Ok(holds)
    }

    pub fn write(&self, hold: &Hold) -> Result<(), String> {
        let failure = |error: std::io::Error| format!("the hold on {} in {}: {error}", hold.commit, self.0.display());
        fs::create_dir_all(&self.0).map_err(failure)?;
        let pending = tempfile::NamedTempFile::new_in(&self.0).map_err(failure)?;
        fs::write(pending.path(), serde_json::to_string(hold).map_err(|error| error.to_string())?).map_err(failure)?;
        pending.persist(self.0.join(Self::file(hold))).map(|_| ()).map_err(|error| failure(error.error))
    }

    fn prune(&self, holds: &[Hold], now: u64) {
        for hold in holds.iter().filter(|hold| hold.lifted.as_ref().is_some_and(|lift| now.saturating_sub(lift.at) >= SHOWN_FOR)) {
            let _ = fs::remove_file(self.0.join(Self::file(hold)));
        }
    }

    /// Places or lifts what `ask` names for `caller` at `now`, and the line that says what was done.
    pub fn serve(&self, ask: Ask, caller: &Caller, now: u64) -> Result<String, String> {
        if !named(&caller.session) || caller.session.contains('/') {
            return Err(format!("{:?} names no fleet session", caller.session));
        }
        if caller.coordinator {
            return Err(format!("{} runs in the coordinator role, which neither places nor lifts a hold: the coordinator's client window runs in no fleet session's scope and cannot, so the coordinator cannot in the screen either", caller.session));
        }
        let holds = self.all()?;
        self.prune(&holds, now);
        match ask {
            Ask::Place { commit, lane, host, until, why } => {
                let on = host.as_deref().map_or(String::new(), |host| format!(" on {host}"));
                let lifts = until.as_deref().map_or(String::new(), |until| format!(" or a head containing a landing of {until} is screened"));
                self.write(&Hold { commit: commit.clone(), lane: lane.clone(), host, until, session: caller.session.clone(), why, at: now, lifted: None })?;
                Ok(format!("north runtime hold: {} holds {commit} of the lane {lane}{on}; no head containing it or a landing of {lane} goes live until {} lifts it{lifts}", caller.session, caller.session))
            }
            Ask::Lift { commit, why } => self.lift(holds, &commit, &why, caller, now),
        }
    }

    fn lift(&self, holds: Vec<Hold>, commit: &str, why: &str, caller: &Caller, now: u64) -> Result<String, String> {
        let standing: Vec<Hold> = holds.into_iter().filter(|hold| hold.commit == commit && hold.lifted.is_none()).collect();
        if standing.is_empty() {
            return Err(format!("no hold stands on {commit}"));
        }
        let (own, others): (Vec<Hold>, Vec<Hold>) = standing.into_iter().partition(|hold| hold.session == caller.session);
        if own.is_empty() {
            let placers: Vec<&str> = others.iter().map(|hold| hold.session.as_str()).collect();
            return Err(format!("{commit} is held by {}; only {} lifts it, not {}", placers.join(", "), placers.join(" or "), caller.session));
        }
        for mut hold in own.iter().cloned() {
            hold.lifted = Some(Lift { by: caller.session.clone(), why: why.to_owned(), at: now });
            self.write(&hold)?;
        }
        let placers: Vec<&str> = own.iter().map(|hold| hold.session.as_str()).collect();
        Ok(format!("north runtime hold: {} lifted the hold of {} on {commit}: {why}", caller.session, placers.join(", ")))
    }
}
/// The lane that holds `commit`: the branch of the push ledger's row whose lane tip or landed commit it is, else the one branch of `branches` (local branches containing it, the destination aside) that holds it; a commit no lane or more than one holds is refused, since its hold would match no landing.
pub fn lane_of(commit: &str, ledger: &str, branches: &[String]) -> Result<String, String> {
    let pushed = ledger.lines().map(|row| row.split('\t').collect::<Vec<_>>()).filter(|fields| fields.get(1) == Some(&REPOSITORY) && (fields.get(3) == Some(&commit) || fields.get(4) == Some(&commit))).filter_map(|fields| fields.get(2).map(|lane| (*lane).to_owned())).next_back();
    let held: Vec<&String> = branches.iter().filter(|branch| *branch != DESTINATION).collect();
    let lane = pushed.or_else(|| match held.as_slice() {
        [only] => Some((*only).clone()),
        _ => None,
    });
    match lane {
        Some(lane) if named(&lane) => Ok(lane),
        _ => Err(format!("{commit} is in no one lane: no landing names it and the branches holding it here are [{}]; hold a commit of the lane under review", held.iter().map(|branch| branch.as_str()).collect::<Vec<_>>().join(", "))),
    }
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs())
}
/// `north runtime hold`, as `command` below, with its exit code; a failure is printed as the runtime verbs print theirs.
pub fn run(flags: &[String], settles: bool, serve: impl FnOnce(Ask) -> Result<String, String>) -> u8 {
    command(flags, settles, serve).map_or_else(
        |failure| {
            eprintln!("north runtime hold: {}", failure.message);
            failure.code
        },
        |()| 0,
    )
}

pub fn git(arguments: &[&str]) -> Result<String, Failure> {
    let output = north_base::platform::process::run("git", None, arguments).map_err(|error| failed(1, format!("git {}: {error}", arguments.join(" "))))?;
    match output.status.success() {
        true => Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned()),
        false => Err(failed(1, format!("git {}: {}", arguments.join(" "), String::from_utf8_lossy(&output.stderr).trim()))),
    }
}

/// The lane that holds `commit` here, read from this host's push ledger and the local branches containing it.
fn lane_here(commit: &str) -> Result<String, Failure> {
    let ledger = read_or_empty(&state_home().join(PUSH_LEDGER)).map_err(|why| failed(1, why))?;
    let branches = git(&["for-each-ref", "--contains", commit, "--format=%(refname:short)", "refs/heads"])?;
    lane_of(commit, &ledger, &branches.lines().map(str::to_owned).collect::<Vec<_>>()).map_err(|why| failed(1, why))
}

/// `north runtime hold` on a host that `settles` the fleet: resolves the commit and its lane in the working directory and has `serve` place or lift the hold for the caller's own session (worker_sandbox/hold_client.rs: through the broker inside a worker sandbox and here outside one), printing the line it says.
fn command(flags: &[String], settles: bool, serve: impl FnOnce(Ask) -> Result<String, String>) -> Result<(), Failure> {
    let Flags { commit: named, lift, host, until, why } = parse(flags)?;
    if !settles {
        return Err(failed(64, format!("the host that settles the fleet reads the holds its order and promotions screen by, and this is not it; place the hold from a session there\n{USAGE}")));
    }
    let commit = git(&["rev-parse", "--verify", &format!("{named}^{{commit}}")]).ok().filter(|commit| full_sha(commit)).ok_or_else(|| failed(1, format!("{named} names no commit here")))?;
    let ask = match lift {
        true => Ask::Lift { commit, why },
        false => Ask::Place { lane: lane_here(&commit)?, commit, host, until, why },
    };
    let line = serve(ask).map_err(|why| failed(1, why))?;
    if !line.is_empty() {
        println!("{line}");
    }
    Ok(())
}

struct Flags {
    commit: String,
    lift: bool,
    host: Option<String>,
    until: Option<String>,
    why: String,
}

fn parse(flags: &[String]) -> Result<Flags, Failure> {
    let usage = || failed(64, USAGE);
    let (lift, rest) = match flags.split_first() {
        Some((first, rest)) if first == "--lift" => (true, rest),
        _ => (false, flags),
    };
    let Some((commit, rest)) = rest.split_first() else { return Err(usage()) };
    let [host, until, why] = valued(rest, if lift { &["--why"] } else { &["--host", "--until", "--why"] }).ok_or_else(usage)?;
    match why {
        Some(why) if !commit.starts_with('-') && said(&why) && host.iter().chain(&until).all(|word| named(word)) => Ok(Flags { commit: commit.clone(), lift, host, until, why: why.trim().to_owned() }),
        _ => Err(usage()),
    }
}

/// The values of `--host`, `--until` and `--why` in `rest`, each given at most once and only when `allowed` names it, or `None` for anything else.
fn valued(mut rest: &[String], allowed: &[&str]) -> Option<[Option<String>; 3]> {
    let mut values: [Option<String>; 3] = Default::default();
    while let [flag, value, more @ ..] = rest {
        let slot = ["--host", "--until", "--why"].iter().position(|name| name == flag).filter(|_| allowed.contains(&flag.as_str()))?;
        if values[slot].replace(value.clone()).is_some() {
            return None;
        }
        rest = more;
    }
    rest.is_empty().then_some(values)
}
