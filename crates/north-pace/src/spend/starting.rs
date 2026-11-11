use super::Starter;

impl Starter {
    /// Who a launch starts as: the reserved callers are never urgent workers, and an urgent one counts the urgent sessions already running on its account.
    pub fn launched(reserved: bool, urgent: bool, running_urgent: impl FnOnce() -> usize) -> Self {
        match (reserved, urgent) {
            (true, _) => Self::Reserved,
            (false, true) => Self::Urgent { running: running_urgent() },
            (false, false) => Self::Worker,
        }
    }

    /// Whether the burn governor holds this starter: a worker always, an urgent one once `limit` urgent sessions run on its account.
    pub fn governed(self, limit: usize) -> bool {
        !matches!(self, Self::Urgent { running } if running < limit)
    }
}
