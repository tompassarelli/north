use time::{Date, Month, OffsetDateTime};

pub const GO_PROVIDER: &str = "opencode-go";
pub const FREE_PROVIDER: &str = "opencode";
pub const GO_MODEL: &str = "opencode-go/deepseek-v4.1-flash";
pub const FREE_MODEL: &str = "opencode/deepseek-v4.1-flash-free";
pub const MODELS: &[&str] = &[GO_MODEL, FREE_MODEL];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Visibility {
    Public,
    Private,
    Unknown,
}

impl Visibility {
    pub fn from_github(text: &str) -> Self {
        match text.trim().to_ascii_uppercase().as_str() {
            "PUBLIC" => Self::Public,
            "PRIVATE" | "INTERNAL" => Self::Private,
            _ => Self::Unknown,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Route {
    Go,
    Free,
}

impl Route {
    pub fn of(visibility: Visibility) -> Self {
        match visibility {
            Visibility::Public | Visibility::Private | Visibility::Unknown => Self::Go,
        }
    }

    pub fn parse(label: &str) -> Option<Self> {
        [Self::Go, Self::Free].into_iter().find(|route| route.label() == label)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Go => "go",
            Self::Free => "free",
        }
    }

    pub fn model(self) -> &'static str {
        match self {
            Self::Go => GO_MODEL,
            Self::Free => FREE_MODEL,
        }
    }

    pub fn provider(self) -> &'static str {
        match self {
            Self::Go => GO_PROVIDER,
            Self::Free => FREE_PROVIDER,
        }
    }

    pub fn fallback(self) -> Option<Self> {
        (self == Self::Free).then_some(Self::Go)
    }
}

/// A private lane may use Go only while its zero-retention agreement runs; a lane on the free model holds no private data, so the date does not bind it.
pub fn admits(route: Route, today: Date, retention_until: Date) -> Result<(), String> {
    if route == Route::Go && today > retention_until {
        return Err(format!("Go's zero-retention agreement for DeepSeek ended on {retention_until}; update harness/opencode.toml from https://opencode.ai/docs/go before routing private lanes to it"));
    }
    Ok(())
}

pub fn date(text: &str) -> Result<Date, String> {
    let bad = || format!("{text:?} is not a YYYY-MM-DD date");
    let mut parts = text.trim().splitn(3, '-');
    let mut number = || parts.next().and_then(|part| part.parse::<u32>().ok()).ok_or_else(bad);
    let (year, month, day) = (number()?, number()?, number()?);
    let month = u8::try_from(month).ok().and_then(|month| Month::try_from(month).ok()).ok_or_else(bad)?;
    Date::from_calendar_date(i32::try_from(year).map_err(|_| bad())?, month, u8::try_from(day).map_err(|_| bad())?).map_err(|_| bad())
}

pub fn today() -> Date {
    OffsetDateTime::now_utc().date()
}
