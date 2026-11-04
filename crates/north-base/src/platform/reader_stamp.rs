use std::fs;
use std::path::{Path, PathBuf};

pub const ROOT: &str = "north/reader-stamps";

pub fn path(state: &Path, signal: &str) -> PathBuf {
    state.join(ROOT).join(signal)
}

pub fn touch(state: &Path, signal: &str) {
    let stamp = path(state, signal);
    if let Some(parent) = stamp.parent()
        && fs::create_dir_all(parent).is_ok()
    {
        let _ = fs::write(stamp, b"");
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_reader_stamp_mirrors_its_signal_under_the_stamp_root_and_is_rewritten_on_each_read() {
        let state = tempfile::tempdir().unwrap();
        super::touch(state.path(), "agents/usage-gate.json");
        let stamp = state.path().join("north/reader-stamps/agents/usage-gate.json");
        std::fs::write(&stamp, b"old").unwrap();
        super::touch(state.path(), "agents/usage-gate.json");
        assert_eq!(std::fs::read(&stamp).unwrap(), b"");
        assert_eq!(super::path(state.path(), "agents/x.json"), stamp.with_file_name("x.json"));
    }
}
