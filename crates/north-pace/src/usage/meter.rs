//! The name each usage row gives a window, and the model families a scoped window may name.
//!
//! The provider writes a model-scoped weekly window as `weekly_scoped/<DisplayName>`: the same row the
//! Opus meter reads (`weekly_scoped/Opus`) carries Fable (`weekly_scoped/Fable`). A family word inside a
//! longer display name (`Fable 5.2`) still names its family, so a provider rename cannot drop a meter.
use super::decide::world::{ModelFamily, Window};

/// The prefix of a model-scoped weekly window row.
pub const SCOPED_PREFIX: &str = "weekly_scoped/";

/// Every family a scoped window may name, in the order a display name is searched.
const FAMILIES: [(&str, ModelFamily); 4] = [("fable", ModelFamily::Fable), ("opus", ModelFamily::Opus), ("sonnet", ModelFamily::Sonnet), ("haiku", ModelFamily::Haiku)];

/// The family a provider's scoped window name names, from its display name or its canonical row name.
pub fn family(name: &str) -> Option<ModelFamily> {
    let name = name.to_ascii_lowercase();
    FAMILIES.into_iter().find(|(word, _)| name.contains(word)).map(|(_, family)| family)
}

/// The provider's canonical name for a family's scoped weekly window.
pub fn scoped(family: ModelFamily) -> String {
    format!("{SCOPED_PREFIX}{}", label(family))
}

/// A family's name as the provider spells it in a scoped window.
pub fn label(family: ModelFamily) -> &'static str {
    match family {
        ModelFamily::Fable => "Fable",
        ModelFamily::Opus => "Opus",
        ModelFamily::Sonnet => "Sonnet",
        ModelFamily::Haiku => "Haiku",
    }
}

/// The window a usage row names, if it names one this model reads.
pub fn window(name: &str) -> Option<Window> {
    match name {
        "session" => Some(Window::FiveHour),
        "weekly_all" => Some(Window::Week),
        scoped => scoped.strip_prefix(SCOPED_PREFIX).and_then(family).map(Window::Scoped),
    }
}
