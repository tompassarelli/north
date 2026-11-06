use super::*;

const EVIDENCE: &str = "test\tfailures\tcatches\tlast_seen\n\
caught::recently\t3\t2\t2026-10-20T01:00:00Z\n\
caught::long_ago\t1\t1\t2026-05-01T01:00:00Z\n\
flaky::only\t9\t0\t2026-11-01T01:00:00Z\n";

const LATEST: &str = "baseline\tcaught::recently\tpass\n\
baseline\tcaught::long_ago\tpass\n\
baseline\tflaky::only\tpass\n\
baseline\tkills::alone\tpass\n\
baseline\tkills::with_a_twin\tpass\n\
baseline\tkills::twin\tpass\n\
baseline\tpins::wire_format\tpass\n\
baseline\tbroken::here\tfail\n\
mutant\tm-000\tsrc/a.rs:3:9 ==->!=\tran\n\
mutant\tm-001\tsrc/a.rs:7:2 true->false\tran\n\
mutant\tm-002\tsrc/b.rs:1:1 +->-\tdead\n\
kill\tm-000\tkills::with_a_twin\n\
kill\tm-000\tkills::twin\n\
kill\tm-000\tflaky::only\n";

const EARLIER: &str = "baseline\tkills::alone\tpass\n\
baseline\tgone::old\tpass\n\
mutant\tn-100\tsrc/c.rs:4:4 <-><=\tran\n\
kill\tn-100\tkills::alone\n";

#[test]
fn retire_proposes_only_tests_with_no_recent_catch_no_unique_kill_and_no_contract() {
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, text: &str| {
        let path = dir.path().join(name);
        fs::write(&path, text).unwrap();
        path
    };
    let args = RetireArgs {
        repo: "north".to_owned(),
        matrices: vec![write("2026-10-06.tsv", EARLIER), write("2026-11-06.tsv", LATEST)],
        evidence: write("north.tsv", EVIDENCE),
        contracts: vec!["wire_format".to_owned()],
        date: "2026-11-06".to_owned(),
        days: 90,
    };
    assert_eq!(
        retire(&args).ok(),
        Some("- [ ] north retirement sweep 2026-11-06: delete or justify the 4 tests with no catch since 2026-08-08, no unique kill among 3 live mutants and no listed `Test-contract:` (1 caught, 1 unique killers, 1 contracts kept; 1 failed on the untouched tree and were not judged): `caught::long_ago`, `flaky::only`, `kills::{twin, with_a_twin}`".to_owned())
    );
}
