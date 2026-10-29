use std::process::ExitStatus;

pub(super) const DEFERRED: u8 = 75;

pub(super) struct Failure {
    pub(super) code: u8,
    pub(super) message: String,
}

pub(super) fn failed(code: u8, message: impl Into<String>) -> Failure {
    Failure { code, message: message.into() }
}

impl From<std::io::Error> for Failure {
    fn from(error: std::io::Error) -> Self {
        failed(1, error.to_string())
    }
}

pub(super) fn code(status: ExitStatus) -> u8 {
    status.code().and_then(|code| u8::try_from(code).ok()).unwrap_or(1)
}
