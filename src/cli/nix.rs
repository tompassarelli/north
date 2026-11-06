use north_core::doctor_nix;
use north_core::nix_gc::{self, Env, System};
use std::process::ExitCode;

pub fn gc(arguments: &[String]) -> ExitCode {
    ExitCode::from(nix_gc::run(arguments, &Env::from_env(), &System))
}

pub fn doctor(arguments: &[String]) -> u8 {
    doctor_nix::run(arguments, &Env::from_env(), &System)
}
