//! The capacity gate's shims: the `cargo`, `rustc` and other programs on PATH that hand a run to `north capacity gate`, which then runs the real program, the next one on PATH that is no shim.
use std::path::Path;

const SHIM_MARKER: &str = "# north capacity gate shim: the real program is the next one on PATH that is not a shim";
const SHIMS_OUTPUT: &str = "-capacity-gate-shims/bin/";

/// Whether `path` is one of the gate's shims rather than the program it stands in for.
pub fn is_shim(path: &Path) -> bool {
    use std::io::Read;
    if std::fs::canonicalize(path).is_ok_and(|real| real.to_string_lossy().contains(SHIMS_OUTPUT)) {
        return true;
    }
    let mut head = Vec::with_capacity(4096);
    let _ = std::fs::File::open(path).and_then(|file| file.take(4096).read_to_end(&mut head));
    let head = String::from_utf8_lossy(&head);
    head.starts_with("#!") && (head.lines().any(|line| line == SHIM_MARKER) || head.contains(SHIMS_OUTPUT))
}
