//! Where dispatch may staff (north#1162): `dispatch_providers` and `dispatch_hosts_excluded` in the orchestration
//! file it reads each round. They choose places, not amounts, and read no account usage.
use toml_edit::{DocumentMut, Item};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scope {
    providers: Option<Vec<String>>,
    hosts_excluded: Vec<String>,
}

fn strings(item: Option<&Item>) -> Option<Vec<String>> {
    item.and_then(Item::as_array).map(|values| values.iter().filter_map(|value| value.as_str().map(str::to_owned)).collect())
}

impl Scope {
    pub fn parse(cfg: &DocumentMut) -> Self {
        Self { providers: strings(cfg.get("dispatch_providers")), hosts_excluded: strings(cfg.get("dispatch_hosts_excluded")).unwrap_or_default() }
    }

    pub fn allows_provider(&self, provider: &str) -> bool {
        self.providers.as_ref().is_none_or(|listed| listed.iter().any(|name| name == provider))
    }

    pub fn excludes_host(&self, host: &str) -> bool {
        self.hosts_excluded.iter().any(|name| name == host)
    }

    pub fn why_not_provider(&self, provider: &str) -> String {
        format!("its routing draws {provider}, outside dispatch_providers = [{}]", self.providers.as_deref().unwrap_or_default().join(", "))
    }

    pub fn why_not_host(&self, host: &str) -> String {
        format!("{host} is in dispatch_hosts_excluded, so dispatch places nothing there")
    }

    pub fn lines(&self) -> [String; 2] {
        let providers = self.providers.as_ref().map_or_else(|| "all".to_owned(), |listed| listed.join(", "));
        let hosts = if self.hosts_excluded.is_empty() { "none".to_owned() } else { self.hosts_excluded.join(", ") };
        [format!("dispatch providers: {providers}"), format!("dispatch hosts excluded: {hosts}")]
    }
}

pub fn provider_of(account: &str) -> &str {
    if account.starts_with("claude login") { "claude" } else { account.split_once(':').map_or(account, |(provider, _)| provider) }
}
