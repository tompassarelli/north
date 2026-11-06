//! What the fact graph holds around the issues and files a message names, as lines for the prompt
//! hook and for a brief. It only reads a graph that `north graph ingest` already built, so a slow or
//! missing graph costs the reader nothing but the lines.
mod backlog;
mod file;
mod issue;
mod ready;
mod reach;
mod rules;
mod seed;
mod show;

#[cfg(test)]
mod ground_tests;
#[cfg(test)]
mod backlog_tests;

pub use backlog::{Backlog, Entry, Held, Verdict, read as read_backlog_from, read_default as read_backlog};
pub use rules::{held_query, ready_query};
pub use ready::command as ready_command;
pub use reach::{Reach, ground, ground_with};
pub use seed::{Seed, seeds};
