//! What the fact graph holds around the issues and files a message names, as lines for the prompt
//! hook and for a brief. It only reads a graph that `north graph ingest` already built, so a slow or
//! missing graph costs the reader nothing but the lines.
mod file;
mod issue;
mod reach;
mod seed;
mod show;

#[cfg(test)]
mod ground_tests;

pub use reach::{Reach, ground, ground_with};
pub use seed::{Seed, seeds};
