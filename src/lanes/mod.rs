mod hardcoded_paths;
mod layout_check;
#[cfg(test)]
mod test_git;

pub(crate) use hardcoded_paths::run as hardcoded_paths;
pub(crate) use layout_check::run as layout_check;

#[cfg(test)]
mod hardcoded_paths_tests;
#[cfg(test)]
mod layout_check_tests;
