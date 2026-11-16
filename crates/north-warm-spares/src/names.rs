/// The variable naming a spare worker's binding file, whose session name, actor and capacity need the worker-sandbox prefix applies once a launch takes the spare.
pub const BINDING_ENV: &str = "NORTH_FLEET_BINDING";
pub const SESSION_ENV: &str = "NORTH_FLEET_SESSION";
pub const ACTOR_ENV: &str = "THREADS_ACTOR";
pub const CAPACITY_NEED_ENV: &str = "AGENT_CAPACITY_NEED";
pub const SPOOL_DIR: &str = "spool";
pub const SESSIONS_DIR: &str = "sessions";
