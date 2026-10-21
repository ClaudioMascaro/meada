use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use meada::decide;
use meada::engine::{self, Engine};
use meada::msg::{self, Msg};

#[derive(Parser)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

// Command descriptions are set at runtime from the message catalog (ADR 0008).
#[derive(Subcommand)]
enum Command {
    Index {
        #[arg(long)]
        workspace: Option<PathBuf>,
    },
    Status {
        #[arg(long)]
        workspace: Option<PathBuf>,
    },
    Export {
        #[arg(long, requires = "repo")]
        workspace: Option<PathBuf>,
        #[arg(long, requires = "workspace")]
        repo: Option<String>,
    },
    Mcp {
        #[arg(long, requires = "workspace")]
        listen: Option<String>,
        #[arg(long, requires = "listen")]
        workspace: Option<PathBuf>,
    },
    Decide {
        #[arg(long)]
        item: Option<String>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        list: bool,
        #[arg(long, conflicts_with_all = ["item", "dry_run", "list"])]
        prune: bool,
        #[arg(long)]
        decider: bool,
        #[arg(long, requires = "decider")]
        show_request: bool,
        #[arg(long, requires = "decider", allow_negative_numbers = true)]
        min_p: Option<f64>,
    },
}

fn main() -> ExitCode {
    msg::init(std::env::var("LANG").ok().as_deref());
    let matches = Cli::command()
        .about(Msg::About.text())
        .mut_subcommand("index", |c| {
            (c.about(Msg::Index.text())).mut_arg("workspace", |a| a.help(Msg::WorkspaceHelp.text()))
        })
        .mut_subcommand("status", |c| {
            (c.about(Msg::Status.text()))
                .mut_arg("workspace", |a| a.help(Msg::WorkspaceHelp.text()))
        })
        .mut_subcommand("export", |c| {
            (c.about(Msg::Export.text()))
                .mut_arg("workspace", |a| a.help(Msg::WorkspaceHelp.text()))
                .mut_arg("repo", |a| a.help(Msg::RepoHelp.text()))
        })
        .mut_subcommand("mcp", |c| {
            (c.about(Msg::Mcp.text()))
                .mut_arg("listen", |a| a.help(Msg::ListenHelp.text()))
                .mut_arg("workspace", |a| a.help(Msg::WorkspaceHelp.text()))
        })
        .mut_subcommand("decide", |c| {
            c.about(Msg::Decide.text())
                .mut_arg("item", |a| a.help(Msg::DecideItemHelp.text()))
                .mut_arg("dry_run", |a| a.help(Msg::DecideDryRunHelp.text()))
                .mut_arg("list", |a| a.help(Msg::DecideListHelp.text()))
                .mut_arg("prune", |a| a.help(Msg::DecidePruneHelp.text()))
                .mut_arg("decider", |a| a.help(Msg::DecideDeciderHelp.text()))
                .mut_arg("show_request", |a| {
                    a.help(Msg::DecideShowRequestHelp.text())
                })
                .mut_arg("min_p", |a| a.help(Msg::DecideMinPHelp.text()))
        })
        .get_matches();
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
    let here = Path::new(".");
    let mut unready = false;
    let result = match cli.command {
        Command::Mcp { listen, workspace } => {
            // The MCP surface is English whatever `LANG` says (ADR 0008).
            msg::init(None);
            match (listen, workspace) {
                (Some(listen), Some(manifest)) => meada::mcp::listen(&listen, &manifest),
                _ => meada::mcp::serve(),
            }
        }
        Command::Status {
            workspace: Some(manifest),
        } => meada::with_busy_timeout(engine::PROBE_BUSY, || {
            engine::status_workspace(&manifest).map(|status| {
                print!("{status}");
                unready = !engine::ready(&manifest);
            })
        }),
        Command::Status { workspace: None } => {
            engine::status(here).map(|status| print!("{status}"))
        }
        // Every repository's problem on its own line; the others are refreshed all the same.
        Command::Index {
            workspace: Some(manifest),
        } => engine::index_workspace(&manifest).map(|problems| {
            for problem in &problems {
                eprintln!("meada: {problem}");
            }
            if !problems.is_empty() {
                std::process::exit(1);
            }
        }),
        Command::Export {
            workspace: Some(manifest),
            repo: Some(repo),
        } => Engine::open_manifest(&manifest, &repo)
            .and_then(|engine| engine.export())
            .map(|json| print!("{json}")),
        Command::Decide {
            item,
            dry_run,
            list,
            prune,
            decider,
            show_request,
            min_p,
        } => Engine::open(here).and_then(|engine| {
            let options = decide::Options {
                item,
                dry_run,
                list,
                prune,
                decider,
                show_request,
                min_p,
            };
            decide::run(
                &engine,
                &options,
                &mut std::io::stdin().lock(),
                &mut std::io::stdout(),
            )
        }),
        command => Engine::open(here).and_then(|engine| match command {
            Command::Export { .. } => engine.export().map(|json| print!("{json}")),
            _ => engine.refresh(),
        }),
    };
    match result {
        // A readiness probe (ADR 0011); returning, not exiting, flushes the status text.
        Ok(()) if unready => ExitCode::FAILURE,
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("meada: {e}");
            ExitCode::FAILURE
        }
    }
}
