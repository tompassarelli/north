use std::error::Error;
use std::fmt;
use std::io;
use std::process::ExitStatus;


#[derive(Debug)]
pub enum NorthError {
    Io(io::Error),
    Json(serde_json::Error),
    Usage(String),
    Configuration(String),
    Protocol(String),
    State(String),
    Rejected(String),
    Interrupted,
    AppServerExit(ExitStatus),
}

impl NorthError {
    pub fn user_message(&self) -> String {
        match self {
            Self::Protocol(message) if message.contains("401 Unauthorized") => {
                "Couldn’t connect to Codex: authorization was rejected.".into()
            }
            Self::Protocol(message) if message.contains("turn ended as failed") => {
                "Codex couldn’t complete the request.".into()
            }
            Self::Interrupted => "Interrupted".into(),
            Self::State(_) => "North couldn’t read the conversation state.".into(),
            _ => self.to_string(),
        }
    }
}

impl fmt::Display for NorthError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) => formatter.write_str(message),
            Self::Io(error) => write!(formatter, "I/O failed: {error}"),
            Self::Json(error) => write!(formatter, "JSON failed: {error}"),
            Self::Configuration(message) => {
                write!(formatter, "North configuration failed: {message}")
            }
            Self::Protocol(message) => write!(formatter, "Codex protocol failed: {message}"),
            Self::State(message) => write!(formatter, "Incomplete North state: {message}"),
            Self::Rejected(message) => write!(formatter, "Codex rejected the request: {message}"),
            Self::Interrupted => write!(formatter, "Codex turn was interrupted"),
            Self::AppServerExit(status) => {
                write!(formatter, "Codex app-server exited unexpectedly: {status}")
            }
        }
    }
}

impl Error for NorthError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Usage(_)
            | Self::Configuration(_)
            | Self::Protocol(_)
            | Self::State(_)
            | Self::Rejected(_)
            | Self::Interrupted
            | Self::AppServerExit(_) => None,
        }
    }
}

impl From<io::Error> for NorthError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for NorthError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}


pub type NorthResult<T> = Result<T, NorthError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_authorization_messages_do_not_disclose_payloads() {
        let mut inputs = crate::test_inputs::Generator::new(0x41555448);
        let public = NorthError::Protocol("401 Unauthorized".into()).user_message();
        for case in 0..crate::test_inputs::cases() {
            let secret = format!("private-{}-{}", inputs.word(), inputs.word());
            let context = inputs.word();
            for payload in [
                format!("{secret}: 401 Unauthorized: {context}"),
                format!("turn ended as failed: {context}; 401 Unauthorized: {secret}"),
                format!("401 Unauthorized\n{secret}\n{context}"),
            ] {
                let message = NorthError::Protocol(payload).user_message();
                assert!(!message.contains(&secret), "case {case}: payload disclosed");
                assert_eq!(message, public, "case {case}: payload changed the public message");
            }
        }
    }

}
