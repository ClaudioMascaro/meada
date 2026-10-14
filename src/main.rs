use std::path::Path;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use meada::engine::{self, Engine};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Build the indexes of the workspace of the repository in the current directory.
    Index,
    /// Show the workspace: each repository, its index state and any peer not found.
    Status,
    /// Print the index as sorted JSON, with the links that touch it.
    Export,
    /// Serve the repository in the current directory to agents over MCP (stdio).
    Mcp,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let here = Path::new(".");
    let result = match cli.command {
        Command::Mcp => meada::mcp::serve(),
        Command::Status => engine::status(here).map(|status| print!("{status}")),
        command => Engine::open(here).and_then(|engine| match command {
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
