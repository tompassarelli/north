use super::super::issues::IssueRef;
use super::super::record;
use std::path::Path;

#[test]
fn a_cloud_launch_with_an_item_records_it_so_dispatch_holds_that_issue() {
    let (home, pace) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    std::fs::write(home.path().join(".claude.json"), "{}").unwrap();
    let mut run = |_: &Path, program: &str, _: &[String]| -> Result<String, String> {
        Ok(if program == "claude" { "Created cloud session: https://claude.ai/code/session_01AbcXyz\n".into() } else { String::new() })
    };
    let item = super::item("smashcraft#7").unwrap();
    let launched = super::launch("smashcraft", "sc-7", "brief", &[], Path::new(super::SOURCES), home.path(), &mut run).unwrap();
    super::hold(&item, &launched, "nexus", pace.path(), 1_000.0).unwrap();

    let holds = record::holds(&record::read(pace.path()), 1_000.0 + 3600.0, &Default::default(), &Default::default());
    let reason = holds.get(&IssueRef::new("smashcraft", 7)).expect("smashcraft#7 is held");
    assert!(reason.contains("session_01AbcXyz") && reason.contains(super::HOST), "{reason}");
    assert!(super::item("smashcraft 7").is_err() && super::item("see smashcraft#7").is_err());
}
