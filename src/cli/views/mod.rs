use crate::foundation::clock::Timestamp;

pub mod accounts;
pub mod health;
pub mod infra;
pub mod report;
pub mod work;

pub struct Snapshot {
    pub at: Timestamp,
    pub work: Option<work::Work>,
    pub accounts: Option<accounts::Accounts>,
    pub infra: Option<infra::Infra>,
    pub health: Option<health::Health>,
}

impl Snapshot {
    pub fn new() -> Self {
        Self {
            at: Timestamp::now(),
            work: None,
            accounts: None,
            infra: None,
            health: None,
        }
    }

    pub fn work(mut self, window: &str) -> Result<Self, String> {
        self.work = Some(work::read(window, self.at)?);
        Ok(self)
    }

    pub fn accounts(mut self, detail: bool) -> Result<Self, String> {
        self.accounts = Some(accounts::read(self.at, detail)?);
        Ok(self)
    }

    pub fn infra(mut self) -> Result<Self, String> {
        self.infra = Some(infra::read()?);
        Ok(self)
    }

    pub fn health(mut self, dry_run: bool) -> Result<Self, String> {
        self.health = Some(health::read(dry_run)?);
        Ok(self)
    }
}

pub fn duration_ms(text: &str) -> Result<u64, String> {
    crate::cli::gate::duration_ms(text).ok_or_else(|| format!("{text} is not a duration such as 30m, 24h or 7d"))
}
