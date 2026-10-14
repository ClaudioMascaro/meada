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
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = Engine::open(Path::new(".")).and_then(|engine| match cli.command {
        Command::Index => Ok(()),
        Command::Export => engine.export().map(|json| print!("{json}")),
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("meada: {e}");
            ExitCode::FAILURE
        }
    }
}
