//! Standalone terminal frontend; no GUI or Node.js runtime is required.
mod serial_bridge;

use clap::{Parser, Subcommand};
use embedide_core::{self as core, BuildOptions, BuildSystem, Error, Result};
use serde_json::{json, Value};
use std::{
    io::{self, IsTerminal, Write},
    path::PathBuf,
    process::ExitCode,
};

#[derive(Parser)]
#[command(
    name = "embedide",
    version,
    about = "Offline-first embedded project tools (Rust migration foundation)"
)]
struct Cli {
    /// Emit a single JSON result on stdout; build output goes to stderr.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Register a directory; never overwrite existing project metadata or files.
    Init {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        build_system: Option<BuildSystem>,
    },
    /// Open the project TUI; --json or redirected I/O inspects metadata without execution.
    Open {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Edit a UTF-8 file using the shared Vim engine and full-screen terminal frontend.
    Edit {
        path: PathBuf,
        /// Read key lines from stdin without prompts; stop on the first error.
        #[arg(long)]
        batch: bool,
        /// Use the legacy cooked-mode key-line frontend.
        #[arg(long, conflicts_with = "batch")]
        line: bool,
    },
    /// Build a trusted project through its native build system.
    Build {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        build_system: Option<BuildSystem>,
        #[arg(long)]
        release: bool,
        #[arg(long, default_value = "2", value_parser = clap::value_parser!(u16).range(1..))]
        jobs: u16,
        /// Allow Cargo network access (other build systems use project policies).
        #[arg(long)]
        online: bool,
        /// Show the plan without executing it.
        #[arg(long)]
        dry_run: bool,
    },
    /// Inventory executable paths; does not execute tools or probe hardware.
    Doctor,
    /// List executable paths for compilers, build tools and language servers.
    Toolchains,
    /// List preserved MCU catalog metadata (hardware unverified).
    Targets,
    /// Read project configuration without changing it.
    Config {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// JSON-lines serial service on stdin/stdout (used by the desktop app).
    SerialBridge,
}

fn execute(command: Commands, interactive: bool) -> Result<(Value, u8)> {
    let value = match command {
        Commands::Init { path, build_system } => {
            serde_json::to_value(core::init(&path, build_system)?)?
        }
        Commands::Open { path } => {
            if interactive {
                embedide_terminal::tui::workspace::run(&path).map_err(|e| Error(e.to_string()))?;
                json!({"workspace": path, "closed": true})
            } else {
                serde_json::to_value(core::inspect(&path)?)?
            }
        }
        Commands::Edit { path, batch, line } => {
            let editor: core::Editor =
                embedide_terminal::open(&path).map_err(|error| Error(error.to_string()))?;
            let report = (if line {
                embedide_terminal::run_line_stdio(editor, false)
            } else {
                embedide_terminal::run_stdio(editor, batch)
            })
            .map_err(|error| Error(error.to_string()))?;
            json!({"path": path.to_string_lossy(), "closed": report.closed, "modified": report.modified,
                "bytes": report.bytes, "line": report.position.line, "scalar_column": report.position.scalar_column})
        }
        Commands::Config { path } => json!({"project": core::inspect(&path)?.metadata}),
        Commands::SerialBridge => unreachable!("handled before execute"),
        Commands::Targets => json!({"hardware_verified": false, "targets": core::targets()?}),
        Commands::Toolchains => json!({"tools": core::toolchains()}),
        Commands::Doctor => json!({"os": std::env::consts::OS, "arch": std::env::consts::ARCH,
            "inventory_only": true, "hardware_probed": false, "tools": core::toolchains()}),
        Commands::Build {
            path,
            build_system,
            release,
            jobs,
            online,
            dry_run,
        } => {
            let project = core::inspect(&path)?;
            let plan = core::plan_build(
                &project,
                &BuildOptions {
                    system: build_system,
                    release,
                    jobs: jobs.into(),
                    online,
                },
            )?;
            if dry_run {
                json!({"plan": plan, "executed": false})
            } else {
                let code = core::run_build(&plan)? as u8;
                return Ok((
                    json!({"plan": plan, "executed": true, "exit_code": code}),
                    code,
                ));
            }
        }
    };
    Ok((value, 0))
}

fn output(value: &Value, json_output: bool) -> io::Result<()> {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    if json_output {
        serde_json::to_writer(&mut out, value)?;
    } else {
        serde_json::to_writer_pretty(&mut out, value)?;
    }
    writeln!(out)
}

fn main() -> ExitCode {
    let mut args: Vec<_> = std::env::args_os().collect();
    let commands = [
        "init",
        "open",
        "edit",
        "config",
        "targets",
        "toolchains",
        "doctor",
        "build",
        "serial-bridge",
        "help",
    ];
    if args.len() == 2
        && !commands.iter().any(|name| args[1] == *name)
        && std::path::Path::new(&args[1]).is_dir()
    {
        args.insert(1, "open".into());
    }
    let wants_json = args.iter().any(|s| s == "--json");
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => {
            if error.exit_code() == 0 {
                let _ = error.print();
                return ExitCode::SUCCESS;
            }
            if wants_json {
                let _ = output(
                    &json!({"ok": false, "error": {"kind": "usage", "message": error.to_string()}}),
                    true,
                );
            } else {
                let _ = error.print();
            }
            return ExitCode::from(2);
        }
    };
    if cli.json && matches!(&cli.command, Commands::Edit { batch: false, .. }) {
        let _ = output(
            &json!({"ok": false, "error": {"kind": "usage",
            "message": "edit --json requires --batch to keep stdout structured"}}),
            true,
        );
        return ExitCode::from(2);
    }
    if matches!(cli.command, Commands::SerialBridge) {
        return match serial_bridge::run() {
            Ok(code) => ExitCode::from(code),
            Err(e) if e.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("embedide serial-bridge: {e}");
                ExitCode::from(1)
            }
        };
    }
    let (value, code) = match execute(
        cli.command,
        !cli.json && io::stdin().is_terminal() && io::stdout().is_terminal(),
    ) {
        Ok((value, code)) => (json!({"ok": code == 0, "data": value}), code),
        Err(Error(message)) => {
            if !cli.json {
                eprintln!("embedide: {message}");
                return ExitCode::from(1);
            }
            (
                json!({"ok": false, "error": {"kind": "operation", "message": message}}),
                1,
            )
        }
    };
    match output(&value, cli.json) {
        Ok(()) => ExitCode::from(code),
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("embedide: cannot write output: {e}");
            ExitCode::from(1)
        }
    }
}
