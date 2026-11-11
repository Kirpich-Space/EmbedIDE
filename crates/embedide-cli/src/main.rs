//! EmbedIDE command-line IDE. No GUI, Electron or Node.js runtime is required.
mod cmd_ai;
mod cmd_project;
mod cmd_toolchain;
mod serial_bridge;

use clap::{Parser, Subcommand};
use cmd_project::{BuildArgs, ConfigCmd, ProjectCmd};
use cmd_toolchain::ToolchainCmd;
use embedide_core::{self as core, BuildOptions, BuildSystem, Error};
use serde_json::{json, Value};
use std::{
    io::{self, IsTerminal, Write},
    path::PathBuf,
    process::ExitCode,
};

#[derive(Parser)]
#[command(
    name = "embed",
    version,
    override_usage = "embed [PATH]\n       embed [OPTIONS] <COMMAND>",
    about = "EmbedIDE — engineering IDE for firmware, drivers, kernels and systems code",
    long_about = "EmbedIDE terminal edition.\n\nRun `embed` to open the full-screen, keyboard-driven IDE in the current directory, \
`embed <directory>` to open that directory, or `embed <file>` to open a file (a missing file in an existing directory \
becomes a new buffer, written only on :w). The IDE has an explorer (Ctrl+Alt+1), editor with Vim modes and syntax \
highlighting, AI assistant (Ctrl+Alt+2), interactive terminal (Ctrl+Alt+3; Alt+1/2/3 in terminals without the kitty \
keyboard protocol), build output and diagnostics. :help inside the IDE lists every key. \
Subcommand names win over same-named files: use `embed ./build` for a file called build.\n\n\
The subcommands below are for scripts and automation.\n\nYou choose the compilers: EmbedIDE detects what is installed (GCC, Clang, zig cc, \
rustc/Cargo, Zig, GNU as, NASM, gfortran, Flang, LFortran, cross GCCs) and builds with exactly what you select. \
It never installs toolchains, never flashes hardware and never starts a debugger on its own."
)]
struct Cli {
    /// Emit a single JSON result on stdout; tool output goes to stderr.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Check the environment: OS, configuration, toolchains per language.
    Doctor,
    /// Discover, inspect and configure compilers (you choose; nothing is installed).
    #[command(subcommand)]
    Toolchain(ToolchainCmd),
    /// Create, adopt and inspect projects (embedide.toml).
    #[command(subcommand)]
    Project(ProjectCmd),
    /// Build a project with its selected toolchains or its own build system.
    Build {
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Build profile from embedide.toml (debug, release, …).
        #[arg(long)]
        profile: Option<String>,
        /// Override a toolchain for this build, e.g. --toolchain c=zig-cc (repeatable).
        #[arg(long, value_parser = cmd_project::parse_choice)]
        toolchain: Vec<(String, String)>,
        /// Projects without embedide.toml: choose among several manifests.
        #[arg(long)]
        build_system: Option<BuildSystem>,
        /// Same as --profile release.
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
    /// Remove EmbedIDE build output (only directories EmbedIDE created) or run the build system's clean.
    Clean {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        dry_run: bool,
    },
    /// Open your shell in the project with its environment and toolchain directories on PATH.
    Terminal {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long, default_value = "debug")]
        profile: String,
    },
    /// Read and change user or project configuration.
    #[command(subcommand)]
    Config(ConfigCmd),
    /// AI providers, chat and code completion (network only on explicit commands).
    #[command(subcommand)]
    Ai(cmd_ai::AiCmd),
    /// Register a directory with embedide.json (legacy; prefer `project init`).
    Init {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        build_system: Option<BuildSystem>,
    },
    /// Open the project TUI on a directory or file (`embed <path>` is the same);
    /// --json or redirected I/O inspects metadata without execution.
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
    /// PATH inventory of known tools (does not execute them).
    Toolchains,
    /// List the MCU catalog (metadata only; hardware unverified).
    Targets,
    /// JSON-lines serial service on stdin/stdout (used by front ends).
    SerialBridge,
}

/// CLI error: usage (exit 2) or operation (exit 1).
pub struct CliError {
    usage: bool,
    message: String,
}
/// Result of a command.
pub type CliResult<T> = std::result::Result<T, CliError>;
/// Operation error.
pub fn fail(message: String) -> CliError {
    CliError {
        usage: false,
        message,
    }
}
/// Usage error.
pub fn usage(message: String) -> CliError {
    CliError {
        usage: true,
        message,
    }
}
impl From<Error> for CliError {
    fn from(e: Error) -> Self {
        fail(e.0)
    }
}

/// Human output goes to stdout only in text mode; JSON mode prints one envelope.
pub struct Out {
    /// JSON output mode.
    pub json: bool,
}
impl Out {
    /// A line of human output.
    pub fn line(&self, s: &str) {
        if !self.json {
            let _ = writeln!(io::stdout(), "{s}");
        }
    }
    /// A progress/diagnostic line (always stderr).
    pub fn err(&self, s: &str) {
        let _ = writeln!(io::stderr(), "{s}");
    }
}

/// (data, exit code, already printed for humans)
type Executed = (Value, u8, bool);

fn execute(command: Commands, interactive: bool, out: &Out) -> CliResult<Executed> {
    Ok(match command {
        Commands::Toolchain(c) => (cmd_toolchain::run(c, out)?, 0, true),
        Commands::Project(c) => (cmd_project::project(c, out)?, 0, true),
        Commands::Config(c) => {
            let shows = matches!(c, ConfigCmd::Show { .. });
            (cmd_project::config(c, out)?, 0, !shows)
        }
        Commands::Clean { path, dry_run } => (cmd_project::clean(&path, dry_run, out)?, 0, true),
        Commands::Terminal { path, profile } => {
            let (v, c) = cmd_project::terminal(&path, &profile, out)?;
            (v, c, true)
        }
        Commands::Ai(c) => {
            let (v, c) = cmd_ai::run(c, out)?;
            (v, c, true)
        }
        Commands::Doctor => {
            let user = embedide_workspace::UserConfig::load();
            let tools = cmd_toolchain::registry(user.as_ref().unwrap_or(&Default::default()), true);
            let per_lang: serde_json::Map<String, Value> = embedide_toolchains::Language::ALL
                .iter()
                .map(|l| {
                    (
                        l.id().to_string(),
                        json!(tools
                            .iter()
                            .filter(|t| t.supports(*l) && t.problem.is_none())
                            .map(|t| t.id.clone())
                            .collect::<Vec<_>>()),
                    )
                })
                .collect();
            out.line(&format!(
                "EmbedIDE {} on {}/{}",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::OS,
                std::env::consts::ARCH
            ));
            match embedide_workspace::UserConfig::path() {
                Ok(p) => out.line(&format!(
                    "config: {}{}",
                    p.display(),
                    if p.exists() { "" } else { " (not created yet)" }
                )),
                Err(e) => out.line(&format!("config: {}", e.0)),
            }
            if let Err(e) = &user {
                out.line(&format!("config error: {}", e.0));
            }
            for (lang, ids) in &per_lang {
                let ids: Vec<_> = ids
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v.as_str())
                    .collect();
                out.line(&format!(
                    "  {lang:<8} {}",
                    if ids.is_empty() {
                        "— none (see `embed toolchain recommend`)".to_string()
                    } else {
                        ids.join(", ")
                    }
                ));
            }
            out.line("Hardware is never probed by doctor.");
            (
                json!({"os": std::env::consts::OS, "arch": std::env::consts::ARCH, "version": env!("CARGO_PKG_VERSION"),
                    "inventory_only": true, "hardware_probed": false, "tools": core::toolchains(),
                    "languages": per_lang, "config_error": user.err().map(|e| e.0)}),
                0,
                true,
            )
        }
        Commands::Init { path, build_system } => (
            serde_json::to_value(core::init(&path, build_system)?).unwrap_or_default(),
            0,
            false,
        ),
        Commands::Open { path } => {
            let cwd = std::env::current_dir()
                .map_err(|e| fail(format!("Cannot read the current directory: {e}")))?;
            let launch = embedide_workspace::resolve_launch(&path, &cwd).map_err(|e| fail(e.0))?;
            if interactive {
                embedide_terminal::tui::workspace::run_with(&launch.root, launch.file.as_ref())
                    .map_err(|e| fail(e.to_string()))?;
                (json!({"workspace": launch.root, "closed": true}), 0, false)
            } else if let Some(embedide_workspace::LaunchFile::New(file)) = &launch.file {
                // Usage error, like an unknown subcommand: scripts with a typo keep exit 2.
                return Err(usage(format!(
                    "{} does not exist and is not a subcommand (an interactive `embed {}` opens it as a new, unsaved file)",
                    file.display(),
                    path.display()
                )));
            } else {
                let mut value =
                    serde_json::to_value(core::inspect(&launch.root)?).unwrap_or_default();
                if let (Some(embedide_workspace::LaunchFile::Existing(file)), Some(map)) =
                    (&launch.file, value.as_object_mut())
                {
                    map.insert("file".into(), json!(file));
                }
                (value, 0, false)
            }
        }
        Commands::Edit { path, batch, line } => {
            let editor: core::Editor =
                embedide_terminal::open(&path).map_err(|error| fail(error.to_string()))?;
            let report = (if line {
                embedide_terminal::run_line_stdio(editor, false)
            } else {
                embedide_terminal::run_stdio(editor, batch)
            })
            .map_err(|error| fail(error.to_string()))?;
            (
                json!({"path": path.to_string_lossy(), "closed": report.closed, "modified": report.modified,
                "bytes": report.bytes, "line": report.position.line, "scalar_column": report.position.scalar_column}),
                0,
                false,
            )
        }
        Commands::Targets => (
            json!({"hardware_verified": false, "targets": core::targets()?}),
            0,
            false,
        ),
        Commands::Toolchains => (json!({"tools": core::toolchains()}), 0, false),
        Commands::Build {
            path,
            profile,
            toolchain,
            build_system,
            release,
            jobs,
            online,
            dry_run,
        } => {
            let root = path
                .canonicalize()
                .map_err(|e| fail(format!("Cannot open {}: {e}", path.display())))?;
            if let Some(m) = embedide_workspace::Manifest::load(&root).map_err(|e| fail(e.0))? {
                if build_system.is_some() {
                    return Err(usage("--build-system applies to projects without embedide.toml; set build.system instead".into()));
                }
                let profile = profile.unwrap_or_else(|| {
                    if release {
                        "release".into()
                    } else {
                        "debug".into()
                    }
                });
                let (v, c) = cmd_project::build_manifest(
                    &root,
                    &m,
                    BuildArgs {
                        profile,
                        toolchain,
                        jobs,
                        online,
                        dry_run,
                    },
                    out,
                )?;
                (v, c, true)
            } else {
                if profile.is_some() || !toolchain.is_empty() {
                    return Err(fail(format!(
                        "--profile/--toolchain need an embedide.toml; run `embed project init {}`",
                        path.display()
                    )));
                }
                let project = core::inspect(&root)?;
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
                    (json!({"plan": plan, "executed": false}), 0, false)
                } else {
                    let code = core::run_build(&plan)? as u8;
                    (
                        json!({"plan": plan, "executed": true, "exit_code": code}),
                        code,
                        false,
                    )
                }
            }
        }
        Commands::SerialBridge => unreachable!("handled before execute"),
    })
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
        "toolchain",
        "doctor",
        "build",
        "clean",
        "terminal",
        "project",
        "ai",
        "serial-bridge",
        "help",
    ];
    if args.len() == 1 {
        // `embed` alone opens the IDE in the current directory.
        if !(io::stdin().is_terminal() && io::stdout().is_terminal()) {
            eprintln!("embed: the IDE needs an interactive terminal; see `embed --help` for scriptable commands");
            return ExitCode::from(2);
        }
        args.extend(["open".into(), ".".into()]);
    }
    // `embed <path>`: a file or directory to open. Subcommand names and
    // options keep their meaning; `embed ./build` opens a file named build.
    if args.len() == 2
        && !commands.iter().any(|name| args[1] == *name)
        && !args[1].as_encoded_bytes().starts_with(b"-")
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
            &json!({"ok": false, "error": {"kind": "usage", "message": "edit --json requires --batch to keep stdout structured"}}),
            true,
        );
        return ExitCode::from(2);
    }
    if matches!(cli.command, Commands::SerialBridge) {
        return match serial_bridge::run() {
            Ok(code) => ExitCode::from(code),
            Err(e) if e.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("embed serial-bridge: {e}");
                ExitCode::from(1)
            }
        };
    }
    let out = Out { json: cli.json };
    let interactive = !cli.json && io::stdin().is_terminal() && io::stdout().is_terminal();
    let (value, code, printed) = match execute(cli.command, interactive, &out) {
        Ok((value, code, printed)) => (json!({"ok": code == 0, "data": value}), code, printed),
        Err(CliError { usage, message }) => {
            let kind = if usage { "usage" } else { "operation" };
            if !cli.json {
                eprintln!("embed: {message}");
                return ExitCode::from(if usage { 2 } else { 1 });
            }
            (
                json!({"ok": false, "error": {"kind": kind, "message": message}}),
                if usage { 2 } else { 1 },
                false,
            )
        }
    };
    if printed && !cli.json {
        return ExitCode::from(code);
    }
    match output(&value, cli.json) {
        Ok(()) => ExitCode::from(code),
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("embed: cannot write output: {e}");
            ExitCode::from(1)
        }
    }
}
