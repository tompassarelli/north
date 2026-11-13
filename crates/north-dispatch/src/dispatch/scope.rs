//! Where dispatch may staff (north#1162): `dispatch_providers`, `dispatch_hosts_excluded` and
//! `dispatch_accounts_excluded` in the orchestration file it reads each round. They choose places, not amounts, and read no account usage.
use toml_edit::{DocumentMut, Item};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scope {
    providers: Option<Vec<String>>,
    hosts_excluded: Vec<String>,
    accounts_excluded: Vec<String>,
}

fn strings(item: Option<&Item>) -> Option<Vec<String>> {
    item.and_then(Item::as_array).map(|values| values.iter().filter_map(|value| value.as_str().map(str::to_owned)).collect())
}

impl Scope {
    pub fn admitting(mut self, provider: &str) -> Self {
        if let Some(listed) = self.providers.as_mut().filter(|listed| !listed.iter().any(|name| name == provider)) {
            listed.push(provider.to_owned());
        }
        self
    }

    pub fn parse(cfg: &DocumentMut) -> Self {
        Self {
            providers: strings(cfg.get("dispatch_providers")),
            hosts_excluded: strings(cfg.get("dispatch_hosts_excluded")).unwrap_or_default(),
            accounts_excluded: strings(cfg.get("dispatch_accounts_excluded")).unwrap_or_default(),
        }
    }

    pub fn allows_provider(&self, provider: &str) -> bool {
        self.providers.as_ref().is_none_or(|listed| listed.iter().any(|name| name == provider))
    }

    pub fn excludes_host(&self, host: &str) -> bool {
        self.hosts_excluded.iter().any(|name| name == host)
    }

    pub fn excludes_account(&self, account: &str) -> bool {
        self.accounts_excluded.iter().any(|name| name == account)
    }

    pub fn why_not_provider(&self, provider: &str) -> String {
        format!("its routing draws {provider}, outside dispatch_providers = [{}]", self.providers.as_deref().unwrap_or_default().join(", "))
    }

    pub fn why_not_host(&self, host: &str) -> String {
        format!("{host} is in dispatch_hosts_excluded, so dispatch places nothing there")
    }

    pub fn why_not_account(&self, account: &str) -> String {
        format!("{account} is in dispatch_accounts_excluded, so dispatch draws nothing from it")
    }

    pub fn lines(&self) -> [String; 3] {
        let providers = self.providers.as_ref().map_or_else(|| "all".to_owned(), |listed| listed.join(", "));
        let listed = |names: &[String]| if names.is_empty() { "none".to_owned() } else { names.join(", ") };
        [format!("dispatch providers: {providers}"), format!("dispatch hosts excluded: {}", listed(&self.hosts_excluded)), format!("dispatch accounts excluded: {}", listed(&self.accounts_excluded))]
    }
}

pub fn provider_of(account: &str) -> &str {
    if account.starts_with("claude login") { "claude" } else { account.split_once(':').map_or(account, |(provider, _)| provider) }
}
