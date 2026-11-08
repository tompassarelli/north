use super::{Reading, Store, graph_reading};
use crate::loops::{Role, check, parse};
use std::time::{Duration, SystemTime};

#[test]
fn a_graph_loop_is_judged_from_its_writer_s_newest_tick_and_fails_once_that_tick_is_over_twice_its_cadence_old() {
    let registry = "[[loop]]\nname = \"eta\"\ndecision = \"d\"\nsites = [\"s\"]\nsignal = \"p75 wall\"\nupdated_by = \"north tune\"\ncadence_hours = 24\nstore = \"graph\"\nwriter = \"tune\"\n";
    let loops = parse(registry).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("graph.db");
    north_graph_loops::tell(&db, "tune", &north_graph_loops::Learned::default(), &[("eta_minutes opus feature".to_owned(), serde_json::json!(92.0))].into(), 1_800_000_000).unwrap();
    let ticks = north_graph_loops::ticks(&db);
    let judged = |hours: u64| check(&loops, &|_: Role| true, &|store| match store {
        Store::Graph { writer } => graph_reading(&ticks, writer),
        Store::Record(_) => Reading::Never,
    }, &|_: &str| None, Vec::new(), SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000 + hours * 3_600)).failing;
    assert_eq!((judged(47), judged(49)), (Vec::<String>::new(), vec!["eta".to_owned()]));
    assert_eq!(graph_reading(&ticks, "flow"), Reading::Never);
}
