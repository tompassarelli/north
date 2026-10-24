mod agent_catalog;
mod capacity;
mod platform;
mod cli;
mod dep_gate;
mod review_gate;
mod landing;
mod autoland_status;
mod provenance;
mod follows_release;
mod pace;
mod issue_box_lint;
mod grounding;
mod claude_stream;
mod language_gate;
mod health;
mod contract_probe;
mod router;
mod codex;
mod error;
mod foundation;
mod rpc;
mod usage_stream;
mod lead_journal;
mod work_items;
mod session_host;
mod ops;
mod hook;
mod guard;
mod pyjson;
mod shell;
mod daemon;
mod threads_client;
mod fleet;
mod ledger;
mod providers;
mod tui;
#[cfg(test)]
mod test_inputs;

fn main() -> std::process::ExitCode {
    platform::stdio::with_blocking_outputs(|| {
        let arguments = std::env::args().skip(1).collect::<Vec<_>>();
        arguments
            .first()
            .filter(|argument| *argument == "private-desktop")
            .map_or_else(cli::main, |_| ops::private_desktop::run(&arguments[1..]))
    })
}
