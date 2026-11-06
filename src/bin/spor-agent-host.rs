//! The agent host on its own (see `spor::host`): what the app's executable
//! does when it's started again as a run's host. The core's tests use it;
//! the app doesn't need it, since it hosts runs itself.

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some(spor::host::ARG) => std::process::exit(spor::host::main(&args[2..])),
        Some(spor::mcp::ARG) => std::process::exit(spor::mcp::main(&args[2..])),
        _ => {
            eprintln!("spor-agent-host is started by spor to host an agent run");
            std::process::exit(2);
        }
    }
}
