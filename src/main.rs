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
    #[cfg(feature = "github-app")]
    GithubApp {
        #[command(subcommand)]
        command: GithubAppCommand,
    },
    #[cfg(feature = "ui")]
    Ui {
        #[arg(long)]
        listen: String,
        #[arg(long)]
        workspace: PathBuf,
    },
    #[cfg(feature = "generation-pointer")]
    Workspace {
        #[command(subcommand)]
        command: WorkspaceCommand,
    },
}

/// `meada workspace` (ADR 0014, feature `generation-pointer`).
#[cfg(feature = "generation-pointer")]
#[derive(Subcommand)]
enum WorkspaceCommand {
    Import {
        file: PathBuf,
        #[arg(long)]
        message: String,
        #[arg(long)]
        author: Option<String>,
        #[arg(long)]
        workspace: PathBuf,
    },
    List {
        #[arg(long)]
        workspace: PathBuf,
    },
    Show {
        version: Option<String>,
        #[arg(long)]
        workspace: PathBuf,
    },
    #[cfg(feature = "refresh-lock")]
    Apply {
        version: String,
        #[arg(long)]
        workspace: PathBuf,
    },
    Pull {
        #[arg(long)]
        workspace: PathBuf,
    },
}

/// `meada workspace`: the answer on stdout (an id, the listing, the entries), notes on stderr;
/// on a failure every line on stderr and exit 1.
#[cfg(feature = "generation-pointer")]
fn workspace(command: WorkspaceCommand) -> ! {
    use meada::snapshot::versions;
    let mut notes = Vec::new();
    let answer = match command {
        WorkspaceCommand::Import {
            file,
            message,
            author,
            workspace,
        } => {
            let author = author.unwrap_or_else(versions::cli_author);
            (versions::import(&workspace, &file, &message, &author, &mut notes)).map(|imported| {
                if let Some(first) = &imported.first {
                    let (author, message) = (
                        versions::shown(&first.author),
                        versions::shown(&first.message),
                    );
                    let stored = Msg::WorkspaceStored {
                        id: &first.id,
                        author: &author,
                        time: &first.time,
                        message: &message,
                    };
                    notes.push(stored.text());
                }
                format!("{}\n", imported.id)
            })
        }
        WorkspaceCommand::List { workspace } => versions::list(&workspace).map(|listing| {
            if let Err(none) = &listing.current {
                notes.push(none.clone());
            }
            listing.text()
        }),
        WorkspaceCommand::Show { version, workspace } => {
            versions::show(&workspace, version.as_deref())
        }
        #[cfg(feature = "refresh-lock")]
        WorkspaceCommand::Apply { version, workspace } => {
            let author = versions::cli_author();
            versions::apply(&workspace, &version, &author).map(|state| format!("{state}\n"))
        }
        // The id on stdout either way; that nothing changed on stderr.
        WorkspaceCommand::Pull { workspace } => versions::pull(&workspace).map(|pulled| {
            let id = match pulled {
                versions::Pulled::Swapped(id) => id,
                versions::Pulled::Current(id) => {
                    notes.push(Msg::WorkspaceAlreadyCurrent(&id).text());
                    id
                }
            };
            format!("{id}\n")
        }),
    };
    for line in notes.iter().flat_map(|n| n.lines()) {
        eprintln!("meada: {line}");
    }
    match answer {
        Ok(text) => {
            print!("{text}");
            std::process::exit(0);
        }
        Err(e) => {
            for line in e.to_string().lines() {
                eprintln!("meada: {line}");
            }
            std::process::exit(1);
        }
    }
}

/// `meada github-app` (ADR 0013, feature `github-app`).
#[cfg(feature = "github-app")]
#[derive(Subcommand)]
enum GithubAppCommand {
    Credential {
        #[arg(long)]
        workspace: PathBuf,
        #[arg(value_parser = ["get", "store", "erase"])]
        action: String,
    },
    Throttled {
        #[arg(long)]
        workspace: PathBuf,
        repository: String,
        #[arg(long, value_name = "SECONDS")]
        retry_after: Option<u64>,
    },
}

/// `meada github-app`: git's answer on stdout, notes on stderr; on a failure every line on
/// stderr and exit 1, so git tries its next helper.
#[cfg(feature = "github-app")]
fn github_app(command: GithubAppCommand) -> ! {
    let reply = match command {
        GithubAppCommand::Credential { workspace, action } => {
            meada::github_app::credential(&workspace, &action, &mut std::io::stdin().lock())
        }
        GithubAppCommand::Throttled {
            workspace,
            repository,
            retry_after,
        } => meada::github_app::throttled(&workspace, &repository, retry_after),
    };
    match reply {
        Ok(reply) => {
            print!("{}", reply.stdout);
            for note in &reply.notes {
                eprintln!("meada: {note}");
            }
            std::process::exit(0);
        }
        Err(e) => {
            for line in e.to_string().lines() {
                eprintln!("meada: {line}");
            }
            std::process::exit(1);
        }
    }
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
    #[cfg(feature = "refresh-lock")]
    Pointer {
        #[command(subcommand)]
        command: PointerCommand,
    },
    #[cfg(feature = "refresh-lock")]
    Fence {
        #[command(subcommand)]
        command: FenceCommand,
    },
}

/// `meada snapshot pointer` (ADR 0014, feature `refresh-lock`).
#[cfg(feature = "refresh-lock")]
#[derive(Subcommand)]
enum PointerCommand {
    Set {
        generation: String,
        #[arg(long)]
        workspace: PathBuf,
    },
    Retain {
        generation: String,
        #[arg(long)]
        workspace: PathBuf,
    },
    Drop {
        generation: String,
        #[arg(long)]
        workspace: PathBuf,
    },
}

/// `meada snapshot fence` (ADR 0014, feature `refresh-lock`).
#[cfg(feature = "refresh-lock")]
#[derive(Subcommand)]
enum FenceCommand {
    Recover {
        #[arg(long, value_name = "FENCE")]
        at_least: Option<String>,
        #[arg(long)]
        workspace: PathBuf,
    },
}

/// `meada snapshot pull`: the generation id on stdout when the pointer file moved, or once
/// when it already named the store's current one; a pull already running is no failure. With
/// `every`, again and again: a failed round is reported and the next one runs.
#[cfg(feature = "snapshot")]
fn pull(manifest: &Path, every: Option<u64>) -> ! {
    use meada::snapshot::pull::Pulled;
    // A host name that is no node name refuses to start; a round never refuses.
    #[cfg(feature = "generation-pointer")]
    if every.is_some()
        && let Err(e) = meada::snapshot::heartbeat::check(manifest)
    {
        for line in e.to_string().lines() {
            eprintln!("meada: {line}");
        }
        std::process::exit(1);
    }
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
        #[cfg(feature = "generation-pointer")]
        if let Some(warning) = meada::snapshot::heartbeat::beat(manifest, seconds) {
            for line in warning.lines() {
                eprintln!("meada: {line}");
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(seconds));
    }
}

/// `serve`, a listener (`meada mcp --listen`, `meada ui`). In debug builds `MEADA_DRAIN_MS`
/// shortens the drain bound (a knob for tests and not documented; a release binary has no way
/// to shorten the bound).
fn drained(serve: impl FnOnce() -> meada::Result<()>) -> meada::Result<()> {
    #[cfg(debug_assertions)]
    if let Some(bound) = std::env::var("MEADA_DRAIN_MS")
        .ok()
        .and_then(|ms| ms.parse().ok())
        .map(std::time::Duration::from_millis)
    {
        return meada::mcp::with_drain_bound(bound, serve);
    }
    serve()
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
        .mut_subcommand("pointer", |c| {
            fn sub(c: clap::Command, about: Msg<'static>, id: Msg<'static>) -> clap::Command {
                (c.about(about.text()))
                    .mut_arg("generation", |a| a.help(id.text()))
                    .mut_arg("workspace", |a| a.help(Msg::WorkspaceHelp.text()))
            }
            let id = || Msg::SnapshotPointerGeneration;
            (c.about(Msg::SnapshotPointer.text()))
                .mut_subcommand("set", |c| sub(c, Msg::SnapshotPointerSet, id()))
                .mut_subcommand("retain", |c| {
                    let id = Msg::SnapshotPointerRetainGeneration;
                    sub(c, Msg::SnapshotPointerRetain, id)
                })
                .mut_subcommand("drop", |c| sub(c, Msg::SnapshotPointerDrop, id()))
        })
        .mut_subcommand("fence", |c| {
            (c.about(Msg::SnapshotFence.text())).mut_subcommand("recover", |c| {
                (c.about(Msg::SnapshotFenceRecover.text()))
                    .mut_arg("at_least", |a| a.help(Msg::SnapshotFenceAtLeast.text()))
                    .mut_arg("workspace", |a| a.help(Msg::WorkspaceHelp.text()))
            })
        })
    });
    #[cfg(feature = "github-app")]
    let matches = matches.mut_subcommand("github-app", |c| {
        (c.about(Msg::GithubApp.text()))
            .mut_subcommand("credential", |c| {
                (c.about(Msg::GithubAppCredential.text()))
                    .mut_arg("workspace", |a| a.help(Msg::WorkspaceHelp.text()))
                    .mut_arg("action", |a| a.help(Msg::GithubAppAction.text()))
            })
            .mut_subcommand("throttled", |c| {
                (c.about(Msg::GithubAppThrottled.text()))
                    .mut_arg("workspace", |a| a.help(Msg::WorkspaceHelp.text()))
                    .mut_arg("repository", |a| a.help(Msg::GithubAppRepository.text()))
                    .mut_arg("retry_after", |a| a.help(Msg::GithubAppRetryAfter.text()))
            })
    });
    #[cfg(feature = "ui")]
    let matches = matches.mut_subcommand("ui", |c| {
        (c.about(Msg::Ui.text()))
            .mut_arg("listen", |a| a.help(Msg::UiListenHelp.text()))
            .mut_arg("workspace", |a| a.help(Msg::UiWorkspaceHelp.text()))
    });
    #[cfg(feature = "generation-pointer")]
    let matches = matches.mut_subcommand("workspace", |c| {
        let bootstrap = |a: clap::Arg| a.help(Msg::WorkspaceBootstrapHelp.text());
        (c.about(Msg::Workspace.text()))
            .mut_subcommand("import", |c| {
                (c.about(Msg::WorkspaceImport.text()))
                    .mut_arg("file", |a| a.help(Msg::WorkspaceImportFile.text()))
                    .mut_arg("message", |a| a.help(Msg::WorkspaceImportMessage.text()))
                    .mut_arg("author", |a| a.help(Msg::WorkspaceImportAuthor.text()))
                    .mut_arg("workspace", bootstrap)
            })
            .mut_subcommand("list", |c| {
                (c.about(Msg::WorkspaceList.text())).mut_arg("workspace", bootstrap)
            })
            .mut_subcommand("show", |c| {
                (c.about(Msg::WorkspaceShow.text()))
                    .mut_arg("version", |a| a.help(Msg::WorkspaceShowVersion.text()))
                    .mut_arg("workspace", bootstrap)
            })
            .mut_subcommand("pull", |c| {
                (c.about(Msg::WorkspacePull.text())).mut_arg("workspace", bootstrap)
            })
    });
    #[cfg(feature = "refresh-lock")]
    let matches = matches.mut_subcommand("workspace", |c| {
        c.mut_subcommand("apply", |c| {
            (c.about(Msg::WorkspaceApply.text()))
                .mut_arg("version", |a| a.help(Msg::WorkspaceApplyVersion.text()))
                .mut_arg("workspace", |a| a.help(Msg::WorkspaceBootstrapHelp.text()))
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
                (Some(listen), Some(manifest)) => {
                    drained(|| meada::mcp::listen(&listen, &manifest))
                }
                _ => meada::mcp::serve(),
            }
        }
        Command::Status {
            workspace: Some(manifest),
            ..
        } => meada::with_busy_timeout(engine::PROBE_BUSY, || {
            engine::probe_workspace(&manifest).map(|(status, ready)| {
                print!("{status}");
                unready = !ready;
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
        #[cfg(feature = "github-app")]
        Command::GithubApp { command } => github_app(command),
        #[cfg(feature = "generation-pointer")]
        Command::Workspace { command } => workspace(command),
        // Every problem that refuses the start on its own line. The API is English whatever
        // `LANG` says, as the MCP surface is (ADR 0008).
        #[cfg(feature = "ui")]
        Command::Ui { listen, workspace } => {
            msg::init(None);
            if let Err(e) = drained(|| meada::ui::listen(&listen, &workspace)) {
                for line in e.to_string().lines() {
                    eprintln!("meada: {line}");
                }
                std::process::exit(1);
            }
            Ok(())
        }
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
        // The new state is the only line on stdout.
        #[cfg(feature = "refresh-lock")]
        Command::Snapshot {
            command:
                SnapshotCommand::Pointer {
                    command:
                        PointerCommand::Set {
                            generation,
                            workspace,
                        },
                },
        } => {
            let author = meada::snapshot::lock::cli_author();
            meada::snapshot::operation::pointer_set(&workspace, &generation, &author)
                .map(|state| println!("{state}"))
        }
        #[cfg(feature = "refresh-lock")]
        Command::Snapshot {
            command:
                SnapshotCommand::Pointer {
                    command:
                        PointerCommand::Retain {
                            generation,
                            workspace,
                        },
                },
        } => {
            let author = meada::snapshot::lock::cli_author();
            meada::snapshot::operation::pointer_retain(&workspace, &generation, &author)
                .map(|id| println!("{id}"))
        }
        #[cfg(feature = "refresh-lock")]
        Command::Snapshot {
            command:
                SnapshotCommand::Pointer {
                    command:
                        PointerCommand::Drop {
                            generation,
                            workspace,
                        },
                },
        } => {
            let author = meada::snapshot::lock::cli_author();
            meada::snapshot::operation::pointer_drop(&workspace, &generation, &author)
                .map(|id| println!("{id}"))
        }
        // What could not be counted on stderr, the new fence on stdout.
        #[cfg(feature = "refresh-lock")]
        Command::Snapshot {
            command:
                SnapshotCommand::Fence {
                    command:
                        FenceCommand::Recover {
                            at_least,
                            workspace,
                        },
                },
        } => {
            let author = meada::snapshot::lock::cli_author();
            let at_least = at_least.as_deref();
            meada::snapshot::operation::fence_recover(&workspace, at_least, &author).map(
                |(fence, skipped)| {
                    for line in skipped.iter().flat_map(|s| s.lines()) {
                        eprintln!("meada: {line}");
                    }
                    println!("{fence}");
                },
            )
        }
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
