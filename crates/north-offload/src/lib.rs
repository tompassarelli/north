//! Running a land check, a session's build and the compiler cache on another host: which host has room, the slot a run
//! holds, the script that ships and starts it, and the disk the cache keeps.
#![forbid(unsafe_code)]

pub mod build_offload;
pub mod land_offload;
pub mod offload_cache;
