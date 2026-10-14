use std::path::Path;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use meada::engine::Engine;

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Build the index of the repository in the current directory.
    Index,
    /// Print the index as sorted JSON.
    Export,
    /// Serve the repository in the current directory to agents over MCP (stdio).
    Mcp,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Mcp => meada::mcp::serve(),
        command => Engine::open(Path::new(".")).and_then(|engine| match command {
            Command::Export => engine.export().map(|json| print!("{json}")),
            _ => engine.refresh(),
        }),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("meada: {e}");
            ExitCode::FAILURE
        }
    }
}
