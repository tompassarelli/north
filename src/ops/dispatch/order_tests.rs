use super::fleets;
use super::issues::{Issue, IssueRef};
use super::plan::Cloud;
use super::record::{self, Record};
use super::survey::claim_rows;
use super::tests::{fleet, issue, survey};
use super::{decide, tries};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

const SEQUENCES: u64 = 300;
const EVENTS: usize = 40;
const HOUR: f64 = 3600.0;
const START: f64 = 1_790_000_000.0;
const GATE: &str = "2026-10-28T00:00Z";
const GATE_AT: f64 = 1_793_145_600.0;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }

    fn pick<'a, T>(&mut self, from: &'a [T]) -> &'a T {
        &from[self.below(from.len())]
    }

    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }
}

#[derive(Clone, Debug)]
enum Event {
    Launch { number: u64, started: bool, hand: bool, cloud: bool, host: &'static str },
    Wait(f64),
    End { worked: bool, mirrored: bool },
    Claim { number: u64, stopped_owner: bool, live: bool },
}

fn event(rng: &mut Rng) -> Event {
    match rng.below(10) {
        0..=3 => Event::Launch { number: 1 + rng.below(3) as u64, started: rng.chance(75), hand: rng.chance(10), cloud: rng.chance(15), host: rng.pick(&["gex45", "whiterabbit", "nexus"]) },
        4..=6 => Event::Wait(*rng.pick(&[60.0, 20.0 * 60.0, 3.0 * HOUR, 7.0 * HOUR, 25.0 * HOUR])),
        7 | 8 => Event::End { worked: rng.chance(50), mirrored: rng.chance(30) },
        _ => Event::Claim { number: 1 + rng.below(3) as u64, stopped_owner: rng.chance(60), live: rng.chance(85) },
    }
}

fn item(number: u64) -> IssueRef {
    IssueRef::new("smashcraft", number)
}

#[derive(Clone, Default)]
struct World {
    at: f64,
    records: Vec<Record>,
    unspent: BTreeMap<String, String>,
    gex45: BTreeMap<String, &'static str>,
    nexus: BTreeMap<String, &'static str>,
    claims: Vec<(u64, String, bool)>,
    serial: usize,
}

fn launch_record(world: &World, number: u64, name: &str, started: bool, hand: bool, cloud: bool, host: &str) -> Record {
    Record { at: world.at, item: item(number).to_string(), host: host.into(), account: "claude:b".into(), name: name.into(), cloud, launched: started, detail: String::new(), tier: None, category: None, arm: false, handoff: None, hand }
}

impl World {
    fn apply(&mut self, event: &Event, rng: &mut Rng) {
        match event {
            Event::Launch { number, started, hand, cloud, host } => {
                if !hand && self.all_holds().contains_key(&item(*number)) {
                    return;
                }
                self.serial += 1;
                let name = format!("sc-{number}-s{}", self.serial);
                self.records.push(launch_record(self, *number, &name, *started, *hand, *cloud, host));
                if *started && !hand && !cloud {
                    self.gex45.insert(name, "working");
                }
            }
            Event::Wait(seconds) => self.at += seconds,
            Event::End { worked, mirrored } => {
                let running: Vec<String> = self.gex45.iter().filter(|(_, status)| **status == "working").map(|(name, _)| name.clone()).collect();
                if running.is_empty() {
                    return;
                }
                let name = rng.pick(&running).clone();
                self.gex45.insert(name.clone(), "exited");
                if *mirrored {
                    self.nexus.insert(name.clone(), "idle");
                }
                if !worked {
                    self.unspent.insert(name, "only waited on capacity".into());
                }
            }
            Event::Claim { number, stopped_owner, live } => {
                let exited: Vec<String> = self.gex45.iter().filter(|(_, status)| **status == "exited").map(|(name, _)| name.clone()).collect();
                let owner = if *stopped_owner && !exited.is_empty() { rng.pick(&exited).clone() } else { "tom".to_owned() };
                self.claims.push((*number, owner, *live));
            }
        }
    }

    fn departed(&self, record: &Record) -> bool {
        self.gex45.get(&record.name) == Some(&"exited") && self.nexus.get(&record.name) != Some(&"idle")
    }

    fn listings(&self) -> Vec<(String, Value)> {
        let rows = |sessions: &BTreeMap<String, &str>| Value::Array(sessions.iter().map(|(name, status)| json!({"name": name, "status": status})).collect());
        vec![("gex45".to_owned(), rows(&self.gex45)), ("nexus".to_owned(), rows(&self.nexus))]
    }

    fn threads(&self) -> Value {
        Value::Array(self.claims.iter().map(|(number, owner, live)| json!({"item": item(*number).to_string(), "owner": owner, "standing": if *live { "live" } else { "released" }})).collect())
    }

    fn gone(&self) -> BTreeSet<String> {
        fleets::gone(&self.listings(), &self.records, self.at)
    }

    fn holds(&self) -> BTreeMap<IssueRef, String> {
        record::holds(&self.records, self.at, &self.unspent, &self.gone())
    }

    fn all_holds(&self) -> BTreeMap<IssueRef, String> {
        let mut holds = self.holds();
        for (held, why) in record::launching(&self.records, self.at, &self.gone()) {
            holds.entry(held).or_insert(why);
        }
        for (host, listing) in self.listings() {
            for session in fleets::running_sessions(&listing) {
                for held in session.items {
                    holds.entry(held).or_insert_with(|| format!("fleet session {} on {host}", session.name));
                }
            }
        }
        let stopped = fleets::stopped_names(&self.listings());
        for (held, why) in claim_rows(&self.threads(), "gex45", &stopped) {
            holds.entry(held).or_insert(why);
        }
        holds
    }
}

fn started_hold(record: &Record) -> f64 {
    if !record.launched {
        15.0 * 60.0
    } else if record.cloud {
        6.0 * HOUR
    } else {
        2.0 * HOUR
    }
}

#[derive(Default)]
struct Broken(BTreeMap<&'static str, String>);

impl Broken {
    fn expect(&mut self, rule: &'static str, holds: bool, detail: impl FnOnce() -> String) {
        if !holds {
            self.0.entry(rule).or_insert_with(detail);
        }
    }
}

fn check_tries(world: &World, trail: &str, broken: &mut Broken) {
    let holds = world.holds();
    let launching = record::launching(&world.records, world.at, &world.gone());
    for number in 1..=3 {
        let held = item(number);
        let mine: Vec<&Record> = world.records.iter().filter(|record| record.item == held.to_string() && !record.hand).collect();
        let recent: Vec<&&Record> = mine.iter().filter(|record| world.at - record.at < 24.0 * HOUR).collect();
        let worked = recent.iter().filter(|record| record.launched && !world.unspent.contains_key(&record.name)).count();
        let reason = holds.get(&held);

        if let Some(fresh) = recent.iter().find(|record| record.launched && world.at - record.at < started_hold(record) && !world.departed(record) && !recent.iter().any(|later| later.at > record.at && !world.departed(later))) {
            broken.expect("a started launch holds its item for two hours, six in the cloud", reason.is_some(), || format!("{held}: {} launched {} s ago, nothing newer, nothing holds it\n{trail}", fresh.name, world.at - fresh.at));
            broken.expect("a fresh started launch shows in launching", launching.contains_key(&held), || format!("{held}\n{trail}"));
        }
        let proxied = reason.is_some_and(|why| why.contains("the proxy decides the next one"));
        broken.expect("two tries that did work in 24 h go to the proxy", worked < 2 || proxied, || format!("{held}: {worked} worked tries, hold {reason:?}\n{trail}"));
        broken.expect("a launch that failed or did no work is no try", worked >= 2 || !proxied, || format!("{held}: {worked} worked tries, hold {reason:?}\n{trail}"));
        if let Some(why) = reason.filter(|_| !proxied) {
            let newest = recent.iter().filter(|record| !world.departed(record)).max_by(|a, b| a.at.total_cmp(&b.at)).expect("a hold names a launch");
            broken.expect("a launch holds its item only within its window", world.at - newest.at < started_hold(newest), || format!("{held}: {why}\n{trail}"));
        }
        broken.expect("nothing launched in 24 h holds nothing", !recent.is_empty() || reason.is_none(), || format!("{held}: {reason:?}\n{trail}"));
    }

    let open: BTreeSet<IssueRef> = (1..=3).map(item).collect();
    for host in ["gex45", "whiterabbit", "nexus"] {
        let shown = world.records.iter().filter(|record| record.launched && !record.hand && record.host == host && world.at - record.at < if record.cloud { 6.0 * HOUR } else { 15.0 * 60.0 }).count();
        broken.expect("a launch is resident only until the copied fleet list can show it", record::recent_on(&world.records, host, &open, world.at) == shown as i64, || format!("{host}\n{trail}"));
    }

    let mut spared = world.unspent.clone();
    if let Some(name) = world.records.iter().find(|record| record.launched && !record.hand).map(|record| record.name.clone()) {
        spared.insert(name, "waited".into());
    }
    broken.expect("sparing a session never adds a hold", record::holds(&world.records, world.at, &spared, &world.gone()).keys().all(|held| holds.contains_key(held)), || trail.to_owned());

    let mut turned = world.records.clone();
    turned.reverse();
    let same: BTreeSet<_> = holds.keys().cloned().collect();
    broken.expect("holds do not depend on the order launches were recorded in", record::holds(&turned, world.at, &world.unspent, &world.gone()).keys().cloned().collect::<BTreeSet<_>>() == same, || trail.to_owned());

    let later = record::holds(&world.records, world.at + 25.0 * HOUR, &world.unspent, &world.gone());
    broken.expect("a day with no launch frees every item", later.is_empty(), || format!("{later:?}\n{trail}"));
}

fn check_claims(world: &World, trail: &str, broken: &mut Broken) {
    let listings = world.listings();
    let stopped = fleets::stopped_names(&listings);
    for (name, status) in &world.gex45 {
        let running_elsewhere = world.nexus.get(name).is_some_and(|other| *other == "idle");
        broken.expect("a session is stopped only when no host shows it running", stopped.contains(name) == (*status == "exited" && !running_elsewhere), || format!("{name}\n{trail}"));
    }
    let rows = claim_rows(&world.threads(), "gex45", &stopped);
    for number in 1..=3 {
        let expected = world.claims.iter().any(|(claimed, owner, live)| *claimed == number && *live && !stopped.contains(owner));
        broken.expect("a live claim holds unless its fleet owner stopped", rows.iter().any(|(held, _)| *held == item(number)) == expected, || format!("{}\n{trail}", item(number)));
    }
    let handmade = world.claims.iter().filter(|(_, owner, live)| *live && owner == "tom").map(|(number, _, _)| item(*number));
    for held in handmade {
        broken.expect("a hand-made claim always holds", rows.iter().any(|(row, _)| *row == held), || format!("{held}\n{trail}"));
    }
}

fn check_decide(world: &World, rng: &mut Rng, trail: &str, broken: &mut Broken) {
    let holds = world.all_holds();
    let gated = rng.chance(50);
    let boxes: Vec<Issue> = (1..=3).map(|number| issue("smashcraft", number, &format!("- [ ] build it{}", if gated && number == 2 { format!(" (not before {GATE})") } else { String::new() }), &[])).collect();
    let (hosts, accounts) = fleet();
    let mut surveyed = survey(boxes, Cloud::Closed(String::new()), hosts, accounts);
    let before = rng.chance(50);
    surveyed.at = if before { GATE_AT - 100.0 } else { GATE_AT + 100.0 };
    surveyed.holds = holds.clone();
    let outcome = decide(&surveyed, "order test");
    for launched in &outcome.launches {
        broken.expect("dispatch launches nothing that is held", !holds.contains_key(&launched.item), || format!("{}\n{trail}", launched.item));
    }
    for number in 1..=3 {
        let held = item(number);
        let skipped = outcome.skipped.iter().find(|(skipped, _)| *skipped == held).map(|(_, why)| why.clone());
        let launched = outcome.launches.iter().any(|launch| launch.item == held);
        broken.expect("every ready box is launched or skipped with a reason", skipped.is_some() != launched, || format!("{held}\n{trail}"));
        let waits = gated && number == 2 && before;
        if gated && number == 2 {
            broken.expect("a (not before) box waits until exactly that time", skipped.as_ref().is_some_and(|why| why.contains("until")) == waits, || format!("{held}: {skipped:?}\n{trail}"));
        }
        if holds.contains_key(&held) && !waits {
            broken.expect("a held box is skipped as held", skipped.is_some_and(|why| why.starts_with("held by")), || format!("{held}\n{trail}"));
        }
    }
}

#[test]
fn holds_and_claims_obey_the_rules_over_every_generated_order_of_events() {
    let mut broken = Broken::default();
    for seed in 0..SEQUENCES {
        let mut rng = Rng(seed);
        let mut world = World { at: START, ..World::default() };
        let mut trail = format!("seed {seed}:");
        for step in 0..EVENTS {
            let next = event(&mut rng);
            trail.push_str(&format!(" {next:?};"));
            world.apply(&next, &mut rng);
            check_tries(&world, &trail, &mut broken);
            check_claims(&world, &trail, &mut broken);
            if step % 5 == 0 {
                check_decide(&world, &mut rng, &trail, &mut broken);
            }
        }
    }
    let report: Vec<String> = broken.0.iter().map(|(rule, trail)| format!("BROKEN RULE: {rule}\n{trail}")).collect();
    assert!(report.is_empty(), "{} rules broken\n{}", report.len(), report.join("\n"));
}

#[test]
fn a_session_spends_a_try_when_it_committed_or_checked_and_otherwise_only_when_its_report_is_no_capacity_wait() {
    let commit = "Bash git -C ~/code/smashcraft/sc-1 commit -m 'x'";
    let push = "Bash safe-push --to main";
    let check = "Bash cargo test --locked";
    let idle = "Bash ls";
    let capacity = [
        "You've hit your session limit · resets 11:50pm (UTC)",
        "You've hit your weekly limit · resets Fri",
        "You've hit your limit",
        "Not done: the usage limit stopped the run.",
        "Admission answered DEFER_GPU_CLIENTS for an hour.",
        "Not done: the level-0 test is still waiting for pair 10.",
        "Not done: connection refused by the host.",
    ];
    let reports = ["Not done: the box asks for two frame counts that contradict each other.", "Done: landed.", "Stop: decision."];
    let mut rng = Rng(7);
    for _ in 0..2000 {
        let tools: Vec<&str> = (0..rng.below(4)).map(|_| *rng.pick(&[commit, push, check, idle])).collect();
        let worked = tools.iter().any(|tool| *tool != idle);
        let limited = rng.chance(50);
        let report = if limited { *rng.pick(&capacity) } else { *rng.pick(&reports) };
        let mut entries = vec![json!({"kind": "user", "text": "Fix it."})];
        entries.extend(tools.iter().map(|tool| json!({"kind": "tool", "text": tool})));
        entries.push(json!({"kind": "assistant", "text": report}));
        let session = json!({"entries": entries, "turns": [{"exit": "completed", "ended_ms": 1_000_000}]});
        let spent = tries::unspent(&session).is_none();
        assert_eq!(spent, worked || !limited, "tools {tools:?}, report {report:?}");
    }
}
