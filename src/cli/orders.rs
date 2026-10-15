use std::process::ExitCode;

use crate::error::{NorthError, NorthResult};
use crate::fleet::prompt::StandingOrders;
use crate::fleet::registry::{home_dir, state_dir};

const USAGE: &str = "Usage: north orders list | north orders add TEXT...";

pub fn run(arguments: &[String]) -> ExitCode {
    match dispatch(arguments) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {}", error.user_message());
            ExitCode::FAILURE
        }
    }
}

fn dispatch(arguments: &[String]) -> NorthResult<()> {
    let orders = StandingOrders::in_state(&state_dir(&home_dir()?));
    match arguments.split_first() {
        Some((verb, [])) if verb == "list" => {
            print!("{}", orders.read()?);
            Ok(())
        }
        Some((verb, text)) if verb == "add" && !text.is_empty() => {
            println!("{}", orders.add(&text.join(" "))?);
            Ok(())
        }
        _ => Err(NorthError::Usage(USAGE.into())),
    }
}
