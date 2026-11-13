use super::*;
use crate::handoff::{Place, placed, script};

#[test]
fn a_missing_or_day_old_map_is_due_for_a_refresh_and_a_fresh_one_is_not() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("north.json");
    assert_eq!(due(&path, SystemTime::UNIX_EPOCH).as_deref(), Some("no map yet"));
    std::fs::write(&path, "{}").unwrap();
    let written = std::fs::metadata(&path).unwrap().modified().unwrap();
    assert_eq!(due(&path, written + Duration::from_secs(3600)), None);
    assert_eq!(due(&path, written + STALE_AFTER + Duration::from_secs(3600)).as_deref(), Some("the map is 25 h old"));
    assert_eq!(unknown_reason(0, 0), None, "a build that lists no test starts no refresh");
    assert_eq!(unknown_reason(1, 10), None, "exactly the share does not pass it");
    let reason = unknown_reason(2, 10).unwrap();
    assert!(reason.contains("2 of the 10") && reason.contains("over 10%"), "{reason}");
}

#[test]
fn a_repository_with_no_checkout_to_refresh_in_starts_no_refresh_unit() {
    let served = tempfile::tempdir().unwrap();
    let why = start(served.path(), "no map yet").unwrap_err();
    assert!(why.contains("has no checkout at") && why.ends_with("so none starts (no map yet)"), "{why}");
    let builders = vec![("gex45".to_owned(), "tom@gex45".to_owned())];
    assert_eq!(placed("laptop", &builders, false, || "gex45".to_owned()), Ok(Place::On("tom@gex45".to_owned())), "a host that is not a build host hands the refresh to one");
    let refused = placed("laptop", &builders, false, || "laptop".to_owned()).unwrap_err();
    assert!(refused.contains("no build host took the refresh") && refused.ends_with("so none starts"), "{refused}");
    assert_eq!(placed("gex45", &builders, false, || unreachable!("a build host places its own refresh")), Ok(Place::Here));
    assert_eq!(placed("laptop", &[], false, || unreachable!("a host with no build hosts listed places its own refresh")), Ok(Place::Here));
    let second = placed("laptop", &builders, true, || unreachable!("a refresh handed on once is not placed again")).unwrap_err();
    assert!(second.contains("already handed on once") && second.ends_with("so none starts"), "{second}");
    assert_eq!(placed("gex45", &builders, true, || unreachable!("a build host runs a refresh handed to it")), Ok(Place::Here));
    assert!(script("north", &[]).contains("NORTH_COVERAGE_HOP=1 \"$n\""), "the forwarded command carries the hop");
    assert!(crate::hosts::plain("north") && !crate::hosts::plain("..") && !crate::hosts::plain(".") && !crate::hosts::plain("a/b"), "a repository name never leaves the code directory");
    let words = script("north", &["--reason".to_owned(), "a 'miss'".to_owned()]);
    assert!(words.starts_with("cd \"$HOME/code/north/main\" && ") && words.ends_with("test coverage refresh --start '--reason' 'a '\\''miss'\\'''"), "{words}");
}
