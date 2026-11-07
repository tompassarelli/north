use super::{ROOT, STAMP, Records, Stamp};
use std::fs;
use std::path::Path;

#[test]
fn a_host_holding_another_hosts_copy_reads_it_and_finds_the_logs_its_records_name_there() {
    let state = tempfile::tempdir().unwrap();
    let root = state.path().join(ROOT);
    fs::create_dir_all(root.join("north")).unwrap();
    let stamp = Stamp { host: "whiterabbit".into(), state: "/home/tom/.local/state".into(), at: 100.0 };
    fs::write(root.join(STAMP), serde_json::to_vec(&stamp).unwrap()).unwrap();
    let elsewhere = Records::of(state.path(), "gex45");
    assert_eq!(elsewhere.home(), root);
    assert_eq!(elsewhere.local(Path::new("/home/tom/.local/state/north/land-queue/k/logs/a.log")), root.join("north/land-queue/k/logs/a.log"));
    assert_eq!(elsewhere.note(130.0).unwrap(), format!("read from whiterabbit's records as mirrored 30 s ago into {}", root.display()));
    let source = Records::of(state.path(), "whiterabbit");
    assert_eq!((source.home(), source.note(130.0)), (state.path(), None));
}
