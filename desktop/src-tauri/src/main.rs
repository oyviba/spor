// No console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Started again by itself as the agent host for a run (see spor::host).
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some(spor::host::ARG) {
        std::process::exit(spor::host::main(&args[2..]));
    }
    // Or as the MCP server a run's agent talks to (see spor::mcp).
    if args.get(1).map(String::as_str) == Some(spor::mcp::ARG) {
        std::process::exit(spor::mcp::main(&args[2..]));
    }
    // Started by git or ssh to ask for a passphrase or password: hand the
    // prompt to the running app and print its answer (see spor::askpass).
    spor::askpass::run_helper_if_invoked();
    spor_desktop_lib::run()
}
