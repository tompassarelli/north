#![cfg(any(test, feature = "test-support"))]

pub const LEDGER_SCHEMA: &str = include_str!("runs/fixtures/ledger.sql");
