pub const LEGACY_COORDINATOR_ROLE: &str = "proxy";

pub fn is_coordinator_role(role: &str) -> bool {
    matches!(role, "coordinator" | LEGACY_COORDINATOR_ROLE)
}
