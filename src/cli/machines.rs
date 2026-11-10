//! `north check machines`: explores every lifecycle machine the registry models over every ordering of the model's
//! scope and fails on a broken promise, a chart that is not what the check generates, a machine the registry does
//! not name and a registry file the landing's check lines do not select; `--fixes` reports each machine whose code a
//! landed incident fix changed, which the machine-fix detector files as a box, and `--replay` scores that trigger over
//! history beside calendar and churn sweeps (docs/rationale/watch/2026-11-10-pstack-verification.md).
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use north_core::error::{NorthError, NorthResult};
use north_machine_model::{Model, Promises, Together, explore, explore_together, page};

mod history;

const USAGE: &str = "usage: north check machines [--write | --registry | --fixes [--days N] | --replay [--days N]] [--root DIR]";
const CHARTS: &str = "docs/rationale/state-machines";
const LAND_CHECKS: &str = ".safe-push";
const VERB: &str = "check machines";
const REGISTRY_ONLY: &str = "--registry";
const SOURCE: [&str; 2] = ["src/", "crates/"];
const IMPLEMENTS: &str = "impl Lifecycle for ";
const REGISTRY: &str = "src/cli/machines.rs";
const ALWAYS: [&str; 5] = ["crates/north-base/src/lifecycle.rs", "crates/north-base/src/lifecycle/", "crates/north-machine-model/", REGISTRY, "docs/rationale/state-machines/"];
const FIX_DAYS: u32 = 7;
const REPLAY_DAYS: u32 = 30;
const AFTER_LANDING: &str = "crates/north-landing/src/land/after_landing_lifecycle.rs";
const RUNTIME: &str = "crates/north-runtime/src/runtime/lifecycle.rs";

/// One lifecycle machine: the file holding its table, the code that moves it, whose incident fixes count against
/// its model, and how to explore it. A machine whose type is private to its crate has no `explore` yet.
struct Machine {
    name: &'static str,
    kind: &'static str,
    source: &'static str,
    /// Code the model's two-instance search drives or gates, so a lane that changes it runs the search.
    guard: &'static [&'static str],
    code: &'static [&'static str],
    explore: Option<fn() -> NorthResult<Model>>,
}

const MACHINES: &[Machine] = &[
    Machine {
        name: "fleet-session",
        kind: "Session",
        source: "crates/north-session-host/src/session_lifecycle.rs",
        guard: &["crates/north-fleet-supervisor/src/fleet/registry/launching.rs"],
        code: &[
            "crates/north-session-host/src/session_host.rs",
            "crates/north-fleet-supervisor/src/fleet/reaper.rs",
            "crates/north-fleet-supervisor/src/fleet/registry/adopting.rs",
            "crates/north-fleet-supervisor/src/fleet/registry/launching.rs",
            "crates/north-fleet-supervisor/src/fleet/registry/status.rs",
            "crates/north-fleet-supervisor/src/fleet/registry/stopping.rs",
        ],
        explore: Some(|| {
            use north_session_host::session_lifecycle::{Phase, Session};
            explore_together::<Session>(&Promises { ends: &[Phase::Retired], repeats: &[Phase::Live, Phase::Stopped, Phase::Adopted] }, &session_together())
        }),
    },
    Machine {
        name: "land-ticket",
        kind: "LandTicket",
        source: "crates/north-base/src/land_ticket.rs",
        guard: &["crates/north-land-scans/src/withdrawal.rs", "crates/north-landing/src/land/enqueue.rs"],
        code: &[
            "crates/north-lander/src/land/enqueue",
            "crates/north-lander/src/land/promote",
            "crates/north-lander/src/land/withdraw.rs",
            "crates/north-land-scans/src/withdrawal.rs",
            "crates/north-landing/src/land/queue.rs",
            "crates/north-landing/src/land/records.rs",
            "crates/north-landing/src/land/reservation.rs",
        ],
        explore: Some(|| {
            use north_core::land_ticket::{LandTicket, Phase};
            explore_together::<LandTicket>(&Promises { ends: &[Phase::Landed, Phase::Replaced, Phase::Withdrawn], repeats: &[Phase::Queued] }, &ticket_together())
        }),
    },
    Machine {
        name: "row-delivery",
        kind: "RowDelivery",
        source: "crates/north-session-host/src/row_delivery.rs",
        guard: &[],
        code: &["crates/north-fleet-supervisor/src/fleet/registry/senders.rs"],
        explore: Some(|| {
            use north_session_host::row_delivery::{Phase, RowDelivery};
            explore::<RowDelivery>(&Promises { ends: &[Phase::Delivered], repeats: &[Phase::Delivered] })
        }),
    },
    Machine {
        name: "after-landing-run",
        kind: "AfterLandingRun",
        source: AFTER_LANDING,
        guard: &[],
        code: &["crates/north-landing/src/landing/after_landing", "crates/north-landing/src/land/sheriff.rs"],
        explore: Some(|| {
            use north_landing::land::after_landing_lifecycle::{AfterLandingRun, Run};
            explore::<AfterLandingRun>(&Promises { ends: &[Run::Retired], repeats: &[Run::Journaled, Run::Retired] })
        }),
    },
    Machine { name: "red-main", kind: "RedMain", source: AFTER_LANDING, guard: &[], code: &[], explore: Some(|| explore::<north_landing::land::after_landing_lifecycle::RedMain>(&Promises { ends: &[], repeats: &[] })) },
    Machine {
        name: "base-broken",
        kind: "BaseBroken",
        source: AFTER_LANDING,
        guard: &[],
        code: &[],
        explore: Some(|| {
            use north_landing::land::after_landing_lifecycle::{BaseBroken, Pause};
            explore::<BaseBroken>(&Promises { ends: &[], repeats: &[Pause::Paused, Pause::Alerted] })
        }),
    },
    Machine {
        name: "capacity-lease",
        kind: "Leased",
        source: "crates/north-capacity/src/capacity/lease_lifecycle.rs",
        guard: &[],
        code: &["crates/north-capacity/src/capacity/lease_deadline.rs", "crates/north-capacity/src/capacity/deadline.rs"],
        explore: Some(|| {
            use north_capacity::capacity::lease_lifecycle::{Leased, Phase};
            explore::<Leased>(&Promises { ends: &[Phase::Released, Phase::Reclaimed], repeats: &[Phase::Reserved] })
        }),
    },
    Machine {
        name: "gate",
        kind: "GateLife",
        source: "crates/north-fleet-store/src/gate_lifecycle.rs",
        guard: &[],
        code: &["src/cli/gate"],
        explore: Some(|| {
            use north_fleet_store::gate_lifecycle::{GateLife, GatePhase};
            explore::<GateLife>(&Promises { ends: &[GatePhase::Answered, GatePhase::Defaulted, GatePhase::Orphaned], repeats: &[] })
        }),
    },
    Machine {
        name: "claim",
        kind: "Claiming",
        source: "crates/north-threads/src/threads/claims/lifecycle.rs",
        guard: &[],
        code: &["crates/north-threads/src/threads/claims/"],
        explore: Some(|| {
            use north_threads::threads::claims::lifecycle::{Claiming, Phase};
            explore::<Claiming>(&Promises { ends: &[], repeats: &[Phase::Held] })
        }),
    },
    Machine {
        name: "check-environment",
        kind: "CheckEnvironment",
        source: "crates/north-check-environment/src/lib.rs",
        guard: &[],
        code: &["crates/north-check-environment/src/"],
        explore: Some(|| {
            use north_check_environment::{CheckEnvironment, Environment};
            explore::<CheckEnvironment>(&Promises { ends: &[], repeats: &[Environment::Placed, Environment::Local] })
        }),
    },
    Machine { name: "runtime-promotion", kind: "Promoting", source: RUNTIME, guard: &[], code: &["crates/north-runtime/src/runtime/lifecycle", "crates/north-runtime/src/runtime/update", "crates/north-runtime/src/runtime/settle.rs"], explore: None },
    Machine { name: "runtime-canary", kind: "Canarying", source: RUNTIME, guard: &[], code: &["crates/north-runtime/src/runtime/canary"], explore: None },
    Machine { name: "runtime-landing-judgement", kind: "Judge", source: RUNTIME, guard: &[], code: &[], explore: None },
    Machine { name: "threads-spool", kind: "SpooledWrite", source: "crates/north-threads/src/threads/client/spool.rs", guard: &[], code: &["crates/north-threads/src/threads/client"], explore: None },
    Machine { name: "offload-offer", kind: "OffloadOffer", source: "crates/north-capacity-host/src/capacity/host/offer_lifecycle.rs", guard: &[], code: &[], explore: None },
];

fn claims(path: &Path) -> bool {
    north_session_host::session_lifecycle::claim(path).is_ok()
}

/// Two launches of one session name: the name is claimed by `create_dir`, so the guard refuses a sequential second
/// launch, and two launches racing for an empty name must leave exactly one winner.
fn session_together() -> Together<north_session_host::session_lifecycle::Phase> {
    use north_session_host::session_lifecycle::Phase;
    Together {
        alone: &[Phase::Live, Phase::Parked, Phase::IdleExited, Phase::HostEnded, Phase::Stopped, Phase::Adopted],
        admits: |other| match other {
            Phase::Retired => true,
            _ => tempfile::tempdir().is_ok_and(|root| {
                let path = root.path().join("launch");
                claims(&path) && claims(&path)
            }),
        },
        race: || {
            let Ok(root) = tempfile::tempdir() else { return 0 };
            let path = root.path().join("launch");
            std::thread::scope(|scope| {
                let handles: Vec<_> = (0..2).map(|_| scope.spawn(|| claims(&path))).collect();
                handles.into_iter().map(|handle| handle.join().unwrap_or(false)).filter(|won| *won).count()
            })
        },
    }
}

/// Two tickets of one head: a head whose newest ticket was withdrawn is not queued again until the withdrawal is
/// cleared, which `refuse_requeue` decides from the queue.
fn ticket_together() -> Together<north_core::land_ticket::Phase> {
    use north_core::land_ticket::Phase;
    Together {
        alone: &[Phase::Withdrawn],
        admits: |other| match other {
            Phase::Withdrawn => withdrawn_head_admitted(),
            _ => true,
        },
        race: || 1,
    }
}

fn withdrawn_head_admitted() -> bool {
    let Ok(root) = tempfile::tempdir() else { return false };
    let queue = root.path().join("queue");
    let withdrawn = queue.join("withdrawn");
    if fs::create_dir_all(&withdrawn).is_err() {
        return false;
    }
    let ticket = serde_json::json!({
        "root": root.path().to_string_lossy(),
        "branch": "lane",
        "head": "abc",
        "session": "session",
        "verdict": "withdrawn by Tom: the callers were not audited",
        "queued_micros": 1
    });
    if fs::write(withdrawn.join("1-ticket.json"), ticket.to_string()).is_err() {
        return false;
    }
    north_landing::land::scans::withdrawal::refuse_requeue(&queue, root.path(), "lane", "abc", Some("session")).is_ok()
}

enum Mode {
    Explore { write: bool },
    Registry,
    Fixes { days: u32 },
    Replay { days: u32 },
}

fn parsed(arguments: &[String]) -> Option<(Mode, PathBuf)> {
    let (mut mode, mut days, mut root) = (None, None, PathBuf::from("."));
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        let chosen = match word.as_str() {
            "--write" => Mode::Explore { write: true },
            REGISTRY_ONLY => Mode::Registry,
            "--fixes" => Mode::Fixes { days: FIX_DAYS },
            "--replay" => Mode::Replay { days: REPLAY_DAYS },
            "--root" => {
                root = PathBuf::from(words.next()?);
                continue;
            }
            "--days" => {
                days = Some(words.next()?.parse().ok()?);
                continue;
            }
            _ => return None,
        };
        if mode.replace(chosen).is_some() {
            return None;
        }
    }
    match (mode.unwrap_or(Mode::Explore { write: false }), days) {
        (Mode::Fixes { .. }, Some(days)) => Some((Mode::Fixes { days }, root)),
        (Mode::Replay { .. }, Some(days)) => Some((Mode::Replay { days }, root)),
        (_, Some(_)) => None,
        (mode, None) => Some((mode, root)),
    }
}

pub fn run(arguments: &[String]) -> u8 {
    let Some((mode, root)) = parsed(arguments) else {
        eprintln!("{USAGE}");
        return 2;
    };
    let checked = match mode {
        Mode::Explore { write } => check(&root, write),
        Mode::Registry => named(&root),
        Mode::Fixes { days } => return reported(history::fixes(&root, days)),
        Mode::Replay { days } => return reported(history::replay(&root, days)),
    };
    match checked {
        Ok(problems) if problems.is_empty() => 0,
        Ok(problems) => {
            problems.iter().for_each(|problem| eprintln!("north check machines: {problem}"));
            1
        }
        Err(error) => {
            eprintln!("north check machines: {error}");
            2
        }
    }
}

fn reported(lines: NorthResult<Vec<String>>) -> u8 {
    match lines {
        Ok(lines) => {
            lines.iter().for_each(|line| println!("{line}"));
            0
        }
        Err(error) => {
            eprintln!("north check machines: {error}");
            2
        }
    }
}

fn git(root: &Path, arguments: &[&str]) -> NorthResult<String> {
    answered(root, arguments, 0)
}

/// `git grep` exits 1 when no line matches, which answers the search and is no failure.
fn grep(root: &Path, text: &str) -> NorthResult<String> {
    answered(root, &["grep", "--full-name", "-F", text, "--", "crates", "src"], 1)
}

fn answered(root: &Path, arguments: &[&str], highest: i32) -> NorthResult<String> {
    let output = Command::new("git").arg("-C").arg(root).args(arguments).output()?;
    if !output.status.code().is_some_and(|code| (0..=highest).contains(&code)) {
        return Err(NorthError::from(std::io::Error::other(format!("git {} failed in {}", arguments.join(" "), root.display()))));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn check(root: &Path, write: bool) -> NorthResult<Vec<String>> {
    let mut problems = Vec::new();
    let mut pages = Vec::new();
    for machine in MACHINES {
        let Some(explore) = machine.explore else { continue };
        let started = Instant::now();
        let model = explore()?;
        println!("{}: {} situations, {} steps, {} moves, {} broken, {} ms", model.machine, model.situations, model.steps, model.moves.len(), model.findings.len(), started.elapsed().as_millis());
        if model.machine != machine.name {
            problems.push(format!("{REGISTRY} names {} and its machine calls itself {}", machine.name, model.machine));
        }
        problems.extend(model.findings.iter().map(|finding| format!("{}: {}", model.machine, finding.line())));
        problems.extend(chart(root, machine, &model, write)?);
        pages.push(format!("{}.md", model.machine));
    }
    problems.extend(strays(root, &pages)?);
    problems.extend(named(root)?);
    Ok(problems)
}
fn chart(root: &Path, machine: &Machine, model: &Model, write: bool) -> NorthResult<Option<String>> {
    let path = root.join(CHARTS).join(format!("{}.md", model.machine));
    let page = page(model, machine.source);
    if fs::read_to_string(&path).is_ok_and(|kept| kept == page) {
        return Ok(None);
    }
    if !write {
        return Ok(Some(format!("{CHARTS}/{}.md is not the chart the check generates; run `north check machines --write`", model.machine)));
    }
    fs::create_dir_all(root.join(CHARTS))?;
    fs::write(&path, page)?;
    println!("wrote {}", path.display());
    Ok(None)
}

fn strays(root: &Path, pages: &[String]) -> NorthResult<Vec<String>> {
    let Ok(entries) = fs::read_dir(root.join(CHARTS)) else { return Ok(Vec::new()) };
    let mut strays = Vec::new();
    for entry in entries {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if !pages.contains(&name) {
            strays.push(format!("{CHARTS}/{name} is the chart of no modelled machine; remove it"));
        }
    }
    strays.sort();
    Ok(strays)
}

/// What needs no exploring: a lifecycle machine the registry does not name, a registry path the tree lacks and a
/// landing check line that does not select what it must.
fn named(root: &Path) -> NorthResult<Vec<String>> {
    let missing = MACHINES.iter().flat_map(|machine| machine.code.iter().filter(|path| git(root, &["ls-files", "--", &format!("{path}*")]).is_ok_and(|files| files.is_empty())).map(|path| format!("{REGISTRY} lists {path} as code of {}, and the tree has no such file", machine.name)));
    Ok(unregistered(root)?.into_iter().chain(missing).chain(unselected(root)?).collect())
}

fn unregistered(root: &Path) -> NorthResult<Vec<String>> {
    let text = grep(root, IMPLEMENTS)?;
    let known: Vec<(&str, &str)> = MACHINES.iter().map(|machine| (machine.kind, machine.source)).collect();
    let machines: Vec<(&str, &str)> = text
        .lines()
        .filter_map(|line| {
            let (source, rest) = line.split_once(':')?;
            let kind = rest.split_once(IMPLEMENTS)?.1.split(|character: char| !character.is_alphanumeric()).next()?;
            (!source.ends_with("tests.rs") && source != REGISTRY).then_some((kind, source))
        })
        .collect();
    let unnamed = machines.iter().filter(|machine| !known.contains(machine)).map(|(kind, source)| format!("{source}: {kind} is a lifecycle machine {REGISTRY} does not name; name it there with its promises"));
    let gone = known.iter().filter(|machine| !machines.contains(machine)).map(|(kind, source)| format!("{REGISTRY} names {kind} in {source}, and no lifecycle machine of that name is there"));
    Ok(unnamed.chain(gone).collect())
}

fn unselected(root: &Path) -> NorthResult<Vec<String>> {
    let checks = fs::read_to_string(root.join(LAND_CHECKS))?;
    let lines: Vec<(Vec<&str>, &str)> = checks
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("check")?.trim_start_matches([' ', '=', '"']).split_once("::"))
        .filter(|(_, command)| command.contains(VERB))
        .map(|(paths, command)| (paths.split(',').map(str::trim).collect(), command))
        .collect();
    let selects = |registry: bool, source: &str| lines.iter().any(|(paths, command)| command.contains(REGISTRY_ONLY) == registry && paths.iter().any(|path| !path.is_empty() && source.starts_with(path)));
    let modelled = MACHINES
        .iter()
        .filter(|machine| machine.explore.is_some())
        .flat_map(|machine| std::iter::once(machine.source).chain(machine.guard.iter().copied()))
        .chain(ALWAYS)
        .filter(|source| !selects(false, source));
    let unexplored = modelled.map(|source| format!("{source} belongs to a modelled machine and no `north {VERB}` line in {LAND_CHECKS} selects it, so a lane changing it lands unchecked"));
    let unscanned = SOURCE.into_iter().chain([LAND_CHECKS]).filter(|source| !selects(true, source));
    let unnamed = unscanned.map(|source| format!("no `north {VERB} {REGISTRY_ONLY}` line in {LAND_CHECKS} selects {source}, so a lane adding a lifecycle machine there lands it unmodelled"));
    Ok(unexplored.chain(unnamed).collect())
}
