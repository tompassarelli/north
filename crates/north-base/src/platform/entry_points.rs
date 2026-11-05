use std::ffi::OsStr;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryPoint {
    SafePush,
}

/// The names that select an entry point from argv[0]; the runtime installer links each one to the north binary.
pub const ENTRY_POINTS: [(&str, EntryPoint); 1] = [("safe-push", EntryPoint::SafePush)];

/// The entry point argv[0] names, by its final path component.
pub fn named(argv0: &OsStr) -> Option<EntryPoint> {
    let name = Path::new(argv0).file_name()?.to_str()?;
    ENTRY_POINTS.iter().find(|(entry, _)| *entry == name).map(|(_, point)| *point)
}

impl EntryPoint {
    /// Variables the entry's launcher scrubbed before it ran, so a link that bypasses the launcher keeps the same environment.
    pub fn scrubbed_variables(self) -> &'static [&'static str] {
        match self {
            Self::SafePush => &["CODEX_HOME"],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn a_bare_name_and_any_path_to_it_select_safe_push() {
        for argv0 in ["safe-push", "/home/tom/.local/bin/safe-push", "./safe-push"] {
            assert_eq!(named(OsStr::new(argv0)), Some(EntryPoint::SafePush), "{argv0}");
        }
    }

    #[test]
    fn north_and_lookalikes_select_nothing() {
        for argv0 in ["north", "/runtimes/abc/north", "safe-push2", "xsafe-push", ""] {
            assert_eq!(named(OsStr::new(argv0)), None, "{argv0}");
        }
    }

    proptest! {
        #[test]
        fn a_basename_outside_the_entry_points_runs_north(directory in "[a-z/]{0,12}", name in "[a-z-]{1,12}") {
            prop_assume!(ENTRY_POINTS.iter().all(|(entry, _)| *entry != name));
            let argv0 = format!("{directory}/{name}");
            prop_assert_eq!(named(OsStr::new(&argv0)), None);
        }
    }
}
