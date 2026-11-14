use super::behind::{Process, Seen, objects};
use crate::runtime::live::Swap;
use super::page::{Pager, conclude};
use super::samples;
use serde_json::json;

const OLD: &str = "aaaaaaaa1111111111111111111111111111aaaa";
const MIDDLE: &str = "bbbbbbbb2222222222222222222222222222bbbb";
const NEWEST: &str = "cccccccc3333333333333333333333333333cccc";
const LATEST: &str = "dddddddd4444444444444444444444444444dddd";

fn process(pid: u32, kind: &str, name: &str, runtime: Option<&str>) -> Process {
    Process { pid, kind: kind.to_owned(), process: name.to_owned(), runtime: runtime.map(str::to_owned) }
}

fn took(runtime: &str, at: i64) -> Option<Swap> {
    Some(Swap { runtime: runtime.to_owned(), at, estimated: true, rollback: false })
}

fn unheld(_: &Seen) -> Option<String> {
    None
}

fn rank(sha: &str) -> u8 {
    [OLD, MIDDLE, NEWEST, LATEST].iter().position(|known| *known == sha).map_or(0, |at| at as u8 + 1)
}

/// Records the pages and clears sent; `accept` says whether the proxy took them.
struct Recorder {
    accept: bool,
    lags: Vec<(String, String)>,
    clears: Vec<String>,
}

impl Recorder {
    fn new(accept: bool) -> Self {
        Self { accept, lags: Vec::new(), clears: Vec::new() }
    }
}

impl Pager for Recorder {
    fn session_lags(&mut self, text: &str) -> bool {
        self.lags.push(("runtime-lag".to_owned(), text.to_owned()));
        self.accept
    }

    fn service_lags(&mut self, key: &str, text: &str) -> bool {
        self.lags.push((key.to_owned(), text.to_owned()));
        self.accept
    }

    fn service_clears(&mut self, key: &str, _text: &str) -> bool {
        self.clears.push(key.to_owned());
        self.accept
    }
}

#[test]
fn runtime_lag_json_reports_seconds_behind_for_every_process_and_pages_each_episode() {
    let seen = vec![
        Seen { host: "laptop".to_owned(), swap: took(NEWEST, 2_000), processes: Ok(vec![process(1, "land serve", "land serve all repositories", Some(NEWEST)), process(2, "session-host", "session-host worker-1", Some(MIDDLE)), process(3, "fleet watchdog", "fleet watchdog worker-1", None)]) },
        Seen { host: "nexus".to_owned(), swap: took(NEWEST, 2_000), processes: Ok(vec![process(4, "offload taker", "offload taker", Some(OLD))]) },
        Seen { host: "gone".to_owned(), swap: None, processes: Err("no answer within 60s".to_owned()) },
    ];
    let holds = |landed: &str, runtime: &str| rank(runtime) >= rank(landed);
    let found = objects(&seen, 2_700, holds, unheld);
    assert_eq!(
        found,
        vec![
            json!({"host": "laptop", "kind": "land serve", "process": "land serve all repositories", "runtime": NEWEST, "linked": NEWEST, "seconds": 0}),
            json!({"host": "laptop", "kind": "session-host", "process": "session-host worker-1", "runtime": MIDDLE, "linked": NEWEST, "seconds": 700}),
            json!({"host": "laptop", "kind": "fleet watchdog", "process": "fleet watchdog worker-1", "runtime": null, "linked": NEWEST, "seconds": 700}),
            json!({"host": "nexus", "kind": "offload taker", "process": "offload taker", "runtime": OLD, "linked": NEWEST, "seconds": 700}),
            json!({"host": "gone", "process": null, "runtime": null, "seconds": null, "unread": "no answer within 60s"}),
        ]
    );

    let state = tempfile::tempdir().unwrap();
    let mut refused = Recorder::new(false);
    conclude(state.path(), &found, 2_690, &mut refused);
    assert_eq!(refused.lags.len(), 2, "both services over 10 min are each tried, a session 10 min behind is not, and a refused page is tried again");

    let mut pager = Recorder::new(true);
    conclude(state.path(), &found, 2_700, &mut pager);
    assert_eq!(pager.lags.len(), 2, "one page for each host service over 10 min: {:?}", pager.lags);
    assert!(pager.lags.iter().any(|(key, text)| key == "runtime-lag:nexus:offload taker" && text.contains("offload taker on nexus") && text.contains("700s")), "{:?}", pager.lags);
    conclude(state.path(), &found, 2_760, &mut pager);
    assert_eq!(pager.lags.len(), 2, "an episode that was paged is not paged again while it lasts");

    let earlier = vec![Seen { host: "laptop".to_owned(), swap: took(MIDDLE, 1_000), processes: seen[0].processes.clone() }];
    let recovered = objects(&earlier, 1_100, holds, unheld);
    conclude(state.path(), &recovered, 1_100, &mut pager);
    assert_eq!(pager.clears, ["runtime-lag:laptop:fleet watchdog worker-1", "runtime-lag:nexus:offload taker"], "a service back under 10 min, or on a host that no longer reports it, ends its episode with a cleared notice");
    conclude(state.path(), &found, 2_800, &mut pager);
    assert_eq!(pager.lags.len(), 4, "lag that falls under 10 min ends the episode, so the next one pages again");
    let sampled = samples::read(state.path());
    assert_eq!(sampled.len(), 5);
    assert_eq!(samples::worst_in(&sampled, 2_000, 3_000), Some((700.0, 4)));
}

#[test]
fn a_session_pages_only_a_slot_behind_the_pin() {
    let swapped = |now: i64| if now >= 10_800 { took(LATEST, 10_800) } else { took(NEWEST, 2_000) };
    let holds = |landed: &str, runtime: &str| rank(runtime) >= rank(landed);
    let reading = |now: i64, broker: &str, session: &str| vec![Seen { host: "laptop".to_owned(), swap: swapped(now), processes: Ok(vec![process(1, "session-host", "session-host worker-1", Some(session)), process(2, "broker", "broker", Some(broker))]) }];
    let state = tempfile::tempdir().unwrap();
    let mut pager = Recorder::new(true);
    let steps = [(10_700, NEWEST, NEWEST), (10_750, NEWEST, NEWEST), (11_000, NEWEST, NEWEST), (11_300, NEWEST, NEWEST), (11_450, NEWEST, NEWEST), (11_500, LATEST, NEWEST), (11_600, NEWEST, NEWEST), (25_000, NEWEST, NEWEST), (25_300, NEWEST, NEWEST), (25_400, NEWEST, NEWEST)];
    for (now, broker, session) in steps {
        conclude(state.path(), &objects(&reading(now, broker, session), now, holds, unheld), now, &mut pager);
        if now < 11_400 {
            assert!(pager.lags.is_empty(), "a swap under 10 min old pages nothing at {now}: {:?}", pager.lags);
        }
        if now < 25_200 {
            assert!(pager.lags.iter().all(|(key, _)| key != "runtime-lag"), "a session behind its host's swap by less than a promotion slot pages nothing at {now}: {:?}", pager.lags);
        }
    }
    let broker_pages: Vec<&String> = pager.lags.iter().filter(|(key, _)| key == "runtime-lag:laptop:broker").map(|(_, text)| text).collect();
    assert_eq!(broker_pages.len(), 2, "the broker a promotion left behind pages each time it starts to lag: {:?}", pager.lags);
    assert!(broker_pages.iter().all(|text| text.contains("broker on laptop")), "{broker_pages:?}");
    assert_eq!(pager.clears, ["runtime-lag:laptop:broker"]);
    assert_eq!(pager.lags.iter().filter(|(key, _)| key == "runtime-lag").count(), 1, "the session a whole slot behind pages once");
    assert_eq!(samples::read(state.path()).len(), 10);
}

#[test]
fn a_service_pages_only_when_behind_since_its_host_took_the_new_runtime_for_10_minutes_and_a_held_host_never() {
    let holds = |landed: &str, runtime: &str| rank(runtime) >= rank(landed);
    let held = |seen: &Seen| (seen.host == "gex45").then(|| "aaaaaaaa of the lane eval, held on gex45 by release-evals-lead".to_owned());
    let reading = |broker: &str, taker: &str| {
        vec![
            Seen { host: "cpx62".to_owned(), swap: took(LATEST, 10_000), processes: Ok(vec![process(1, "broker", "broker", Some(broker)), process(2, "offload taker", "offload taker", Some(taker))]) },
            Seen { host: "gex45".to_owned(), swap: took(OLD, 1_000), processes: Ok(vec![process(3, "land serve", "land serve all repositories", None)]) },
        ]
    };
    let state = tempfile::tempdir().unwrap();
    let mut pager = Recorder::new(true);
    for (now, broker, taker) in [(10_300, NEWEST, NEWEST), (10_500, NEWEST, LATEST), (10_590, NEWEST, LATEST)] {
        conclude(state.path(), &objects(&reading(broker, taker), now, holds, held), now, &mut pager);
    }
    assert!(pager.lags.is_empty(), "a service under 10 min behind its host's swap pages nothing, and one restarted 500 s after the swap never pages: {:?}", pager.lags);
    let late = objects(&reading(NEWEST, LATEST), 10_700, holds, held);
    conclude(state.path(), &late, 10_700, &mut pager);
    assert_eq!(pager.lags.len(), 1, "only the broker still behind 700 s after its host took the new runtime pages: {:?}", pager.lags);
    let (key, text) = &pager.lags[0];
    assert_eq!(key, "runtime-lag:cpx62:broker");
    assert!(text.starts_with("north runtime lag: broker on cpx62 has stayed on cccccccc 700s since cpx62 took dddddddd"), "{text}");
    let gex45 = late.iter().find(|object| object["host"] == "gex45").unwrap();
    assert_eq!((gex45["seconds"].as_i64(), gex45["held"].as_bool()), (Some(0), Some(true)), "a host a recorded hold keeps on an old runtime reads 0 and never pages, 9700 s after its swap: {gex45}");
}
