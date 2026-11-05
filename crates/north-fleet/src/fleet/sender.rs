use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::{NorthError, NorthResult};

const SESSION: &str = "session:";
const SERVICE: &str = "service:";
const TOM: &str = "tom";

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Sender {
    Tom,
    Session(String),
    Service(String),
}

impl Sender {
    pub fn fleet() -> Self {
        Self::Service("north-fleet".into())
    }

    pub fn service(unit: &str) -> NorthResult<Self> {
        let unit = unit.strip_prefix(SERVICE).unwrap_or(unit);
        if unit.is_empty() || unit.chars().any(char::is_whitespace) {
            return Err(NorthError::Usage(format!("not a service unit name: {unit:?}")));
        }
        Ok(Self::Service(unit.to_owned()))
    }

    pub fn declared(from: Option<&str>, caller: Option<String>) -> NorthResult<Self> {
        match (from, caller) {
            (Some(text), _) => match Self::from_str(text)? {
                Self::Tom => Err(NorthError::Usage("--from tom is refused: only the fleet TUI and north proxy send as Tom".into())),
                sender => Ok(sender),
            },
            (None, Some(session)) => Ok(Self::Session(session)),
            (None, None) => Err(NorthError::Usage("north fleet send from outside a fleet session needs --from service:UNIT naming the timer or daemon that sends".into())),
        }
    }

    pub fn typed(caller: Option<String>) -> Self {
        caller.map_or(Self::Tom, Self::Session)
    }

    pub fn session(&self) -> Option<&str> {
        match self {
            Self::Session(name) => Some(name),
            Self::Tom | Self::Service(_) => None,
        }
    }

    pub fn on_host(&self, host: &str) -> Self {
        match self {
            Self::Tom => Self::Tom,
            Self::Session(name) => Self::Session(format!("{name}@{host}")),
            Self::Service(unit) => Self::Service(format!("{unit}@{host}")),
        }
    }

    pub fn prefix(&self) -> Option<String> {
        match self {
            Self::Tom => None,
            Self::Session(name) => Some(name.clone()),
            Self::Service(unit) => Some(format!("{SERVICE}{unit}")),
        }
    }
}

impl fmt::Display for Sender {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tom => formatter.write_str(TOM),
            Self::Session(name) => write!(formatter, "{SESSION}{name}"),
            Self::Service(unit) => write!(formatter, "{SERVICE}{unit}"),
        }
    }
}

impl FromStr for Sender {
    type Err = NorthError;

    fn from_str(text: &str) -> NorthResult<Self> {
        if text == TOM {
            return Ok(Self::Tom);
        }
        if let Some(name) = text.strip_prefix(SESSION).filter(|name| !name.is_empty()) {
            return Ok(Self::Session(name.to_owned()));
        }
        if text.starts_with(SERVICE) {
            return Self::service(text);
        }
        Err(NorthError::Usage(format!("not a sender: {text:?}; use tom, session:NAME or service:UNIT")))
    }
}

impl Sender {
    pub fn unknown() -> Self {
        Self::Session("unknown".into())
    }
}

fn stored_before_w1(text: &str) -> Option<Sender> {
    (!text.is_empty() && !text.contains(':') && !text.chars().any(char::is_whitespace)).then(|| Sender::Session(text.to_owned()))
}

impl Serialize for Sender {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Sender {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse().or_else(|error: NorthError| stored_before_w1(&text).ok_or_else(|| serde::de::Error::custom(error.user_message())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sender_round_trips_through_its_wire_text_and_only_tom_is_unprefixed() {
        for (sender, wire, prefix) in [
            (Sender::Tom, "tom", None),
            (Sender::Session("lead".into()), "session:lead", Some("lead")),
            (Sender::Service("north-ops-check.service".into()), "service:north-ops-check.service", Some("service:north-ops-check.service")),
        ] {
            assert_eq!(sender.to_string(), wire);
            assert_eq!(wire.parse::<Sender>().unwrap(), sender);
            assert_eq!(serde_json::to_string(&sender).unwrap(), format!("{wire:?}"));
            assert_eq!(sender.prefix().as_deref(), prefix);
        }
        assert!("lead".parse::<Sender>().is_err(), "a bare name is no sender");
        assert!("service:".parse::<Sender>().is_err());
        assert!(serde_json::from_str::<Sender>("null").is_err(), "a row without a sender is refused");
    }

    #[test]
    fn a_command_line_sends_as_a_service_or_its_session_and_never_as_tom_or_nobody() {
        assert_eq!(Sender::declared(Some("service:tick"), None).unwrap(), Sender::Service("tick".into()));
        assert_eq!(Sender::declared(Some("service:tick"), Some("lead".into())).unwrap(), Sender::Service("tick".into()));
        assert_eq!(Sender::declared(None, Some("lead".into())).unwrap(), Sender::Session("lead".into()));
        assert!(Sender::declared(None, None).unwrap_err().user_message().contains("--from service:UNIT"));
        assert!(Sender::declared(Some("tom"), None).is_err(), "a timer cannot pose as Tom");
    }

    #[test]
    fn a_sender_seen_from_another_host_keeps_tom_and_names_the_rest_by_host() {
        assert_eq!(Sender::Tom.on_host("far"), Sender::Tom);
        assert_eq!(Sender::Session("lead".into()).on_host("far").prefix().as_deref(), Some("lead@far"));
        assert_eq!(Sender::Service("tick".into()).on_host("far").prefix().as_deref(), Some("service:tick@far"));
    }
}
