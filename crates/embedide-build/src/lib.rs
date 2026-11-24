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

mod fortran;
pub mod makefile;
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
    /// Output subdirectories created by [`Prepared::begin`] right before execution.
    #[serde(skip)]
    pub(crate) dirs: Vec<PathBuf>,
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
        for dir in &self.dirs {
            std::fs::create_dir_all(dir)?;
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

/// Toolchains available to builds: PATH, configured search directories, toolchains EmbedIDE has
/// installed for the user (see [`ensure_required`]), plus the user's custom entries. This function
/// never installs or downloads anything (see [`ensure_required`] for that).
pub fn registry(user: &UserConfig, probe: bool) -> Vec<Toolchain> {
    let mut search_dirs = user.toolchains.search_dirs.clone();
    let specs = embedide_toolchains::install::merged(&user.toolchains.install);
    for d in embedide_toolchains::install::managed_search_dirs(&specs) {
        if !search_dirs.contains(&d) {
            search_dirs.push(d);
        }
    }
    embedide_toolchains::detect(&embedide_toolchains::DetectOptions {
        search_dirs,
        custom: user.toolchains.custom.clone(),
        probe,
    })
}

/// Names of the downloadable toolchains this build needs: the user's `toolchains.require` plus the
/// project's `[install] toolchains`, without duplicates.
pub fn required_names(user: &UserConfig, manifest: Option<&Manifest>) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for n in user
        .toolchains
        .require
        .iter()
        .chain(manifest.iter().flat_map(|m| m.install.toolchains.iter()))
    {
        if !names.contains(n) {
            names.push(n.clone());
        }
    }
    names
}

/// Names of required toolchains that are not available yet (no network access). A front end can call
/// this to decide whether to run [`ensure_required`] off its main thread.
pub fn missing_installs(user: &UserConfig, manifest: Option<&Manifest>) -> Result<Vec<String>> {
    let names = required_names(user, manifest);
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let specs = embedide_toolchains::install::merged(&user.toolchains.install);
    let detected = registry(user, false);
    let ids: Vec<&str> = detected.iter().map(|t| t.id.as_str()).collect();
    let todo = embedide_toolchains::install::missing(&names, &specs, &ids).map_err(Error)?;
    Ok(todo.into_iter().map(|(n, _)| n.to_string()).collect())
}

/// Download and unpack the required toolchains that are neither installed by EmbedIDE nor already on
/// the machine. Only entries from the user's `config.toml` (or the built-in, checksum-pinned ones) can
/// be fetched, each verified against its SHA-256; see `embedide_toolchains::install`. Returns the names
/// that were installed. `progress` receives status lines. Does nothing when `toolchains.auto_install` is
/// off (it then fails with a hint) or when nothing is required.
pub fn ensure_required(
    user: &UserConfig,
    manifest: Option<&Manifest>,
    progress: &mut dyn FnMut(&str),
) -> Result<Vec<String>> {
    let names = required_names(user, manifest);
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let specs = embedide_toolchains::install::merged(&user.toolchains.install);
    let detected = registry(user, false);
    let ids: Vec<&str> = detected.iter().map(|t| t.id.as_str()).collect();
    let todo = embedide_toolchains::install::missing(&names, &specs, &ids).map_err(Error)?;
    if todo.is_empty() {
        return Ok(Vec::new());
    }
    if !user.toolchains.auto_install {
        return Err(Error(format!(
            "Toolchains not installed: {}. Run `embed toolchain install` (automatic downloads are off: toolchains.auto_install = false).",
            todo.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")
        )));
    }
    let mut installed = Vec::new();
    for (name, spec) in todo {
        embedide_toolchains::install::install(name, &spec, progress).map_err(Error)?;
        installed.push(name.to_string());
    }
    Ok(installed)
}

/// Missing toolchains discovered when opening a workspace in the IDE.
#[derive(Debug, Default, Clone)]
pub struct StartupPlan {
    /// Pinned downloadable archives (`name`, `spec`) ready for automatic background installation.
    pub downloadable: Vec<(String, embedide_toolchains::InstallSpec)>,
    /// Project languages with no installed compiler and no pinned archive spec, paired with the
    /// host OS package-manager command (if known).
    pub os_hints: Vec<(embedide_toolchains::Language, Option<String>)>,
    /// Configuration error in required toolchain specs, if any.
    pub error: Option<String>,
}

fn builtin_matches_host(
    name: &str,
    user_specs: &BTreeMap<String, embedide_toolchains::InstallSpec>,
) -> bool {
    if user_specs.contains_key(name) {
        return true;
    }
    name != "zig" || cfg!(all(target_os = "linux", target_arch = "x86_64"))
}

/// Inspect what toolchains are missing at IDE startup (pure filesystem check; never touches the
/// network or spawns compiler version probes).
pub fn startup_install_plan(user: &UserConfig, manifest: Option<&Manifest>) -> StartupPlan {
    let mut plan = StartupPlan::default();
    let detected = registry(user, false);
    let ids: Vec<&str> = detected.iter().map(|t| t.id.as_str()).collect();
    let specs = embedide_toolchains::install::merged(&user.toolchains.install);
    let explicit = required_names(user, manifest);
    if !explicit.is_empty() {
        match embedide_toolchains::install::missing(&explicit, &specs, &ids) {
            Ok(todo) => {
                for (name, spec) in todo {
                    plan.downloadable.push((name.to_string(), spec));
                }
            }
            Err(e) => plan.error = Some(e),
        }
    }
    // Check every compiler explicitly chosen in user `[toolchains.defaults]` or project
    // `[toolchain]` / `[profiles.*.toolchain]`: if not on disk, auto-download from matching
    // `[toolchains.install.*]` (or built-in) spec, or record an OS package hint.
    let mut configured_choices: Vec<(&str, &str)> = user
        .toolchains
        .defaults
        .iter()
        .map(|(l, id)| (l.as_str(), id.as_str()))
        .collect();
    if let Some(m) = manifest {
        for (l, id) in &m.toolchain {
            configured_choices.push((l.as_str(), id.as_str()));
        }
        for profile in m.profiles.values() {
            for (l, id) in &profile.toolchain {
                configured_choices.push((l.as_str(), id.as_str()));
            }
        }
    }
    let family = embedide_toolchains::package_family();
    for (lang_str, chosen_id) in configured_choices {
        if ids.contains(&chosen_id) {
            continue;
        }
        let candidate = specs.iter().find(|(name, spec)| {
            builtin_matches_host(name, &user.toolchains.install)
                && !embedide_toolchains::install::is_installed(name, spec)
                && (name.as_str() == chosen_id || spec.provides.iter().any(|p| p == chosen_id))
        });
        if let Some((name, spec)) = candidate {
            if !plan.downloadable.iter().any(|(n, _)| n == name) {
                plan.downloadable.push((name.clone(), spec.clone()));
            }
        } else if let Ok(lang) = lang_str.parse::<embedide_toolchains::Language>() {
            if !plan.os_hints.iter().any(|(l, _)| *l == lang) {
                let hint = embedide_toolchains::recommend(lang, family)
                    .into_iter()
                    .find(|r| r.kind == chosen_id)
                    .and_then(|r| r.command)
                    .or_else(|| {
                        embedide_toolchains::recommend(lang, family)
                            .into_iter()
                            .find_map(|r| r.command)
                    });
                plan.os_hints.push((lang, hint));
            }
        }
    }
    if let Some(m) = manifest {
        if let Ok(lang) = m.language() {
            let have_compiler = detected
                .iter()
                .any(|t| t.supports(lang) && t.problem.is_none());
            if !have_compiler {
                let candidate = specs.iter().find(|(name, spec)| {
                    builtin_matches_host(name, &user.toolchains.install)
                        && !embedide_toolchains::install::is_installed(name, spec)
                        && spec.provides.iter().any(|p| {
                            embedide_toolchains::kind(p)
                                .is_some_and(|k| k.languages.contains(&lang))
                        })
                });
                if let Some((name, spec)) = candidate {
                    if !plan.downloadable.iter().any(|(n, _)| n == name) {
                        plan.downloadable.push((name.clone(), spec.clone()));
                    }
                } else if !plan.os_hints.iter().any(|(l, _)| *l == lang) {
                    let hint = embedide_toolchains::recommend(lang, family)
                        .into_iter()
                        .find_map(|r| r.command);
                    plan.os_hints.push((lang, hint));
                }
            }
        }
    }
    plan
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
        dirs: Vec::new(),
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
    let mut parsers = [
        diagnostics::Parser::default(),
        diagnostics::Parser::default(),
    ];
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
                        if let Some(d) = parsers[err as usize].feed(line.trim_end()) {
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
    for (buf, parser) in pending.iter().zip(parsers.iter_mut()) {
        if let Some(d) = parser.feed(buf.trim_end()) {
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
            program: embedide_core::make_program().into(),
            args: vec!["clean".into()],
        }),
        None if has(BuildSystem::Cargo) => CleanAction::Run(ProcessSpec {
            program: "cargo".into(),
            args: vec!["clean".into()],
        }),
        None if has(BuildSystem::Make) => CleanAction::Run(ProcessSpec {
            program: embedide_core::make_program().into(),
            args: vec!["clean".into()],
        }),
        _ => CleanAction::Nothing(
            "Zig projects: remove zig-out/ and .zig-cache/ yourself if needed".into(),
        ),
    })
}

#[cfg(test)]
mod startup_install_tests {
    use super::*;

    #[test]
    fn startup_plan_queues_missing_configured_spec_and_reports_unknown_names() {
        let mut user = UserConfig::default();
        user.toolchains.require = vec!["custom-tc".into()];
        user.toolchains.install.insert(
            "custom-tc".into(),
            embedide_toolchains::InstallSpec {
                url: "https://example.org/custom-tc.tar.xz".into(),
                sha256: "a".repeat(64),
                archive: None,
                strip: 1,
                bin: vec!["bin".into()],
                provides: vec!["nonexistent-compiler-id".into()],
            },
        );
        let plan = startup_install_plan(&user, None);
        assert!(plan.error.is_none());
        assert_eq!(plan.downloadable.len(), 1);
        assert_eq!(plan.downloadable[0].0, "custom-tc");

        let mut bad_user = UserConfig::default();
        bad_user.toolchains.require = vec!["no-such-spec".into()];
        let bad_plan = startup_install_plan(&bad_user, None);
        assert!(bad_plan.error.is_some());

        // Choosing a missing compiler in `[toolchains.defaults]` automatically queues its pinned spec.
        let mut chosen_user = UserConfig::default();
        chosen_user
            .toolchains
            .defaults
            .insert("c".into(), "missing-custom-cc".into());
        chosen_user.toolchains.install.insert(
            "my-cc".into(),
            embedide_toolchains::InstallSpec {
                url: "https://example.org/my-cc.tar.xz".into(),
                sha256: "b".repeat(64),
                archive: None,
                strip: 1,
                bin: vec!["bin".into()],
                provides: vec!["missing-custom-cc".into()],
            },
        );
        let chosen_plan = startup_install_plan(&chosen_user, None);
        assert!(chosen_plan.error.is_none());
        assert_eq!(chosen_plan.downloadable.len(), 1);
        assert_eq!(chosen_plan.downloadable[0].0, "my-cc");
    }
}
