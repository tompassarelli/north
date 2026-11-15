use super::behind::{Process, Seen, objects};
use crate::runtime::live::Swap;
use super::page::{Pager, conclude};

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

fn rank(sha: &str) -> u8 {
    [OLD, MIDDLE, NEWEST, LATEST].iter().position(|known| *known == sha).map_or(0, |at| at as u8 + 1)
}

/// Records the pages and clears sent; `accept` says whether the coordinator took them.
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
