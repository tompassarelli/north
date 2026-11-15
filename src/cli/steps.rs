use std::path::PathBuf;
use std::process::Command;

pub(super) fn north(name: &str) -> Result<PathBuf, u8> {
    std::env::current_exe().map_err(|error| {
        eprintln!("{name}: {error}");
        1
    })
}

pub(super) fn run_in_order(name: &str, steps: Vec<Command>) -> u8 {
    for mut step in steps {
        let code = match step.status() {
            Ok(status) => status.code().and_then(|code| u8::try_from(code).ok()).unwrap_or(1),
            Err(error) => {
                eprintln!("{name}: {}: {error}", step.get_program().to_string_lossy());
                1
            }
        };
        if code != 0 {
            return code;
        }
    }
    0
}
