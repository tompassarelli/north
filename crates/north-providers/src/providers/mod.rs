pub mod agy;
pub mod claude;
pub mod codex;
#[doc(hidden)]
pub mod fixtures;
pub mod ledger_rows;
pub mod opencode;
pub mod pi;

/// The harness a provider label names, for the providers that run DeepSeek on a lane.
pub fn harness_named(provider: &str) -> Option<&'static dyn opencode::harness::Harness> {
    let harnesses: [&'static dyn opencode::harness::Harness; 2] = [&opencode::harness::OPENCODE, &pi::PI];
    harnesses.into_iter().find(|harness| harness.provider() == provider)
}
