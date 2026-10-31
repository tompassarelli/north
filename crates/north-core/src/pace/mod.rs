use serde::Deserialize;
use serde_json::{Value, json};

pub mod admission;
pub mod control;
pub mod exhaustion;
pub mod limits;
pub mod target;
#[cfg(test)]
mod replay_tests;
#[cfg(test)]
mod tests;
mod trail;
pub mod window;

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
    TargetFor {
        account: String,
        at: f64,
    },
}

fn answer(request: Request) -> Result<Value, String> {
    Ok(match request {
        Request::Advance {
            previous,
            windows,
            workers,
            at,
            initial,
            margin,
            target,
        } => {
            let reading = Reading {
                windows,
                workers,
                at,
                initial,
                margin,
                target,
            };
            serde_json::to_value(control::advance(&previous, reading))
                .map_err(|error| error.to_string())?
        }
        Request::Fresh { control, at } => json!(admission::is_fresh(&control, at)),
        Request::Refusal {
            account,
            control,
            now,
        } => json!(admission::refusal(&account, &control, now)),
        Request::Interval {
            previous,
            elapsed,
            throttled,
            retry_after,
        } => {
            json!(admission::interval(
                previous,
                elapsed,
                throttled,
                retry_after
            ))
        }
        Request::TargetFor { account, at } => json!(target::target_for(&account, at)?),
    })
}

pub fn answer_value(value: Value) -> Result<Value, String> {
    answer(serde_json::from_value(value).map_err(|e| e.to_string())?)
}
