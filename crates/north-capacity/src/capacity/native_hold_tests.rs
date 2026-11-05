#![cfg(test)]

use serde_json::json;

use super::admission::{Class, Facts, Reason, admit, decide};
use super::native_hold::{PairRefusal, queue_pair_leases};
use super::tests::facts;

// KEEP(bug): a pair the Wisp queue holds for a live process is refused to a native lease, naming a different free pair; regression of a0b83f6f8
#[test]
fn a_pair_the_wisp_queue_holds_for_a_live_process_is_refused_to_a_native_lease_with_a_different_free_pair() {
    let directory = std::env::temp_dir().join(format!("capture-queue-{}", std::process::id()));
    std::fs::create_dir_all(directory.join("leases")).unwrap();
    let write = |name: &str, host: &str, pair: u32, pid: u32| std::fs::write(directory.join("leases").join(name), json!({ "host": host, "pair": pair, "owner": "wisp-sylv-cloak", "pid": pid, "since": "" }).to_string()).unwrap();
    write("gex45-16.json", "gex45", 16, std::process::id());
    write("gex45-17.json", "gex45", 17, u32::MAX);
    write("elsewhere-18.json", "elsewhere", 18, std::process::id());
    let held = queue_pair_leases(&directory, "gex45");
    std::fs::remove_dir_all(&directory).unwrap();
    assert_eq!(held.iter().map(|lease| (lease.pair, lease.owner.as_str())).collect::<Vec<_>>(), vec![(16, "wisp-sylv-cloak")]);
    let ask = |pair: u32| Facts { owner: Some("sc-lk-gale".to_owned()), pair: Some(pair), pair_leases: held.clone(), ..facts(Class::Native) };
    let refused = admit(&ask(16), |_| None);
    assert_eq!(refused.reason, Reason::PairHeld);
    assert_eq!(refused.pair, Some(PairRefusal { pair: 16, holder: "wisp-sylv-cloak".to_owned(), free_pair: 0 }));
    assert_eq!(decide(&ask(0)), Reason::Run);
}
