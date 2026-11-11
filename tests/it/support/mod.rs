pub mod template;
pub mod tree;

use std::collections::BTreeMap;
use std::sync::{LazyLock, Mutex, PoisonError};

/// The absolute path of a binary of this package: the compile-time value Cargo sets, else the run-time value a Buck2 test target sets relative to the project root.
macro_rules! bin_exe {
    ($variable:literal) => {
        $crate::support::resolve_bin_exe($variable, option_env!($variable))
    };
}
pub(crate) use bin_exe;

static RESOLVED: LazyLock<Mutex<BTreeMap<&'static str, &'static str>>> = LazyLock::new(Mutex::default);

pub fn resolve_bin_exe(variable: &'static str, built: Option<&'static str>) -> &'static str {
    built.unwrap_or_else(|| {
        let mut resolved = RESOLVED.lock().unwrap_or_else(PoisonError::into_inner);
        resolved.entry(variable).or_insert_with(|| {
            let relative = std::env::var_os(variable).unwrap_or_else(|| panic!("{variable} is set neither at compile time nor at run time"));
            let absolute = std::path::absolute(&relative).unwrap_or_else(|error| panic!("{variable}: {error}"));
            Box::leak(absolute.into_os_string().into_string().unwrap_or_else(|path| panic!("{variable} is not UTF-8: {path:?}")).into_boxed_str())
        })
    })
}
