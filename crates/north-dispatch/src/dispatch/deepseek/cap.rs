//! The opencode slot cap (north#1020): DeepSeek workers run beside the Claude workers, not out of them, so the sessions
//! that fill this cap are opencode sessions alone and no Claude worker, however many, counts against it.
use super::PROVIDER;

/// The opencode sessions among `providers`, one provider name per running session.
pub fn running<'a>(providers: impl IntoIterator<Item = &'a str>) -> usize {
    providers.into_iter().filter(|provider| *provider == PROVIDER).count()
}

/// Whether another opencode worker may start with `running` opencode sessions up and `cap` allowed.
pub fn admit(running: usize, cap: usize) -> Result<(), String> {
    if running >= cap { Err(format!("{running} opencode workers run, the cap of {cap} in harness/opencode.toml [dispatch] slot_cap; it starts once one ends")) } else { Ok(()) }
}
