use super::build_offload::*;

fn heavy() -> Candidate<'static> {
    Candidate { class: "heavy", landing: false, need: None, deferred: true, offloaded_already: false }
}

fn rooms(room: Room) -> impl FnOnce() -> Vec<(String, Room)> {
    move || vec![("gex45".to_owned(), room)]
}

#[test]
fn capacity_a_deferred_heavy_build_with_room_on_gex45_routes_there_with_reason_offload() {
    assert_eq!(route(&heavy(), rooms(Room::Free)), Route::Offload { host: "gex45".to_owned() });
    assert_eq!(REASON, "OFFLOAD");
}

#[test]
fn capacity_a_host_without_room_or_unreachable_leaves_the_build_on_the_local_path() {
    assert_eq!(route(&heavy(), rooms(Room::Unreachable)), Route::Local);
    assert_eq!(route(&heavy(), rooms(Room::Short("1.0 cores free of 20, needs 6".to_owned()))), Route::Local);
    let refused = first_free(vec![("gex45".to_owned(), Room::Unreachable)]).unwrap_err();
    assert_eq!(refused, ["gex45: unreachable or unreadable over ssh"]);
    assert_eq!(PROBE_LIMIT_SECONDS, 5);
}

#[test]
fn capacity_gpu_native_warcraft_private_and_landing_work_never_offloads_and_is_never_probed() {
    for need in ["gpu", "native:wisp", "warcraft:wisp", "client:smashcraft", "private:smashcraft"] {
        let candidate = Candidate { need: Some(need), ..heavy() };
        let probed = std::cell::Cell::new(false);
        let route = route(&candidate, || {
            probed.set(true);
            vec![("gex45".to_owned(), Room::Free)]
        });
        assert_eq!((route, probed.get()), (Route::Local, false), "{need}");
    }
    for candidate in [Candidate { class: "moderate", ..heavy() }, Candidate { class: "native", ..heavy() }, Candidate { landing: true, ..heavy() }, Candidate { deferred: false, ..heavy() }, Candidate { offloaded_already: true, ..heavy() }] {
        assert_eq!(route(&candidate, rooms(Room::Free)), Route::Local, "{candidate:?}");
    }
}

#[test]
fn capacity_a_lane_with_a_private_asset_or_an_oversized_diff_stays_on_this_host() {
    assert_eq!(ships(&["src/lib.rs", "Cargo.toml"], 1000), Ok(()));
    assert!(ships(&["maps/arena.W3X"], 10).unwrap_err().contains("private asset"));
    assert!(ships(&["assets/a.blp"], 10).is_err());
    assert!(ships(&["src/lib.rs"], MAX_DIFF_BYTES + 1).unwrap_err().contains("limit"));
}
