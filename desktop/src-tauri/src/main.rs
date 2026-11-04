// No console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Started again by itself as the agent host for a run (see spor::host).
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some(spor::host::ARG) {
        std::process::exit(spor::host::main(&args[2..]));
    }
    spor_desktop_lib::run()
}
