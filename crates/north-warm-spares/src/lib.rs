//! Spare workers started ahead on a build host, taken by a launch whose spec matches (north#1239), with the host types a launch there shares.
pub mod host;
pub mod machine;
pub mod names;
pub mod spares;
pub mod ssh;
#[cfg(any(test, feature = "test-support"))]
pub mod fake_host;

#[cfg(test)]
mod spares_tests;
