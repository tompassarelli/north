/// The variable naming a spare worker's binding file, whose session name, actor and capacity need the worker-sandbox prefix applies once a launch takes the spare.
pub const BINDING_ENV: &str = "NORTH_FLEET_BINDING";
pub const SESSION_ENV: &str = "NORTH_FLEET_SESSION";
pub const ACTOR_ENV: &str = "THREADS_ACTOR";
pub const CAPACITY_NEED_ENV: &str = "AGENT_CAPACITY_NEED";
pub const SPOOL_DIR: &str = "spool";
pub const SESSIONS_DIR: &str = "sessions";

/// Where this process stands as a spare worker: started as no spare, a spare no launch has taken yet, or one a launch has taken and bound.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Binding {
    None,
    Awaited,
    Written,
}

/// This process's [`Binding`], read from [`BINDING_ENV`] and the file it names.
pub fn binding() -> Binding {
    match std::env::var_os(BINDING_ENV).filter(|file| !file.is_empty()) {
        None => Binding::None,
        Some(file) if std::path::Path::new(&file).exists() => Binding::Written,
        Some(_) => Binding::Awaited,
    }
}
