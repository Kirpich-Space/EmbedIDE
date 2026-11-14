//! Headless application services shared by CLI and future native GUI.

#[cfg(not(unix))]
compile_error!(
    "EmbedIDE supports Linux and FreeBSD only (Unix systems); Windows is not supported."
);

pub mod binary;
pub mod config;
pub mod debug;
pub mod diagnostics;
pub mod disasm;
pub mod elf;
pub mod explorer;
pub mod hardware;
pub mod lsp;
pub mod process;

pub use embedide_editor as editor;
pub use embedide_editor::{Editor, EditorError, EditorState, Effect};
pub use embedide_project::{init, make_program, BuildSystem, Error, Project, Result};
use serde::Serialize;
use std::{
    env,
    ffi::OsStr,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// One executable invocation. Arguments are never interpreted by a shell.
#[derive(Debug, Clone, Serialize)]
pub struct ProcessSpec {
    /// Program resolved through the host PATH when executed.
    pub program: String,
    /// Individual process arguments.
    pub args: Vec<String>,
}

/// A reviewable build plan. Creating a plan never starts a process.
#[derive(Debug, Clone, Serialize)]
pub struct BuildPlan {
    /// Working directory shared by all steps.
    pub directory: PathBuf,
    /// Selected manifest adapter.
    pub system: BuildSystem,
    /// Steps run in order; a failure prevents subsequent steps.
    pub steps: Vec<ProcessSpec>,
}

/// Explicit options independent of any frontend.
pub struct BuildOptions {
    /// Override ambiguous or configured build-system selection.
    pub system: Option<BuildSystem>,
    /// Use release rather than debug configuration.
    pub release: bool,
    /// Maximum build parallelism (must be positive).
    pub jobs: usize,
    /// Permit Cargo to access the network. Other tools retain their own policies.
    pub online: bool,
}

/// Plan Cargo, Make, CMake or Zig builds from actual project manifests.
pub fn plan_build(project: &Project, options: &BuildOptions) -> Result<BuildPlan> {
    if options.jobs == 0 {
        return Err(Error("Build jobs must be greater than zero".into()));
    }
    let system = project.build_system(options.system)?;
    let jobs = options.jobs.to_string();
    let profile = if options.release { "Release" } else { "Debug" };
    let step = |program: &str, args: Vec<String>| ProcessSpec {
        program: program.into(),
        args,
    };
    let strings = |args: &[&str]| args.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let steps = match system {
        BuildSystem::Cargo => {
            let mut args = strings(&["build", "--jobs", &jobs]);
            if !options.online {
                args.push("--offline".into());
            }
            if options.release {
                args.push("--release".into());
            }
            vec![step("cargo", args)]
        }
        BuildSystem::Make => {
            if options.release {
                return Err(Error("Make configuration is project-defined; --release is supported for cargo, cmake and zig".into()));
            }
            vec![step(make_program(), strings(&["-j", &jobs]))]
        }
        BuildSystem::Cmake => vec![
            step(
                "cmake",
                strings(&[
                    "-S",
                    ".",
                    "-B",
                    ".embedide-build",
                    &format!("-DCMAKE_BUILD_TYPE={profile}"),
                ]),
            ),
            step(
                "cmake",
                strings(&[
                    "--build",
                    ".embedide-build",
                    "--config",
                    profile,
                    "--parallel",
                    &jobs,
                ]),
            ),
        ],
        BuildSystem::Zig => vec![step(
            "zig",
            strings(&[
                "build",
                &format!("-j{jobs}"),
                if options.release {
                    "-Doptimize=ReleaseSafe"
                } else {
                    "-Doptimize=Debug"
                },
            ]),
        )],
    };
    Ok(BuildPlan {
        directory: project.root.clone(),
        system,
        steps,
    })
}

/// Run a build synchronously for a terminal frontend, streaming all child output
/// to stderr so stdout remains available for a single JSON result. A GUI must
/// use a worker with a future cancellable task adapter, never its event thread.
/// Build scripts are executable project code and must be trusted by the caller.
pub fn run_build(plan: &BuildPlan) -> Result<i32> {
    for step in &plan.steps {
        let status = Command::new(&step.program)
            .args(&step.args)
            .current_dir(&plan.directory)
            .stdin(Stdio::null())
            .stdout(std::io::stderr())
            .stderr(Stdio::inherit())
            .status()
            .map_err(|e| {
                Error(format!(
                    "Cannot run {} in {}: {e}",
                    step.program,
                    plan.directory.display()
                ))
            })?;
        if !status.success() {
            return Ok(status.code().filter(|c| (1..=255).contains(c)).unwrap_or(1));
        }
    }
    Ok(0)
}

/// PATH inventory entry; presence does not prove that a tool runs correctly.
#[derive(Debug, Serialize)]
pub struct ToolStatus {
    /// Executable name.
    pub name: &'static str,
    /// Absolute location, if found. No tool is executed by inventory.
    pub path: Option<PathBuf>,
}

/// Find supported host and embedded tools without starting background processes.
pub fn toolchains() -> Vec<ToolStatus> {
    let path = env::var_os("PATH").unwrap_or_default();
    [
        "rg",
        "probe-rs",
        "gdb-multiarch",
        "arduino-cli",
        "pio",
        "esptool",
        "ninja",
        "rustc",
        "cargo",
        "gcc",
        "g++",
        "clang",
        "clang++",
        "zig",
        "cmake",
        "make",
        "gmake",
        "arm-none-eabi-gcc",
        "arm-none-eabi-gdb",
        "gdb",
        "openocd",
        "clangd",
        "rust-analyzer",
    ]
    .into_iter()
    .map(|name| ToolStatus {
        name,
        path: find_executable(name, &path),
    })
    .collect()
}

fn find_executable(name: &str, path: &OsStr) -> Option<PathBuf> {
    for directory in env::split_paths(path).filter(|p| p.is_absolute()) {
        let candidate = directory.join(name);
        if !candidate.is_file() {
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if candidate.metadata().ok()?.permissions().mode() & 0o111 == 0 {
                continue;
            }
        }
        return Some(candidate);
    }
    None
}

/// Read the preserved legacy MCU catalog. Entries are migration data, not
/// hardware validation or a restriction on what new project targets may use.
pub fn targets() -> Result<serde_json::Value> {
    Ok(serde_json::from_str(include_str!(
        "../data/legacy-targets.json"
    ))?)
}

/// Build plan helper for frontends that already know a directory.
pub fn inspect(path: &Path) -> Result<Project> {
    Project::open(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_has_unique_ids_and_keeps_original_default() {
        let catalog = targets().unwrap();
        let boards = catalog.as_array().unwrap();
        assert_eq!(boards.len(), 67);
        let ids: std::collections::BTreeSet<_> =
            boards.iter().map(|b| b["id"].as_str().unwrap()).collect();
        assert_eq!(ids.len(), boards.len());
        assert!(ids.contains("stm32f407vg"));
    }
    #[test]
    fn inventory_does_not_search_current_directory() {
        assert!(find_executable("Cargo.toml", OsStr::new(".:")).is_none());
    }
}
