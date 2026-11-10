//! `north fleet held`: the messages the fleet held back from waking their recipient, page-flood folds included,
//! read from the held log alone so the proxy sees what a fold kept from it in well under a second.
use crate::error::{NorthError, NorthResult};
use crate::fleet::flood::WINDOW_MS;
use crate::fleet::registry::Fleet;
use crate::fleet::runtime::now_ms;

pub(super) fn run(fleet: &Fleet, flags: &[&str]) -> NorthResult<()> {
    let window_ms = match flags {
        [] => WINDOW_MS,
        ["--since", window] => crate::cli::views::duration_ms(window).map_err(NorthError::Usage)?,
        _ => return Err(NorthError::Usage("north fleet held [--since DURATION]".into())),
    };
    let now = now_ms();
    let held = fleet.held_since(now.saturating_sub(window_ms));
    if held.is_empty() {
        println!("nothing held in the last {} minutes", window_ms / 60_000);
    }
    for message in held {
        let first = message.text.lines().next().unwrap_or_default();
        println!("{}m ago {} -> {}: {first}", now.saturating_sub(message.at_ms) / 60_000, message.from, message.to);
    }
    Ok(())
}
