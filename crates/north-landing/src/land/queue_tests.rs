use super::*;

#[test]
fn the_key_is_the_first_sixteen_hex_of_the_origin_hash_the_bash_lock_uses() {
    assert_eq!(key("https://github.com/tompassarelli/north.git").len(), 16);
    assert_eq!(key("https://github.com/tompassarelli/north.git"), "de2e0e8479546ba9");
    assert_eq!(key("https://github.com/tompassarelli/north"), "de2e0e8479546ba9");
    assert_eq!(key("https://github.com/tompassarelli/north/"), "de2e0e8479546ba9");
}

#[test]
fn a_ticket_renamed_to_sort_first_keeps_the_place_its_recorded_time_gives_it() {
    let directory = tempfile::tempdir().unwrap();
    let ticket = |branch: &str, micros| Ticket { branch: branch.to_owned(), queued_micros: micros, ..Default::default() };
    write(&directory.path().join(file_name(1_000, "earlier")), &ticket("earlier", Some(1_000))).unwrap();
    write(&directory.path().join(file_name(2_000, "later")), &ticket("later", Some(2_000))).unwrap();
    write(&directory.path().join(file_name(3_000, "old")), &ticket("old", None)).unwrap();
    fs::rename(directory.path().join(file_name(2_000, "later")), directory.path().join("1-later.json")).unwrap();
    assert_eq!(queued_micros(&directory.path().join("1-later.json")), 2_000);
    assert_eq!(queued_micros(&directory.path().join(file_name(3_000, "old"))), 3_000, "a ticket written before the field existed keeps its file-name id");
    let order: Vec<String> = collect(directory.path()).unwrap().into_iter().map(|(_, ticket)| ticket.branch).collect();
    assert_eq!(order, ["earlier", "later", "old"]);
}
