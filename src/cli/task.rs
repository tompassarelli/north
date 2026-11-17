use std::process::ExitCode;

use crate::error::{NorthError, NorthResult};

const USAGE: &str = "Usage: north task comment REPO#N --body TEXT";

pub fn run(arguments: &[String]) -> ExitCode {
    let outcome = match arguments {
        [task, verb, ..] if task == "task" && verb == "comment" && let Some(code) = north_worker_sandbox::worker_sandbox::reenter_lane_only(arguments) => return ExitCode::from(code),
        [task, verb, reference, flag, body] if task == "task" && verb == "comment" && flag == "--body" => comment(reference, body),
        _ => Err(NorthError::Usage(USAGE.into())),
    };
    match outcome {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {}", error.user_message());
            ExitCode::from(error.exit_code())
        }
    }
}

fn comment(reference: &str, body: &str) -> NorthResult<ExitCode> {
    let (repository, number) = reference.split_once('#').and_then(|(repository, number)| Some((repository, number.parse::<u64>().ok()?))).filter(|(repository, _)| !repository.is_empty()).ok_or_else(|| NorthError::Usage(format!("{reference} is not REPO#N")))?;
    let repository = if repository.contains('/') { repository.to_owned() } else { format!("tompassarelli/{repository}") };
    crate::platform::github::comment(&repository, number, body).map_err(NorthError::Usage)?;
    println!("commented on {repository}#{number}");
    Ok(ExitCode::SUCCESS)
}
