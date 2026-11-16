//! `embed project …`, `build`, `clean`, `terminal`, `config …`
use crate::{cmd_toolchain::registry, fail, CliResult, Out};
use clap::Subcommand;
use embedide_build::{self as build, CleanAction, Event, Request};
use embedide_toolchains::Language;
use embedide_workspace::{templates, ConfigFile, Manifest, UserConfig, MANIFEST_FILE};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Subcommand)]
pub enum ProjectCmd {
    /// Create a project (or adopt an existing directory without touching its files).
    Init {
        /// Directory to create or adopt.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Project name (defaults to the directory name).
        #[arg(long)]
        name: Option<String>,
        /// Primary language: c, cpp, rust, zig, asm, fortran.
        #[arg(long)]
        language: Option<Language>,
        /// Template (see `embed project templates`).
        #[arg(long)]
        template: Option<String>,
        /// Record a toolchain choice, e.g. --toolchain c=clang (repeatable).
        #[arg(long, value_parser = parse_choice)]
        toolchain: Vec<(String, String)>,
        /// Board id for firmware templates (see `embed targets`).
        #[arg(long)]
        board: Option<String>,
    },
    /// Show the project configuration and the toolchains it would use.
    Info {
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Profile whose toolchain selection to show.
        #[arg(long, default_value = "debug")]
        profile: String,
    },
    /// List project templates.
    Templates,
}

#[derive(Subcommand)]
pub enum ConfigCmd {
    /// Print every key = value (user config, or the project with --project).
    List {
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// Print one value.
    Get {
        key: String,
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// Set a value (parsed as TOML: true, 3, ["-O2"], otherwise a string).
    Set {
        key: String,
        value: String,
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// Remove a key.
    Unset {
        key: String,
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// Print configuration file locations.
    Path,
    /// Legacy: print embedide.json project metadata.
    Show {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
}

/// `lang=toolchain` pairs for --toolchain.
pub fn parse_choice(s: &str) -> Result<(String, String), String> {
    let (l, t) = s
        .split_once('=')
        .ok_or("expected LANGUAGE=TOOLCHAIN, e.g. c=clang")?;
    let lang: Language = l.parse()?;
    if t.is_empty() {
        return Err("toolchain id is empty".into());
    }
    Ok((lang.id().to_string(), t.to_string()))
}

fn user() -> CliResult<UserConfig> {
    UserConfig::load().map_err(|e| fail(e.0))
}

pub fn project(cmd: ProjectCmd, out: &Out) -> CliResult<Value> {
    match cmd {
        ProjectCmd::Templates => {
            for (id, d) in templates::TEMPLATES {
                out.line(&format!("{id:<12} {d}"));
            }
            Ok(
                json!({"templates": templates::TEMPLATES.iter().map(|(i, d)| json!({"id": i, "description": d})).collect::<Vec<_>>()}),
            )
        }
        ProjectCmd::Init {
            path,
            name,
            language,
            template,
            toolchain,
            board,
        } => {
            let abs = if path.is_absolute() {
                path.clone()
            } else {
                std::env::current_dir()
                    .map_err(|e| fail(e.to_string()))?
                    .join(&path)
            };
            // Default name from the directory, made valid ("my app" → "my-app")
            let name = name.unwrap_or_else(|| {
                let raw = abs
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let clean: String = raw
                    .trim()
                    .chars()
                    .map(|c| match c {
                        c if c.is_ascii_alphanumeric() || "-_.".contains(c) => c,
                        c if c.is_whitespace() => '-',
                        _ => '_',
                    })
                    .collect();
                let clean = clean.trim_start_matches(['.', '-']).to_string();
                if clean.is_empty() {
                    "project".into()
                } else {
                    clean
                }
            });
            let created = templates::create(
                &abs,
                &name,
                &templates::CreateOptions {
                    template,
                    language,
                    toolchain: toolchain.into_iter().collect(),
                    board,
                },
            )
            .map_err(|e| fail(e.0))?;
            if created.adopted {
                out.line(&format!(
                    "Adopted {} ({:?} build); only {MANIFEST_FILE} was written",
                    created.root.display(),
                    created.system
                ));
            } else {
                out.line(&format!(
                    "Created {} ({:?} build):",
                    created.root.display(),
                    created.system
                ));
                for f in &created.files {
                    out.line(&format!("  {f}"));
                }
            }
            Ok(serde_json::to_value(&created).unwrap_or_default())
        }
        ProjectCmd::Info { path, profile } => {
            let root = path
                .canonicalize()
                .map_err(|e| fail(format!("Cannot open {}: {e}", path.display())))?;
            let Some(m) = Manifest::load(&root).map_err(|e| fail(e.0))? else {
                let legacy = embedide_core::inspect(&root).map_err(|e| fail(e.0))?;
                out.line(&format!("{} has no {MANIFEST_FILE}; legacy project (build systems: {:?}). Run `embed project init` to add one.", root.display(), legacy.detected_build_systems));
                return Ok(json!({"root": root, "manifest": null, "legacy": legacy}));
            };
            let user = user()?;
            let tools = registry(&user, true);
            let req = Request {
                profile: profile.clone(),
                ..Default::default()
            };
            let mut selection = Vec::new();
            let mut problems = Vec::new();
            match build::prepare(&root, Some(&m), &user, &tools, &req) {
                Ok(p) => selection = p.toolchains,
                Err(e) => problems.push(e.0),
            }
            out.line(&format!(
                "project:  {} ({})",
                m.project.name, m.project.language
            ));
            out.line(&format!("root:     {}", root.display()));
            out.line(&format!(
                "build:    {:?}, sources {:?}, output {}",
                m.build.system, m.build.sources, m.build.output
            ));
            out.line(&format!(
                "profiles: {}",
                m.profiles.keys().cloned().collect::<Vec<_>>().join(", ")
            ));
            if let Some(b) = &m.target.board {
                out.line(&format!("target:   board {b} {}", m.target.flags.join(" ")));
            }
            for s in &selection {
                out.line(&format!(
                    "  {:<8} {:<20} {} ({})",
                    s.language.id(),
                    s.id,
                    s.program.display(),
                    s.reason
                ));
            }
            for p in problems.iter().chain(m.warnings().iter()) {
                out.line(&format!("note: {p}"));
            }
            Ok(
                json!({"root": root, "manifest": m, "profile": profile, "toolchains": selection, "problems": problems, "warnings": m.warnings()}),
            )
        }
    }
}

/// Options for `embed build` on a manifest project.
pub struct BuildArgs {
    pub profile: String,
    pub toolchain: Vec<(String, String)>,
    pub jobs: u16,
    pub online: bool,
    pub dry_run: bool,
}

pub fn build_manifest(
    root: &Path,
    m: &Manifest,
    a: BuildArgs,
    out: &Out,
) -> CliResult<(Value, u8)> {
    let user = user()?;
    // Toolchains the user (or the project, by name) requires and the machine lacks are downloaded first.
    if !a.dry_run {
        build::ensure_required(&user, Some(m), &mut |line| out.err(line)).map_err(|e| fail(e.0))?;
    }
    let tools = registry(&user, true);
    let req = Request {
        profile: a.profile,
        toolchain: a.toolchain.into_iter().collect::<BTreeMap<_, _>>(),
        jobs: a.jobs.into(),
        online: a.online,
    };
    let prepared = build::prepare(root, Some(m), &user, &tools, &req).map_err(|e| fail(e.0))?;
    for s in &prepared.toolchains {
        out.err(&format!(
            "[{}] {} → {} {}",
            s.language.id(),
            s.id,
            s.program.display(),
            s.version.as_deref().unwrap_or("")
        ));
    }
    if a.dry_run {
        for s in &prepared.steps {
            out.line(&format!("$ {} {}", s.program, shell_display(&s.args)));
        }
        return Ok((json!({"prepared": prepared, "executed": false}), 0));
    }
    let cancel = build::Cancel::default();
    let c = cancel.clone();
    let _ = ctrlc::set_handler(move || c.cancel());
    let json = out.json;
    let outcome = build::run(&prepared, &cancel, |ev| match ev {
        Event::Command(c) => {
            let _ = writeln!(std::io::stderr(), "$ {c}");
        }
        Event::Output(t, _) => {
            // Child output goes to stderr so stdout can carry a single JSON result
            let _ = std::io::stderr().write_all(t.as_bytes());
            let _ = json;
        }
    })
    .map_err(|e| fail(e.0))?;
    let errors = outcome
        .diagnostics
        .iter()
        .filter(|d| d.severity == "error")
        .count();
    let warnings = outcome
        .diagnostics
        .iter()
        .filter(|d| d.severity == "warning")
        .count();
    if outcome.cancelled {
        out.line("Build cancelled");
    } else if outcome.success {
        out.line(&format!(
            "Build succeeded ({} step(s), {} up to date, {warnings} warning(s), {} ms){}",
            prepared.steps.len(),
            prepared.up_to_date,
            outcome.elapsed_ms,
            outcome
                .artifact
                .as_ref()
                .map(|a| format!(" → {}", a.display()))
                .unwrap_or_default()
        ));
    } else {
        out.line(&format!(
            "Build failed (exit {}): {errors} error(s), {warnings} warning(s)",
            outcome
                .exit_code
                .map(|c| c.to_string())
                .unwrap_or("?".into())
        ));
        for d in outcome
            .diagnostics
            .iter()
            .filter(|d| d.severity == "error")
            .take(10)
        {
            out.line(&format!(
                "  {}:{}:{}: {}",
                d.file
                    .as_ref()
                    .map(|f| f.display().to_string())
                    .unwrap_or_default(),
                d.line,
                d.column,
                d.message
            ));
        }
    }
    let code = if outcome.success {
        0
    } else if outcome.cancelled {
        130
    } else {
        outcome
            .exit_code
            .filter(|c| (1..=255).contains(c))
            .unwrap_or(1) as u8
    };
    Ok((
        json!({"prepared": prepared, "executed": true, "outcome": outcome}),
        code,
    ))
}

fn shell_display(args: &[String]) -> String {
    args.iter()
        .map(|a| {
            if a.is_empty() || a.contains(|c: char| c.is_whitespace() || "'\"$\\".contains(c)) {
                format!("'{}'", a.replace('\'', "'\\''"))
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn clean(path: &Path, dry_run: bool, out: &Out) -> CliResult<Value> {
    let root = path
        .canonicalize()
        .map_err(|e| fail(format!("Cannot open {}: {e}", path.display())))?;
    let m = Manifest::load(&root).map_err(|e| fail(e.0))?;
    let action = build::plan_clean(&root, m.as_ref()).map_err(|e| fail(e.0))?;
    match &action {
        CleanAction::Remove(d) => {
            out.line(&format!(
                "{} {}",
                if dry_run { "Would remove" } else { "Removing" },
                d.display()
            ));
            if !dry_run {
                std::fs::remove_dir_all(d)
                    .map_err(|e| fail(format!("Cannot remove {}: {e}", d.display())))?;
            }
        }
        CleanAction::Run(spec) => {
            out.line(&format!(
                "{} {} {}",
                if dry_run { "Would run" } else { "Running" },
                spec.program,
                spec.args.join(" ")
            ));
            if !dry_run {
                let st = std::process::Command::new(&spec.program)
                    .args(&spec.args)
                    .current_dir(&root)
                    .stdout(std::io::stderr())
                    .status()
                    .map_err(|e| fail(format!("Cannot run {}: {e}", spec.program)))?;
                if !st.success() {
                    return Err(fail(format!(
                        "{} {} failed",
                        spec.program,
                        spec.args.join(" ")
                    )));
                }
            }
        }
        CleanAction::Nothing(why) => out.line(&format!("Nothing to clean: {why}")),
    }
    Ok(json!({"action": action, "executed": !dry_run}))
}

/// Interactive shell in the project with its environment and toolchain dirs on PATH.
pub fn terminal(path: &Path, profile: &str, out: &Out) -> CliResult<(Value, u8)> {
    if out.json {
        return Err(crate::usage(
            "terminal is interactive; it cannot run with --json".into(),
        ));
    }
    let root = path
        .canonicalize()
        .map_err(|e| fail(format!("Cannot open {}: {e}", path.display())))?;
    let mut env: BTreeMap<String, String> = BTreeMap::new();
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(m) = Manifest::load(&root).map_err(|e| fail(e.0))? {
        env.extend(m.env.clone());
        let user = user()?;
        let tools = registry(&user, false);
        if let Ok(p) = build::prepare(
            &root,
            Some(&m),
            &user,
            &tools,
            &Request {
                profile: profile.into(),
                ..Default::default()
            },
        ) {
            for s in p.toolchains {
                if let Some(d) = s.program.parent() {
                    if !dirs.iter().any(|x| x == d) {
                        dirs.push(d.to_path_buf());
                    }
                }
            }
        }
    }
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    let joined = std::env::join_paths(dirs.iter().cloned().chain(std::env::split_paths(&path_var)))
        .map_err(|e| fail(e.to_string()))?;
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/bin/sh".into());
    eprintln!("EmbedIDE terminal: {shell} in {}", root.display());
    for d in &dirs {
        eprintln!("  PATH += {}", d.display());
    }
    eprintln!("  exit the shell to return");
    let status = std::process::Command::new(&shell)
        .current_dir(&root)
        .envs(&env)
        .env("PATH", joined)
        .env("EMBEDIDE_PROJECT", &root)
        .status()
        .map_err(|e| fail(format!("Cannot start {shell}: {e}")))?;
    Ok((
        json!({"shell": shell, "exit_code": status.code()}),
        status.code().unwrap_or(1).clamp(0, 255) as u8,
    ))
}

pub fn config(cmd: ConfigCmd, out: &Out) -> CliResult<Value> {
    let file = |project: &Option<PathBuf>| -> CliResult<(ConfigFile, bool)> {
        match project {
            Some(p) => Ok((
                ConfigFile::open(&p.join(MANIFEST_FILE)).map_err(|e| fail(e.0))?,
                true,
            )),
            None => Ok((
                ConfigFile::open(&UserConfig::path().map_err(|e| fail(e.0))?)
                    .map_err(|e| fail(e.0))?,
                false,
            )),
        }
    };
    let check = |is_project: bool| {
        move |t: &str| -> embedide_workspace::Result<()> {
            if is_project {
                Manifest::parse(t).map(|_| ())
            } else {
                UserConfig::parse(t).map(|_| ())
            }
        }
    };
    match cmd {
        ConfigCmd::Path => {
            let p = UserConfig::path().map_err(|e| fail(e.0))?;
            out.line(&format!("user config:  {}", p.display()));
            out.line(&format!("project file: <project>/{MANIFEST_FILE}"));
            Ok(json!({"user": p, "project_file": MANIFEST_FILE}))
        }
        ConfigCmd::List { project } => {
            let (f, _) = file(&project)?;
            let items = f.list();
            for (k, v) in &items {
                out.line(&format!("{k} = {v}"));
            }
            if items.is_empty() {
                out.line(&format!("(empty: {})", f.path().display()));
            }
            Ok(json!({"file": f.path(), "values": items.into_iter().collect::<BTreeMap<_, _>>()}))
        }
        ConfigCmd::Get { key, project } => {
            let (f, _) = file(&project)?;
            let v = f
                .get(&key)
                .map_err(|e| fail(e.0))?
                .ok_or_else(|| fail(format!("{key} is not set in {}", f.path().display())))?;
            out.line(&v);
            Ok(json!({"key": key, "value": v}))
        }
        ConfigCmd::Set {
            key,
            value,
            project,
        } => {
            let (mut f, is_project) = file(&project)?;
            if is_project && !f.path().exists() {
                return Err(fail(format!(
                    "{} does not exist; run `embed project init` first",
                    f.path().display()
                )));
            }
            f.set(&key, &value).map_err(|e| fail(e.0))?;
            f.save(check(is_project))
                .map_err(|e| fail(format!("Not saved: {}", e.0)))?;
            out.line(&format!(
                "{key} = {}",
                f.get(&key).ok().flatten().unwrap_or_default()
            ));
            Ok(json!({"file": f.path(), "key": key}))
        }
        ConfigCmd::Unset { key, project } => {
            let (mut f, is_project) = file(&project)?;
            let existed = f.unset(&key).map_err(|e| fail(e.0))?;
            if existed {
                f.save(check(is_project))
                    .map_err(|e| fail(format!("Not saved: {}", e.0)))?;
            }
            out.line(&format!(
                "{key} {}",
                if existed { "removed" } else { "was not set" }
            ));
            Ok(json!({"key": key, "removed": existed}))
        }
        ConfigCmd::Show { path } => {
            Ok(json!({"project": embedide_core::inspect(&path).map_err(|e| fail(e.0))?.metadata}))
        }
    }
}
