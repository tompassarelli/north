mod agent_catalog;
mod platform;
mod cli;
mod dep_gate;
mod autoland_status;
mod provenance;
mod follows_release;
mod issue_box_lint;
mod grounding;
mod claude_stream;
mod language_gate;
mod health;
mod contract_probe;
mod north_state;
mod menu;
mod router;
mod state_core;
mod codex;
mod codex_socket;
mod command_surface;
mod composer;
mod error;
mod foundation;
mod event_store;
mod rpc;
mod prompts;
mod references;
mod usage;
mod usage_stream;
mod local_store;
mod lead_journal;
mod work_items;
mod session_host;
mod hook;
mod guard;
mod pyjson;
mod shell;
#[cfg_attr(not(test), allow(dead_code))]
mod watch;
#[cfg_attr(not(test), allow(dead_code))]
mod stopgap;
mod daemon;
#[cfg_attr(not(test), allow(dead_code))]
mod event_delivery;
mod threads_client;
mod fleet;
mod providers;
mod tui;
#[cfg(test)]
mod test_inputs;

fn main() -> std::process::ExitCode {
    cli::main()
}
