use std::collections::BTreeSet;

use north_base::foundation::clock::Timestamp;
use north_session_host::held::HeldUnit;

use super::super::store::SessionRow;
use super::{Table, strays};

const NOW: u64 = 1_794_041_323_000;
const HOUR: u64 = 3_600_000;

fn unit(name: &str, hours: u64, attached: bool) -> HeldUnit {
    HeldUnit { unit: format!("north-fleet-{name}-0666733b.service"), active_since_ms: NOW - hours * HOUR, attached }
}

fn row(name: &str, status: &str, minutes_ago: u64) -> SessionRow {
    let updated = Timestamp::from_unix_seconds(i64::try_from((NOW - minutes_ago * 60_000) / 1000).unwrap()).iso();
    SessionRow { name: name.to_owned(), host: "whiterabbit".to_owned(), status: status.to_owned(), created: updated.clone(), updated, ..SessionRow::default() }
}

#[test]
fn a_detached_unit_whose_session_no_fresh_registry_holds_live_is_a_stray_and_every_other_unit_is_kept() {
    let units = [
        unit("north-643-d2035", 9, false),
        unit("north-737-d0241", 7, false),
        unit("gex45-queue", 1, true),
        unit("north-706-d2331", 8, false),
        unit("north-801-d1200", 0, false),
        unit("north-650-d0100", 5, false),
        unit("north-660-d0100", 5, false),
    ];
    let rows = [row("north-643-d2035", "exited", 1), row("gex45-queue", "working", 1), row("north-706-d2331", "working", 600), row("north-650-d0100", "idle", 1)];
    let live_here = BTreeSet::from(["north-660-d0100".to_owned()]);
    let found: Vec<String> = strays(&units, &live_here, &Table { rows: &rows, cached_ms: NOW - 60_000 }, NOW).into_iter().map(|stray| stray.unit).collect();
    assert_eq!(found, ["north-fleet-north-643-d2035-0666733b.service", "north-fleet-north-737-d0241-0666733b.service"]);
    assert!(strays(&units, &live_here, &Table { rows: &rows, cached_ms: NOW - 3 * HOUR }, NOW).is_empty());
}
