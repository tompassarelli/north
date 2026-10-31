use serde::de::{IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};
use std::fmt;

struct Members<T>(Vec<(String, T)>);
impl<T> Default for Members<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Members<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Entries<T>(std::marker::PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for Entries<T> {
            type Value = Members<T>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an account object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut entries = Vec::new();
                while let Some(entry) = map.next_entry()? {
                    entries.push(entry);
                }
                Ok(Members(entries))
            }
        }
        deserializer.deserialize_map(Entries(std::marker::PhantomData))
    }
}
#[derive(Deserialize)]
struct Provider {
    #[serde(default)]
    controls: Members<IgnoredAny>,
}
pub(super) fn accounts(gate: &str) -> Result<Value, String> {
    let providers: Members<Provider> = serde_json::from_str(gate).map_err(|e| e.to_string())?;
    let mut order = json!({});
    for (name, provider) in providers.0 {
        order[name] = json!(
            provider
                .controls
                .0
                .into_iter()
                .map(|(name, _)| name)
                .collect::<Vec<_>>()
        );
    }
    Ok(order)
}
