#![cfg(test)]

use super::build_offload::*;

fn heavy() -> Candidate<'static> {
    Candidate { class: "heavy", landing: false, need: None, deferred: true, agent: false, offloaded_already: false }
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
    let refused = [("gpu", Candidate { need: Some("gpu"), ..heavy() }), ("native:wisp", Candidate { need: Some("native:wisp"), ..heavy() }), ("warcraft:wisp", Candidate { need: Some("warcraft:wisp"), ..heavy() }), ("private:makgora", Candidate { need: Some("private:makgora"), ..heavy() }), ("landing", Candidate { landing: true, ..heavy() }), ("already offloaded", Candidate { offloaded_already: true, ..heavy() })];
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

fn env_of(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
    move |name| pairs.iter().find(|(key, _)| *key == name).map(|(_, value)| (*value).to_owned())
}

#[test]
fn capacity_an_offloaded_run_record_carries_ship_seconds_cache_state_and_the_host_it_ran_on() {
    let warm = ShipCost::read(env_of(&[(SHIP_SECONDS_ENV, "4.256"), (SLOT_TARGET_ENV, "1"), (SLOT_HEAD_ENV, "abc123")]), "gex45").unwrap();
    assert_eq!(warm.fields(), vec![("host", "gex45".into()), ("shipSeconds", serde_json::json!(4.26)), ("cacheState", "warm".into()), ("slotHead", "abc123".into())]);
    let fresh = ShipCost::read(env_of(&[(SHIP_SECONDS_ENV, "31"), (SLOT_TARGET_ENV, "0"), (SLOT_HEAD_ENV, "")]), "gex45").unwrap();
    assert_eq!(fresh.cache_state, CacheState::Cold);
    assert_eq!(fresh.fields().iter().map(|(key, _)| *key).collect::<Vec<_>>(), ["host", "shipSeconds", "cacheState"]);
}

#[test]
fn capacity_a_slot_is_warm_only_when_its_target_existed_and_it_held_a_previous_head() {
    assert_eq!(CacheState::of(true, Some("abc")), CacheState::Warm);
    assert_eq!(CacheState::of(false, Some("abc")), CacheState::Cold);
    assert_eq!(CacheState::of(true, None), CacheState::Cold);
    assert_eq!(CacheState::of(true, Some("")), CacheState::Cold);
}

#[test]
fn capacity_a_run_that_was_not_offloaded_or_a_garbled_ship_time_carries_no_ship_cost() {
    assert_eq!(ShipCost::read(env_of(&[]), "gex45"), None);
    assert_eq!(ShipCost::read(env_of(&[(SHIP_SECONDS_ENV, "soon")]), "gex45"), None);
    assert_eq!(ShipCost::read(env_of(&[(SHIP_SECONDS_ENV, "-1")]), "gex45"), None);
}

#[test]
fn capacity_the_median_ship_time_takes_the_middle_value_or_the_mean_of_the_middle_two() {
    assert_eq!(median(&[9.0, 1.0, 4.0, 2.0, 30.0]), Some(4.0));
    assert_eq!(median(&[1.0, 2.0, 4.0, 10.0]), Some(3.0));
    assert_eq!(median(&[]), None);
}

mod properties {
    use super::*;
    use proptest::prelude::*;

    fn admission() -> impl Strategy<Value = Admission> {
        (0.0..64.0f64, 0.0..64.0f64, 0.0..100.0f64, 1.0..100.0f64).prop_map(|(limit_cpus, committed_cpus, cpu_pressure_percent, pressure_high_percent)| Admission { limit_cpus, committed_cpus, cpu_pressure_percent, pressure_high_percent, land_check_waiting: false })
    }

    fn named<T>(items: Vec<T>) -> Vec<(String, T)> {
        items.into_iter().enumerate().map(|(index, item)| (format!("host{index}"), item)).collect()
    }

    proptest! {
        #[test]
        fn prop_route_never_picks_a_host_whose_room_is_below_the_need_and_picks_the_first_with_room(
            admissions in proptest::collection::vec(admission(), 0..6),
            needs in 0.0..32.0f64,
            reserved in 0.0..16.0f64,
        ) {
            let hosts = named(admissions);
            let fits = |admission: &Admission| admission.cpu_pressure_percent < admission.pressure_high_percent && admission.limit_cpus - admission.committed_cpus - reserved >= needs;
            let rooms: Vec<(String, Room)> = hosts.iter().map(|(name, admission)| (name.clone(), admission.shortfall(needs, reserved).map_or(Room::Free, Room::Short))).collect();
            let expected = hosts.iter().find(|(_, admission)| fits(admission)).map(|(name, _)| name.clone());
            let routed = match route(&Candidate { class: "heavy", landing: false, need: None, deferred: true, agent: false, offloaded_already: false }, || rooms) {
                Route::Offload { host } => Some(host),
                Route::Local | Route::Declined(_) => None,
            };
            prop_assert_eq!(routed, expected);
        }

        #[test]
        fn prop_a_command_naming_a_local_tree_is_refused_and_one_naming_none_is_not(
            words in proptest::collection::vec("[a-z/ -]{0,12}", 0..5),
            named_tree in proptest::option::of(0usize..2),
        ) {
            let trees = ["/home/user/code/north/worktrees/lane", "/home/user/code/north/main"];
            let mut command = words;
            if let Some(index) = named_tree {
                command.push(format!("--manifest-path={}/Cargo.toml", trees[index]));
            }
            let names = command.iter().any(|word| trees.iter().any(|tree| word.contains(tree)));
            prop_assert_eq!(names_a_path_here(&command, &trees).is_err(), names);
            prop_assert_eq!(named_tree.is_some(), names);
        }
    }
}

fn agent_build() -> Candidate<'static> {
    Candidate { deferred: false, agent: true, ..heavy() }
}

#[test]
fn capacity_an_agents_heavy_build_the_laptop_would_admit_offloads_to_the_first_host_with_room() {
    let hosts = || vec![("gex45".to_owned(), Room::Short("1.0 cores free of 20, needs 4".to_owned())), ("ccx33".to_owned(), Room::Free)];
    assert_eq!(route(&agent_build(), hosts), Route::Offload { host: "ccx33".to_owned() });
    assert_eq!(route(&agent_build(), rooms(Room::Free)), Route::Offload { host: "gex45".to_owned() });
}

#[test]
fn capacity_an_agents_heavy_build_stays_for_local_admission_only_when_no_host_has_room_or_none_is_configured() {
    let full = vec![("gex45".to_owned(), Room::Short("busy".to_owned())), ("ccx33".to_owned(), Room::Unreachable)];
    assert_eq!(declined(route(&agent_build(), || full)).0, "no room");
    assert_eq!(route(&agent_build(), Vec::new), Route::Local);
    let tom = Candidate { agent: false, ..agent_build() };
    assert_eq!(route(&tom, || panic!("probed")), Route::Local, "a build outside a fleet session that this host admits is never probed");
    for need in ["gpu", "native:wisp"] {
        let pinned = Candidate { need: Some(need), ..agent_build() };
        assert_eq!(declined(route(&pinned, || panic!("probed"))).0, format!("refused: {need}"));
    }
    let landing = Candidate { landing: true, ..agent_build() };
    assert_eq!(declined(route(&landing, || panic!("probed"))).0, "refused: landing");
}
