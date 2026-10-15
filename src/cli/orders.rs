use crate::error::{NorthError, NorthResult};
use crate::fleet::registry::Fleet;

const USAGE: &str = "Usage: north orders list | north orders add TEXT...";

pub fn run(fleet: &Fleet, words: &[&str]) -> NorthResult<()> {
    let orders = fleet.orders();
    match words {
        ["list"] => print!("{}", orders.read()?),
        ["add", text @ ..] if !text.is_empty() => println!("{}", orders.add(&text.join(" "))?),
        _ => return Err(NorthError::Usage(USAGE.into())),
    }
    Ok(())
}
