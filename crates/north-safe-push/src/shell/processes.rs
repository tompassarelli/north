use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use north_base::foundation::step::Step;
use north_base::platform::process::{exit_status, on_path};

pub trait Processes {
    fn on_path(&self, program: &str) -> bool;
    fn run_with_input(&self, program: &Path, directory: &Path, arguments: &[&str], input: &str) -> std::io::Result<Step>;
    fn exit_code(&self, program: &str, arguments: &[&str]) -> Option<i32>;
}

pub struct Live;

impl Processes for Live {
    fn on_path(&self, program: &str) -> bool {
        on_path(program)
    }

    fn run_with_input(&self, program: &Path, directory: &Path, arguments: &[&str], input: &str) -> std::io::Result<Step> {
        let mut child = Command::new(program).current_dir(directory).args(arguments).stdin(Stdio::piped()).spawn()?;
        let _ = child.stdin.take().map(|mut stdin| stdin.write_all(input.as_bytes()));
        Ok(exit_status(child.wait()))
    }

    fn exit_code(&self, program: &str, arguments: &[&str]) -> Option<i32> {
        Command::new(program).args(arguments).status().ok().and_then(|status| status.code())
    }
}
