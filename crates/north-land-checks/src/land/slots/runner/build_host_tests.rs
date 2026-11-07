use north_base::foundation::shim::write_script;

use super::chosen;

#[tokio::test]
async fn checks_before_queueing_go_to_an_idle_build_host_when_this_laptop_has_the_most_room() {
    let directory = tempfile::tempdir().unwrap();
    let north = directory.path().join("north");
    write_script(&north, "case \"$*\" in\n  *'--avoid whiterabbit'*) echo gex45 ;;\n  *'--avoid busy'*) echo 'north capacity offload: no host has room' >&2; exit 1 ;;\n  *) echo 'north capacity offload: whiterabbit has the most room, so the land check runs here' >&2 ;;\nesac\n");
    let north = north.to_str().unwrap();
    assert_eq!(chosen(north, "build-host", Some("whiterabbit")).await.as_deref(), Some("gex45"), "on 2026-11-06 the laptop at load 12 of 24 out-roomed gex45 at 6 of 20, so every lane's checks before queueing got no verdict");
    assert!(chosen(north, "build-host", None).await.is_some_and(|host| !host.is_empty()), "on 2026-11-08 a probe the laptop out-roomed built in its own cold worktree target for 8 minutes beside the laptop's warm offload slots, so this host is named and its checks run in those slots");
    assert_eq!(chosen(north, "build-host", Some("busy")).await, None, "with no host having room the checks run in place under the land-check capacity lease");
}
