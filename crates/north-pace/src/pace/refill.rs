use super::window::Window;

pub struct Fleet<'a> {
    pub accounts: Vec<(&'a str, &'a [Window])>,
    pub protected: Vec<&'a str>,
}

impl Fleet<'_> {
    pub fn refill_hours(&self, account: &str) -> Option<f64> {
        if self.protected.contains(&account) {
            return None;
        }
        let headroom = week(self.windows(account)?).map(|w| (w.quota_ceiling.unwrap_or(100.0) - w.used_pct).max(0.0))?;
        self.accounts
            .iter()
            .filter(|(other, _)| *other != account)
            .filter_map(|(_, windows)| week(windows))
            .filter(|w| w.used_pct >= headroom && w.reset_h > 0.0)
            .map(|w| w.reset_h)
            .min_by(f64::total_cmp)
    }

    fn windows(&self, account: &str) -> Option<&[Window]> {
        self.accounts.iter().find(|(name, _)| *name == account).map(|(_, windows)| *windows)
    }
}

fn week(windows: &[Window]) -> Option<&Window> {
    windows.iter().find(|w| w.is_week())
}
