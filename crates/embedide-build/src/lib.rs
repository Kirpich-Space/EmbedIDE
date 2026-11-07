//! Build engine.
//!
//! [`prepare`] turns a project + profile + toolchain choice into a reviewable
//! [`Prepared`] plan (no process is started). [`run`] executes it through the
//! existing cancellable `embedide_core::process::ProcessTask`: no shell, real
//! exit codes, streamed output, process-group cancellation.
//!
//! Native projects are compiled file by file with the selected compilers and
//! rebuilt incrementally (compiler depfiles + a hash of each command).
//! Make/CMake/Cargo/Zig projects keep their own build systems.

mod native;
pub mod select;

use embedide_core::{
    diagnostics::{self, Diagnostic},
    process::{ProcessEvent, ProcessTask},
    BuildPlan, BuildSystem, ProcessSpec,
};
use embedide_toolchains::Toolchain;
use embedide_workspace::{BuildKind, Manifest, UserConfig};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fmt,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

pub use select::Selection;

/// Name of the marker file that makes an output directory removable by `clean`.
pub const OUTPUT_MARKER: &str = ".embedide-output";

/// Build error with an actionable message.
#[derive(Debug)]
pub struct Error(pub String);
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self(e.to_string())
    }
}
impl From<embedide_workspace::Error> for Error {
    fn from(e: embedide_workspace::Error) -> Self {
        Self(e.0)
    }
}
impl From<embedide_core::Error> for Error {
    fn from(e: embedide_core::Error) -> Self {
        Self(e.0)
    }
}
/// Result type of this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// What the caller asks for.
#[derive(Debug, Clone)]
pub struct Request {
    /// Profile name (`debug`, `release`, …).
    pub profile: String,
    /// Command-line toolchain overrides: language id → toolchain id.
    pub toolchain: BTreeMap<String, String>,
    /// Parallel jobs for external build systems.
    pub jobs: usize,
    /// Allow Cargo network access.
    pub online: bool,
}

impl Default for Request {
    fn default() -> Self {
        Self {
            profile: "debug".into(),
            toolchain: BTreeMap::new(),
            jobs: 2,
            online: false,
        }
    }
}

/// A reviewable build.
#[derive(Debug, Clone, Serialize)]
pub struct Prepared {
    /// Project root.
    pub root: PathBuf,
    /// Driver.
    pub system: BuildKind,
    /// Profile used.
    pub profile: String,
    /// Toolchains chosen per language (`c → gcc @ /usr/bin/gcc`).
    pub toolchains: Vec<Selection>,
    /// Steps in order.
    pub steps: Vec<ProcessSpec>,
    /// Extra environment for every step.
    pub env: BTreeMap<String, String>,
    /// Expected artifact, when known.
    pub artifact: Option<PathBuf>,
    /// Sources skipped because they are up to date (native builds).
    pub up_to_date: usize,
    /// Files written after a successful run (native command hashes).
    #[serde(skip)]
    pub(crate) stamps: Vec<(PathBuf, String)>,
    /// Output root that receives the clean marker (native builds).
    #[serde(skip)]
    pub(crate) output_root: Option<PathBuf>,
}

impl Prepared {
    /// Create the output directory and its clean marker. Call before running
    /// [`Prepared::steps`] with your own process runner; [`run`] does this itself.
    pub fn begin(&self) -> Result<()> {
        if let Some(out) = &self.output_root {
            std::fs::create_dir_all(out)?;
            let marker = out.join(OUTPUT_MARKER);
            if !marker.exists() {
                std::fs::write(
                    &marker,
                    "Created by EmbedIDE; `embed clean` may delete this directory.\n",
                )?;
            }
        }
        Ok(())
    }

    /// Record incremental-build stamps. Call only after every step exited with 0.
    pub fn commit(&self) {
        for (path, text) in &self.stamps {
            let _ = std::fs::write(path, text);
        }
    }

    /// The steps as a process plan for `embedide_core::process::ProcessTask`.
    pub fn plan(&self) -> BuildPlan {
        BuildPlan {
            directory: self.root.clone(),
            system: BuildSystem::Make,
            steps: self.steps.clone(),
        }
    }
}

/// Toolchains available to builds: PATH and configured search directories,
/// plus the user's custom entries. Nothing is installed or downloaded.
pub fn registry(user: &UserConfig, probe: bool) -> Vec<Toolchain> {
    embedide_toolchains::detect(&embedide_toolchains::DetectOptions {
        search_dirs: user.toolchains.search_dirs.clone(),
        custom: user.toolchains.custom.clone(),
        probe,
    })
}

/// Outcome of [`run`].
#[derive(Debug, Serialize)]
pub struct Outcome {
    /// True only when every step exited with status 0.
    pub success: bool,
    /// Exit code of the failing step (or 0); None when cancelled or not started.
    pub exit_code: Option<i32>,
    /// True when cancelled.
    pub cancelled: bool,
    /// Parsed compiler diagnostics.
    pub diagnostics: Vec<Diagnostic>,
    /// Artifact path, if the build succeeded and produced it.
    pub artifact: Option<PathBuf>,
    /// Wall time in milliseconds.
    pub elapsed_ms: u128,
    /// Output bytes dropped because the consumer was slow.
    pub dropped_bytes: u64,
}

/// Streamed while running.
pub enum Event<'a> {
    /// A step is about to start (program + args, already split).
    Command(&'a str),
    /// Output text; `true` for stderr.
    Output(&'a str, bool),
}

/// Plan a build. Nothing is executed except toolchain version probes.
pub fn prepare(
    root: &Path,
    manifest: Option<&Manifest>,
    user: &UserConfig,
    toolchains: &[Toolchain],
    req: &Request,
) -> Result<Prepared> {
    let root = root
        .canonicalize()
        .map_err(|e| Error(format!("Cannot open {}: {e}", root.display())))?;
    match manifest {
        Some(m) if m.build.system == BuildKind::Native => {
            native::prepare(&root, m, user, toolchains, req)
        }
        Some(m) => external(&root, Some(m), user, toolchains, req),
        None => external(&root, None, user, toolchains, req),
    }
}

fn external(
    root: &Path,
    m: Option<&Manifest>,
    user: &UserConfig,
    toolchains: &[Toolchain],
    req: &Request,
) -> Result<Prepared> {
    let project = embedide_core::Project::open(root)?;
    let system = m.map(|m| match m.build.system {
        BuildKind::Cargo => BuildSystem::Cargo,
        BuildKind::Make => BuildSystem::Make,
        BuildKind::Cmake => BuildSystem::Cmake,
        BuildKind::Zig => BuildSystem::Zig,
        BuildKind::Native => unreachable!(),
    });
    let release = req.profile == "release";
    let mut plan: BuildPlan = embedide_core::plan_build(
        &project,
        &embedide_core::BuildOptions {
            system,
            release: release && system != Some(BuildSystem::Make),
            jobs: req.jobs.max(1),
            online: req.online,
        },
    )?;
    let mut selections = Vec::new();
    let profile = m.and_then(|m| m.profiles.get(&req.profile));
    if let (Some(m), None) = (m, profile) {
        m.profile(&req.profile)?;
    }
    match plan.system {
        BuildSystem::Cargo => {
            if let Some(cp) = profile.and_then(|p| p.cargo_profile.clone()) {
                let args = &mut plan.steps[0].args;
                args.retain(|a| a != "--release");
                if cp != "dev" {
                    args.extend(["--profile".into(), cp]);
                }
            }
            if let Some(t) = m.and_then(|m| m.target.triple.clone()) {
                plan.steps[0].args.extend(["--target".into(), t]);
            }
        }
        BuildSystem::Make | BuildSystem::Cmake => {
            // Hand the user's compiler choice to the existing build system
            for lang in [
                embedide_toolchains::Language::C,
                embedide_toolchains::Language::Cpp,
                embedide_toolchains::Language::Fortran,
            ] {
                let explicit = select::explicit_choice(m, user, req, lang);
                let Some(id) = explicit else { continue };
                let sel = select::resolve(
                    toolchains,
                    &id,
                    lang,
                    m.and_then(|m| m.target.triple.as_deref()),
                )?;
                if plan.system == BuildSystem::Make {
                    let var = match lang {
                        embedide_toolchains::Language::C => "CC",
                        embedide_toolchains::Language::Cpp => "CXX",
                        _ => "FC",
                    };
                    let mut value = sel.program.to_string_lossy().into_owned();
                    if value.contains(char::is_whitespace) {
                        return Err(Error(format!(
                            "Make splits variables on spaces, so {var}='{value}' would break. Put the compiler directory on PATH (and select it by name) or use a path without spaces"
                        )));
                    }
                    for a in &sel.prefix_args {
                        value.push(' ');
                        value.push_str(a);
                    }
                    plan.steps[0].args.push(format!("{var}={value}"));
                } else {
                    let var = match lang {
                        embedide_toolchains::Language::C => "C",
                        embedide_toolchains::Language::Cpp => "CXX",
                        _ => "Fortran",
                    };
                    plan.steps[0]
                        .args
                        .push(format!("-DCMAKE_{var}_COMPILER={}", sel.program.display()));
                    if let Some(first) = sel.prefix_args.first() {
                        plan.steps[0]
                            .args
                            .push(format!("-DCMAKE_{var}_COMPILER_ARG1={first}"));
                    }
                }
                selections.push(sel);
            }
        }
        BuildSystem::Zig => {}
    }
    Ok(Prepared {
        root: root.to_path_buf(),
        system: match plan.system {
            BuildSystem::Cargo => BuildKind::Cargo,
            BuildSystem::Make => BuildKind::Make,
            BuildSystem::Cmake => BuildKind::Cmake,
            BuildSystem::Zig => BuildKind::Zig,
        },
        profile: req.profile.clone(),
        toolchains: selections,
        steps: plan.steps,
        env: m.map(|m| m.env.clone()).unwrap_or_default(),
        artifact: None,
        up_to_date: 0,
        stamps: Vec::new(),
        output_root: None,
    })
}

/// Cancellation handle shared with a signal handler or UI.
#[derive(Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);
impl Cancel {
    /// Request cancellation.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release)
    }
    /// Whether cancellation was requested.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Run a prepared build, streaming events. Returns after the last step.
pub fn run(
    prepared: &Prepared,
    cancel: &Cancel,
    mut on_event: impl FnMut(Event<'_>),
) -> Result<Outcome> {
    let started = std::time::Instant::now();
    prepared.begin()?;
    if prepared.steps.is_empty() {
        return Ok(Outcome {
            success: true,
            exit_code: Some(0),
            cancelled: false,
            diagnostics: Vec::new(),
            artifact: prepared.artifact.clone().filter(|a| a.exists()),
            elapsed_ms: started.elapsed().as_millis(),
            dropped_bytes: 0,
        });
    }
    let task = ProcessTask::start(prepared.plan(), prepared.env.clone())?;
    let mut diags = Vec::new();
    let mut pending = [String::new(), String::new()];
    let mut finished = None;
    while finished.is_none() {
        if cancel.is_cancelled() {
            task.cancel();
        }
        for ev in task.drain() {
            match ev {
                ProcessEvent::Command(c) => on_event(Event::Command(&c)),
                ProcessEvent::Output(text, err) => {
                    on_event(Event::Output(&text, err));
                    let buf = &mut pending[err as usize];
                    buf.push_str(&text);
                    while let Some(nl) = buf.find('\n') {
                        let line: String = buf.drain(..=nl).collect();
                        if let Some(d) = diagnostics::parse(line.trim_end()) {
                            diags.push(d);
                        }
                    }
                }
                ProcessEvent::Finished(code, _) => finished = Some(code),
            }
        }
        if finished.is_none() {
            std::thread::sleep(Duration::from_millis(15));
        }
    }
    for buf in &pending {
        if let Some(d) = diagnostics::parse(buf.trim_end()) {
            diags.push(d);
        }
    }
    let code = finished.flatten();
    let success = code == Some(0);
    if success {
        prepared.commit();
    }
    Ok(Outcome {
        success,
        exit_code: code,
        cancelled: cancel.is_cancelled(),
        diagnostics: diags,
        artifact: if success {
            prepared.artifact.clone().filter(|a| a.exists())
        } else {
            None
        },
        elapsed_ms: started.elapsed().as_millis(),
        dropped_bytes: task.dropped_bytes(),
    })
}

/// What `clean` would remove or run.
#[derive(Debug, Serialize)]
pub enum CleanAction {
    /// Remove an EmbedIDE-owned output directory.
    Remove(PathBuf),
    /// Run the build system's own clean command.
    Run(ProcessSpec),
    /// Nothing to do.
    Nothing(String),
}

/// Decide how to clean. Only directories carrying [`OUTPUT_MARKER`] inside
/// the project root are ever deleted; other systems use their own command.
pub fn plan_clean(root: &Path, manifest: Option<&Manifest>) -> Result<CleanAction> {
    let root = root.canonicalize()?;
    let system = manifest.map(|m| m.build.system);
    if matches!(system, Some(BuildKind::Native) | Some(BuildKind::Cmake))
        || (system.is_none() && root.join(".embedide-build").exists())
    {
        let out = match manifest {
            Some(m) if m.build.system == BuildKind::Native => root.join(&m.build.output),
            _ => root.join(".embedide-build"),
        };
        if !out.exists() {
            return Ok(CleanAction::Nothing(format!(
                "{} does not exist",
                out.display()
            )));
        }
        let canon = out.canonicalize()?;
        if canon == root || !canon.starts_with(&root) {
            return Err(Error(format!(
                "Refusing to delete {}: not a subdirectory of the project",
                canon.display()
            )));
        }
        let is_cmake = !matches!(system, Some(BuildKind::Native));
        if !is_cmake && !canon.join(OUTPUT_MARKER).is_file() {
            return Err(Error(format!(
                "Refusing to delete {}: it was not created by EmbedIDE (no {OUTPUT_MARKER}). Remove it yourself if intended",
                canon.display()
            )));
        }
        if is_cmake && !canon.join("CMakeCache.txt").is_file() {
            return Err(Error(format!(
                "Refusing to delete {}: it does not look like a CMake build directory",
                canon.display()
            )));
        }
        return Ok(CleanAction::Remove(canon));
    }
    let detected = embedide_core::Project::open(&root).ok();
    let has = |s: BuildSystem| {
        detected
            .as_ref()
            .is_some_and(|p| p.detected_build_systems.contains(&s))
    };
    Ok(match system {
        Some(BuildKind::Cargo) => CleanAction::Run(ProcessSpec {
            program: "cargo".into(),
            args: vec!["clean".into()],
        }),
        Some(BuildKind::Make) => CleanAction::Run(ProcessSpec {
            program: "make".into(),
            args: vec!["clean".into()],
        }),
        None if has(BuildSystem::Cargo) => CleanAction::Run(ProcessSpec {
            program: "cargo".into(),
            args: vec!["clean".into()],
        }),
        None if has(BuildSystem::Make) => CleanAction::Run(ProcessSpec {
            program: "make".into(),
            args: vec!["clean".into()],
        }),
        _ => CleanAction::Nothing(
            "Zig projects: remove zig-out/ and .zig-cache/ yourself if needed".into(),
        ),
    })
}
