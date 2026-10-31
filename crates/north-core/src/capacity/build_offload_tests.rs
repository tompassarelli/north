use super::build_offload::*;

fn heavy() -> Candidate<'static> {
    Candidate { class: "heavy", landing: false, need: None, deferred: true, offloaded_already: false }
}

fn rooms(room: Room) -> impl FnOnce() -> Vec<(String, Room)> {
    move || vec![("gex45".to_owned(), room)]
}

fn declined(route: Route) -> (String, String) {
    let Route::Declined(why) = route else { panic!("{route:?} is not a decline") };
    (why.reason(), why.detail())
}

#[test]
fn capacity_a_deferred_heavy_build_with_room_on_gex45_routes_there_with_reason_offload() {
    assert_eq!(route(&heavy(), rooms(Room::Free)), Route::Offload { host: "gex45".to_owned() });
    assert_eq!(REASON, "OFFLOAD");
}

#[test]
fn capacity_a_host_without_room_or_unreachable_declines_naming_which() {
    assert_eq!(declined(route(&heavy(), rooms(Room::Unreachable))), ("unreachable".to_owned(), "gex45: unreachable or unreadable over ssh".to_owned()));
    let short = route(&heavy(), rooms(Room::Short("1.0 cores free of 20, needs 6".to_owned())));
    assert_eq!(declined(short), ("no room".to_owned(), "gex45: 1.0 cores free of 20, needs 6".to_owned()));
    let mixed = first_free(vec![("dark".to_owned(), Room::Unreachable), ("full".to_owned(), Room::Short("busy".to_owned()))]).unwrap_err();
    assert_eq!((mixed.reason(), mixed.detail()), ("no room".to_owned(), "full: busy; dark: unreachable or unreadable over ssh".to_owned()));
    assert_eq!(route(&heavy(), Vec::new), Route::Local);
    assert_eq!(PROBE_LIMIT_SECONDS, 5);
}

#[test]
fn capacity_gpu_native_warcraft_private_and_landing_work_never_offloads_is_never_probed_and_names_its_class() {
    let refused = [("gpu", Candidate { need: Some("gpu"), ..heavy() }), ("native:wisp", Candidate { need: Some("native:wisp"), ..heavy() }), ("warcraft:wisp", Candidate { need: Some("warcraft:wisp"), ..heavy() }), ("private:smashcraft", Candidate { need: Some("private:smashcraft"), ..heavy() }), ("landing", Candidate { landing: true, ..heavy() }), ("already offloaded", Candidate { offloaded_already: true, ..heavy() })];
    for (class, candidate) in refused {
        let probed = std::cell::Cell::new(false);
        let route = route(&candidate, || {
            probed.set(true);
            vec![("gex45".to_owned(), Room::Free)]
        });
        assert_eq!((declined(route).0, probed.get()), (format!("refused: {class}"), false), "{class}");
    }
    for candidate in [Candidate { class: "moderate", ..heavy() }, Candidate { class: "native", ..heavy() }, Candidate { deferred: false, ..heavy() }] {
        assert_eq!(route(&candidate, || panic!("probed")), Route::Local, "{candidate:?}");
    }
}

#[test]
fn capacity_a_lane_with_a_private_asset_or_an_oversized_diff_stays_on_this_host() {
    assert_eq!(ships(&["src/lib.rs", "Cargo.toml"], 1000), Ok(()));
    let private = ships(&["maps/arena.W3X"], 10).unwrap_err();
    assert_eq!((private.reason(), private.detail()), ("refused: private asset".to_owned(), "maps/arena.W3X".to_owned()));
    assert!(ships(&["assets/a.blp"], 10).is_err());
    let oversized = ships(&["src/lib.rs"], MAX_DIFF_BYTES + 1).unwrap_err();
    assert_eq!(oversized.reason(), "refused: oversized diff");
    assert!(oversized.detail().contains("limit"));
}

#[test]
fn capacity_a_retried_decline_prints_again_only_when_its_reason_changes() {
    let mut last = None;
    let short = |cores: &str| Declined::NoRoom(vec![format!("gex45: {cores} cores free of 20, needs 6")]);
    let printed: Vec<bool> = [short("1.0"), short("2.5"), Declined::Unreachable(vec!["gex45".to_owned()]), short("3.0")].iter().map(|why| why.changes(&mut last)).collect();
    assert_eq!(printed, [true, false, true, true]);
}
