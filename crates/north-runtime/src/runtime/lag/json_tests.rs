use super::behind::{Pin, Process, Seen, objects};
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
    let pin = Pin { at: 2_000, sha: NEWEST.to_owned() };
    let seen = vec![
        Seen { host: "laptop".to_owned(), processes: Ok(vec![process(1, "land serve", "land serve all repositories", Some(NEWEST)), process(2, "session-host", "session-host worker-1", Some(MIDDLE)), process(3, "fleet watchdog", "fleet watchdog worker-1", None)]) },
        Seen { host: "nexus".to_owned(), processes: Ok(vec![process(4, "offload taker", "offload taker", Some(OLD))]) },
        Seen { host: "gone".to_owned(), processes: Err("no answer within 60s".to_owned()) },
    ];
    let holds = |landed: &str, runtime: &str| rank(runtime) >= rank(landed);
    let found = objects(&seen, Some(&pin), 2_500, holds);
    assert_eq!(
        found,
        vec![
            json!({"host": "laptop", "kind": "land serve", "process": "land serve all repositories", "runtime": NEWEST, "seconds": 0}),
            json!({"host": "laptop", "kind": "session-host", "process": "session-host worker-1", "runtime": MIDDLE, "seconds": 500}),
            json!({"host": "laptop", "kind": "fleet watchdog", "process": "fleet watchdog worker-1", "runtime": null, "seconds": 500}),
            json!({"host": "nexus", "kind": "offload taker", "process": "offload taker", "runtime": OLD, "seconds": 500}),
            json!({"host": "gone", "process": null, "runtime": null, "seconds": null, "unread": "no answer within 60s"}),
        ]
    );

    let state = tempfile::tempdir().unwrap();
    let mut refused = Recorder::new(false);
    conclude(state.path(), &found, 2_490, &mut refused);
    assert_eq!(refused.lags.len(), 2, "both services over 5 min are each tried, a session 5 min behind is not, and a refused page is tried again");

    let mut pager = Recorder::new(true);
    conclude(state.path(), &found, 2_500, &mut pager);
    assert_eq!(pager.lags.len(), 2, "one page for each host service over 5 min: {:?}", pager.lags);
    assert!(pager.lags.iter().any(|(key, text)| key == "runtime-lag:nexus:offload taker" && text.contains("offload taker on nexus") && text.contains("500s")), "{:?}", pager.lags);
    conclude(state.path(), &found, 2_560, &mut pager);
    assert_eq!(pager.lags.len(), 2, "an episode that was paged is not paged again while it lasts");

    let earlier = Pin { at: 1_000, sha: MIDDLE.to_owned() };
    let recovered = objects(&seen[..1], Some(&earlier), 1_100, holds);
    conclude(state.path(), &recovered, 1_100, &mut pager);
    assert_eq!(pager.clears, ["runtime-lag:laptop:fleet watchdog worker-1", "runtime-lag:nexus:offload taker"], "a service back under 5 min, or on a host that no longer reports it, ends its episode with a cleared notice");
    conclude(state.path(), &found, 2_600, &mut pager);
    assert_eq!(pager.lags.len(), 4, "lag that falls under 5 min ends the episode, so the next one pages again");
    let sampled = samples::read(state.path());
    assert_eq!(sampled.len(), 5);
    assert_eq!(samples::worst_in(&sampled, 2_000, 3_000), Some((500.0, 4)));
}

#[test]
fn a_session_pages_only_a_slot_behind_the_pin() {
    let pinned = |now: i64| if now >= 10_800 { Pin { at: 10_800, sha: LATEST.to_owned() } } else { Pin { at: 2_000, sha: NEWEST.to_owned() } };
    let holds = |landed: &str, runtime: &str| rank(runtime) >= rank(landed);
    let reading = |broker: &str, session: &str| vec![Seen { host: "laptop".to_owned(), processes: Ok(vec![process(1, "session-host", "session-host worker-1", Some(session)), process(2, "broker", "broker", Some(broker))]) }];
    let state = tempfile::tempdir().unwrap();
    let mut pager = Recorder::new(true);
    let steps = [(10_700, NEWEST, NEWEST), (10_750, NEWEST, NEWEST), (11_000, NEWEST, NEWEST), (11_200, NEWEST, NEWEST), (11_300, NEWEST, NEWEST), (11_400, LATEST, NEWEST), (11_500, NEWEST, NEWEST), (25_000, NEWEST, NEWEST), (25_300, NEWEST, NEWEST), (25_400, NEWEST, NEWEST)];
    for (now, broker, session) in steps {
        conclude(state.path(), &objects(&reading(broker, session), Some(&pinned(now)), now, holds), now, &mut pager);
        if now < 11_100 {
            assert!(pager.lags.is_empty(), "a landing 5 min old or a promotion under 5 min old pages nothing at {now}: {:?}", pager.lags);
        }
        if now < 25_200 {
            assert!(pager.lags.iter().all(|(key, _)| key != "runtime-lag"), "a session behind the pin by less than a promotion slot pages nothing at {now}: {:?}", pager.lags);
        }
    }
    let broker_pages: Vec<&String> = pager.lags.iter().filter(|(key, _)| key == "runtime-lag:laptop:broker").map(|(_, text)| text).collect();
    assert_eq!(broker_pages.len(), 2, "the broker a promotion left behind pages each time it starts to lag: {:?}", pager.lags);
    assert!(broker_pages.iter().all(|text| text.contains("broker on laptop")), "{broker_pages:?}");
    assert_eq!(pager.clears, ["runtime-lag:laptop:broker"]);
    assert_eq!(pager.lags.iter().filter(|(key, _)| key == "runtime-lag").count(), 1, "the session a whole slot behind pages once");
    assert_eq!(samples::read(state.path()).len(), 10);
}
