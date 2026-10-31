use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Provider {
    Claude,
    Codex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum WindowKind {
    #[serde(rename = "5h")]
    FiveHour,
    #[serde(rename = "week")]
    Week,
    #[serde(rename = "credit")]
    Credit,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Window {
    pub kind: WindowKind,
    pub used_pct: f64,
    pub resets_at: String,
    pub as_of: String,
    pub smoothed_pct_h: Option<f64>,
    pub bound_pct_h: Option<f64>,
    pub per_worker_pct_min: Option<f64>,
    pub workers: Option<u32>,
    pub held_reason: Option<String>,
    pub target_burn_pct_h: Option<f64>,
    pub last_429: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Member {
    pub present: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Billing {
    pub standard: String,
    pub overflow_money: bool,
    pub fresh: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Grant {
    pub id: String,
    pub spawner: String,
    pub expires_at: String,
    pub remaining: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Slot {
    pub id: String,
    pub provider: Provider,
    pub hosts: BTreeMap<String, Member>,
    pub windows: Vec<Window>,
    pub billing: Billing,
    pub auto_route: bool,
    pub grants: Vec<Grant>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reservation {
    pub id: String,
    pub slot: String,
    pub at: f64,
    pub duration_min: f64,
    pub add: BTreeMap<WindowKind, f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ItemKind {
    Worker,
    Lead,
    Plan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Execution {
    Native,
    LaunchLocal,
    QueueLaptop,
    Advisory,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Spawner {
    pub provider: Provider,
    pub account: String,
    pub host: String,
    pub session: String,
    pub task: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub kind: ItemKind,
    pub category: String,
    pub difficulty: f64,
    pub spec: String,
    pub scope: String,
    pub surface: String,
    pub verify: String,
    pub files: Vec<String>,
    pub eta_min: f64,
    pub native: bool,
    pub private: bool,
    pub requested_tier: Option<String>,
    pub actual_model: Option<ActualModel>,
    pub project: String,
    pub urgent: bool,
    pub proxy: bool,
    pub spawner: Spawner,
    pub pin_account: Option<String>,
    pub exec: Vec<Execution>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActualModel {
    pub model: String,
    pub effort: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Facts {
    pub slots: Vec<Slot>,
    pub router_reservations: Vec<Reservation>,
    pub router_draining: BTreeMap<String, bool>,
    pub controls: BTreeMap<String, Controller>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DecisionKind {
    Native,
    Launch,
    Queued,
    Refused,
    Advice,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Decision {
    pub kind: DecisionKind,
    pub slot: Option<String>,
    pub tier: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub host: Option<String>,
    pub decided_by: String,
    pub why: String,
    pub warnings: Vec<String>,
    pub excluded: BTreeMap<String, String>,
    pub eta: Option<String>,
    pub reservation_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Controller {
    pub at: serde_json::Number,
    pub allowed_workers: u32,
    pub held: bool,
    pub held_reason: Option<String>,
    pub workers: Option<u32>,
    pub target_burn_pct_h: Option<f64>,
    pub burn_pct_worker_min: BTreeMap<WindowKind, f64>,
    pub windows: Vec<ControllerWindow>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControllerWindow {
    pub window: WindowKind,
    pub used_pct: f64,
    pub as_of: String,
    pub resets_at: String,
    pub smoothed_pct_h: Option<f64>,
    pub bound_pct_h: Option<f64>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControllerContract {
    pub controls: BTreeMap<String, Controller>,
    pub replay: Value,
}

#[cfg(test)]
mod tests {
    use super::*;
    const CONTRACT: &str = include_str!("fixtures/controller-contract.json");

    #[test]
    fn controller_contract_round_trip_preserves_every_field() {
        let original: Value = serde_json::from_str(CONTRACT).unwrap();
        let contract: ControllerContract = serde_json::from_value(original.clone()).unwrap();
        assert_eq!(serde_json::to_value(contract).unwrap(), original);
    }

    #[test]
    fn controller_accepts_null_optional_readings() {
        let mut raw: Value = serde_json::from_str(CONTRACT).unwrap();
        for control in raw["controls"].as_object_mut().unwrap().values_mut() {
            for key in ["held_reason", "workers", "target_burn_pct_h"] {
                control[key] = Value::Null;
            }
            for window in control["windows"].as_array_mut().unwrap() {
                window["smoothed_pct_h"] = Value::Null;
                window["bound_pct_h"] = Value::Null;
            }
        }
        let contract: ControllerContract = serde_json::from_value(raw).unwrap();
        assert!(contract.controls.values().all(|c| {
            c.workers.is_none()
                && c.windows
                    .iter()
                    .all(|w| w.smoothed_pct_h.is_none() && w.bound_pct_h.is_none())
        }));
    }
}
