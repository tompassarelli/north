use super::super::bounded;
use super::super::layout::Layout;
use std::process::Command;
use std::time::Duration;

const IMPORT_LIMIT: Duration = Duration::from_secs(300);

pub(super) fn import(layout: &Layout) {
    let imported = bounded::status(Command::new(layout.current().join("north")).args(["graph", "ground", "import"]), IMPORT_LIMIT);
    if !imported.is_ok_and(|status| status.success()) {
        eprintln!("north runtime update: grounding import failed; the next update retries it");
    }
}
