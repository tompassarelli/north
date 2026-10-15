use std::io::{Read, Write};

use serde::Deserialize;
use serde_json::{Value, json};

mod admission;
mod control;
mod limits;
mod target;
#[cfg(test)]
mod tests;
mod trail;
mod window;

use control::{Control, Reading};
use target::Target;
use window::Window;

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Request {
    Advance {
        previous: Control,
        windows: Vec<Window>,
        workers: Option<i64>,
        at: f64,
        initial: i64,
        margin: f64,
        target: Option<Target>,
    },
    Fresh {
        control: Control,
        at: f64,
    },
    Refusal {
        account: String,
        control: Control,
        now: f64,
    },
    Interval {
        previous: f64,
        elapsed: f64,
        throttled: bool,
        retry_after: f64,
    },
    WeekCeiling {
        window: Window,
        target: Option<Target>,
    },
    TargetFor {
        account: String,
        at: f64,
    },
}

fn answer(request: Request) -> Result<Value, String> {
    Ok(match request {
        Request::Advance { previous, windows, workers, at, initial, margin, target } => {
            let reading = Reading { windows, workers, at, initial, margin, target };
            serde_json::to_value(control::advance(&previous, reading)).map_err(|error| error.to_string())?
        }
        Request::Fresh { control, at } => json!(admission::is_fresh(&control, at)),
        Request::Refusal { account, control, now } => json!(admission::refusal(&account, &control, now, target::target_for(&account, now))),
        Request::Interval { previous, elapsed, throttled, retry_after } => {
            json!(admission::interval(previous, elapsed, throttled, retry_after))
        }
        Request::WeekCeiling { window, target } => json!(window.ceiling(target.as_ref())),
        Request::TargetFor { account, at } => json!(target::target_for(&account, at)?),
    })
}

pub(crate) fn run(_arguments: &[String]) -> u8 {
    let mut input = String::new();
    let outcome = std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|error| error.to_string())
        .and_then(|_| serde_json::from_str::<Request>(&input).map_err(|error| error.to_string()))
        .and_then(answer);
    match outcome {
        Ok(value) => {
            let _ = writeln!(std::io::stdout(), "{value}");
            0
        }
        Err(message) => {
            eprintln!("north pace: {message}");
            2
        }
    }
}
