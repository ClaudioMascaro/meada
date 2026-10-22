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
        #[cfg(feature = "snapshot")]
        #[arg(long, requires = "workspace")]
        generations: bool,
    },
    Export {
        #[arg(long, requires = "repo")]
        workspace: Option<PathBuf>,
        #[arg(long, requires = "workspace")]
        repo: Option<String>,
        #[cfg(feature = "snapshot")]
        #[arg(long, requires = "workspace")]
        generations: bool,
    },
    Mcp {
        #[arg(long, requires = "workspace")]
        listen: Option<String>,
        #[arg(long, requires = "listen")]
        workspace: Option<PathBuf>,
        #[cfg(feature = "snapshot")]
        #[arg(long, requires = "listen")]
        generations: bool,
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
    #[cfg(feature = "snapshot")]
    Snapshot {
        #[command(subcommand)]
        command: SnapshotCommand,
    },
}

/// `meada snapshot` (ADR 0012, feature `snapshot`).
#[cfg(feature = "snapshot")]
#[derive(Subcommand)]
enum SnapshotCommand {
    Publish {
        #[arg(long)]
        workspace: PathBuf,
    },
    Pull {
        #[arg(long)]
        workspace: PathBuf,
        #[arg(long, value_name = "SECONDS", value_parser = clap::value_parser!(u64).range(1..))]
        every: Option<u64>,
    },
    #[cfg(feature = "refresh-lock")]
    Lock {
        #[arg(long)]
        workspace: PathBuf,
        #[arg(last = true, required = true, value_name = "COMMAND")]
        command: Vec<std::ffi::OsString>,
    },
}

/// `meada snapshot pull`: the generation id on stdout when the pointer file moved, or once
/// when it already named the store's current one; a pull already running is no failure. With
/// `every`, again and again: a failed round is reported and the next one runs.
#[cfg(feature = "snapshot")]
fn pull(manifest: &Path, every: Option<u64>) -> ! {
    use meada::snapshot::pull::Pulled;
    loop {
        let pulled = meada::snapshot::pull::pull(manifest).map(|(pulled, warnings)| {
            for line in warnings.iter().flat_map(|w| w.lines()) {
                eprintln!("meada: {line}");
            }
            pulled
        });
        match pulled {
            Ok(Pulled::Swapped(id)) => println!("{id}"),
            Ok(Pulled::Current(id)) if every.is_none() => println!("{id}"),
            Ok(Pulled::Current(_)) => {}
            Ok(Pulled::Running(dir)) => {
                eprintln!(
                    "meada: {}",
                    Msg::SnapshotPulling(&dir.display().to_string()).text()
                )
            }
            Err(e) => {
                for line in e.to_string().lines() {
                    eprintln!("meada: {line}");
                }
                if every.is_none() {
                    std::process::exit(1);
                }
            }
        }
        let Some(seconds) = every else {
            std::process::exit(0);
        };
        std::thread::sleep(std::time::Duration::from_secs(seconds));
    }
}

/// `meada mcp --listen`. In debug builds `MEADA_DRAIN_MS` shortens the drain bound (a knob for
/// tests and not documented; a release binary has no way to shorten the bound).
fn listen_at(listen: &str, manifest: &Path) -> meada::Result<()> {
    #[cfg(debug_assertions)]
    if let Some(bound) = std::env::var("MEADA_DRAIN_MS")
        .ok()
        .and_then(|ms| ms.parse().ok())
        .map(std::time::Duration::from_millis)
    {
        return meada::mcp::with_drain_bound(bound, || meada::mcp::listen(listen, manifest));
    }
    meada::mcp::listen(listen, manifest)
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
        });
    #[cfg(feature = "snapshot")]
    let matches = matches.mut_subcommand("snapshot", |c| {
        (c.about(Msg::Snapshot.text()))
            .mut_subcommand("publish", |c| {
                (c.about(Msg::SnapshotPublish.text()))
                    .mut_arg("workspace", |a| a.help(Msg::WorkspaceHelp.text()))
            })
            .mut_subcommand("pull", |c| {
                (c.about(Msg::SnapshotPull.text()))
                    .mut_arg("workspace", |a| a.help(Msg::WorkspaceHelp.text()))
                    .mut_arg("every", |a| a.help(Msg::SnapshotEvery.text()))
            })
    });
    #[cfg(feature = "refresh-lock")]
    let matches = matches.mut_subcommand("snapshot", |c| {
        c.mut_subcommand("lock", |c| {
            (c.about(Msg::SnapshotLock.text()))
                .mut_arg("workspace", |a| a.help(Msg::WorkspaceHelp.text()))
                .mut_arg("command", |a| a.help(Msg::SnapshotLockCommand.text()))
        })
    });
    #[cfg(feature = "snapshot")]
    let matches = ["status", "export", "mcp"]
        .into_iter()
        .fold(matches, |matches, name| {
            matches.mut_subcommand(name, |c| {
                c.mut_arg("generations", |a| a.help(Msg::GenerationsHelp.text()))
            })
        });
    let matches = matches.get_matches();
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
    let here = Path::new(".");
    let mut unready = false;
    let result = match cli.command {
        #[cfg(feature = "snapshot")]
        Command::Mcp {
            listen: Some(listen),
            workspace: Some(manifest),
            generations: true,
        } => {
            msg::init(None);
            meada::mcp::listen_generations(&listen, &manifest)
        }
        #[cfg(feature = "snapshot")]
        Command::Status {
            workspace: Some(manifest),
            generations: true,
        } => meada::with_busy_timeout(engine::PROBE_BUSY, || {
            engine::status_generations(&manifest).map(|status| {
                print!("{status}");
                if let Err(problems) = engine::served_generation(&manifest) {
                    for line in problems.to_string().lines() {
                        eprintln!("meada: {line}");
                    }
                    unready = true;
                }
            })
        }),
        #[cfg(feature = "snapshot")]
        Command::Export {
            workspace: Some(manifest),
            repo: Some(repo),
            generations: true,
        } => Engine::open_node(&manifest, &repo)
            .and_then(|engine| engine.export())
            .map(|json| print!("{json}")),
        Command::Mcp {
            listen, workspace, ..
        } => {
            // The MCP surface is English whatever `LANG` says (ADR 0008).
            msg::init(None);
            match (listen, workspace) {
                (Some(listen), Some(manifest)) => listen_at(&listen, &manifest),
                _ => meada::mcp::serve(),
            }
        }
        Command::Status {
            workspace: Some(manifest),
            ..
        } => meada::with_busy_timeout(engine::PROBE_BUSY, || {
            engine::status_workspace(&manifest).map(|status| {
                print!("{status}");
                unready = !engine::ready(&manifest);
            })
        }),
        Command::Status {
            workspace: None, ..
        } => engine::status(here).map(|status| print!("{status}")),
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
        // The generation id is the only line on stdout.
        #[cfg(feature = "snapshot")]
        Command::Snapshot {
            command: SnapshotCommand::Publish { workspace },
        } => match meada::snapshot::publish(&workspace) {
            Ok(generation) => {
                println!("{generation}");
                Ok(())
            }
            // Every repository's problem on its own line, as `index --workspace` reports them.
            Err(e) => {
                for line in e.to_string().lines() {
                    eprintln!("meada: {line}");
                }
                std::process::exit(1);
            }
        },
        #[cfg(feature = "snapshot")]
        Command::Snapshot {
            command: SnapshotCommand::Pull { workspace, every },
        } => pull(&workspace, every),
        // The child's exit code, or 1 with the reason: the lock held elsewhere or lost.
        #[cfg(feature = "refresh-lock")]
        Command::Snapshot {
            command: SnapshotCommand::Lock { workspace, command },
        } => match meada::snapshot::lock::run(&workspace, &command) {
            Ok(code) => std::process::exit(code),
            Err(e) => {
                for line in e.to_string().lines() {
                    eprintln!("meada: {line}");
                }
                std::process::exit(1);
            }
        },
        Command::Export {
            workspace: Some(manifest),
            repo: Some(repo),
            ..
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
