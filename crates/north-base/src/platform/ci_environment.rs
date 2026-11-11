pub const STEP_PREFIX: &str = "Environment: ";

pub fn failed_only_in_environment<'a>(steps: impl IntoIterator<Item = (&'a str, Option<&'a str>)>) -> bool {
    let mut failed = steps.into_iter().filter(|(_, conclusion)| *conclusion == Some("failure")).peekable();
    failed.peek().is_some() && failed.all(|(name, _)| name.starts_with(STEP_PREFIX))
}
