//! Non-destructive project discovery and legacy `embedide.json` compatibility.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fmt, fs,
    io::Write,
    path::{Path, PathBuf},
    str::FromStr,
};

/// An actionable service error; frontends choose how to display it.
#[derive(Debug)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self(value.to_string())
    }
}
impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Self(value.to_string())
    }
}

/// Result returned by project and core services.
pub type Result<T> = std::result::Result<T, Error>;

/// A build system with a direct executable adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BuildSystem {
    /// Cargo (`Cargo.toml`).
    Cargo,
    /// GNU-compatible Make (`GNUmakefile`, `makefile`, `Makefile`).
    Make,
    /// CMake (`CMakeLists.txt`).
    Cmake,
    /// Zig (`build.zig`).
    Zig,
}

impl fmt::Display for BuildSystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Cargo => "cargo",
            Self::Make => "make",
            Self::Cmake => "cmake",
            Self::Zig => "zig",
        })
    }
}
impl FromStr for BuildSystem {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "cargo" => Ok(Self::Cargo),
            "make" => Ok(Self::Make),
            "cmake" => Ok(Self::Cmake),
            "zig" => Ok(Self::Zig),
            _ => Err(Error(format!(
                "Unknown build system '{s}'; expected cargo, make, cmake or zig"
            ))),
        }
    }
}

/// An explicitly opened directory. Opening never executes code or changes files.
#[derive(Debug, Serialize)]
pub struct Project {
    /// Canonical project directory.
    pub root: PathBuf,
    /// Unmodified metadata, including fields unknown to this version.
    pub metadata: Option<Value>,
    /// Build systems detected at the project root.
    pub detected_build_systems: Vec<BuildSystem>,
}

impl Project {
    /// Read metadata and inspect manifests in exactly this directory.
    pub fn open(path: &Path) -> Result<Self> {
        let root = path
            .canonicalize()
            .map_err(|e| Error(format!("Cannot open {}: {e}", path.display())))?;
        if !root.is_dir() {
            return Err(Error(format!("Not a directory: {}", root.display())));
        }
        let metadata_path = root.join("embedide.json");
        let metadata = match fs::read(&metadata_path) {
            Ok(bytes) => {
                let value: Value = serde_json::from_slice(&bytes)
                    .map_err(|e| Error(format!("Invalid {}: {e}", metadata_path.display())))?;
                let object = value
                    .as_object()
                    .ok_or_else(|| Error("embedide.json must contain an object".into()))?;
                if let Some(version) = object.get("version") {
                    if version.as_u64() != Some(1) {
                        return Err(Error(
                            "Unsupported project metadata version; expected 1".into(),
                        ));
                    }
                }
                if let Some(system) = object.get("buildSystem") {
                    system
                        .as_str()
                        .ok_or_else(|| Error("buildSystem must be a string".into()))?
                        .parse::<BuildSystem>()?;
                }
                Some(value)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                return Err(Error(format!(
                    "Cannot read {}: {e}",
                    metadata_path.display()
                )))
            }
        };
        let mut detected_build_systems = Vec::new();
        for (system, manifests) in [
            (BuildSystem::Cargo, &["Cargo.toml"][..]),
            (
                BuildSystem::Make,
                &["GNUmakefile", "makefile", "Makefile"][..],
            ),
            (BuildSystem::Cmake, &["CMakeLists.txt"][..]),
            (BuildSystem::Zig, &["build.zig"][..]),
        ] {
            if manifests.iter().any(|name| root.join(name).is_file()) {
                detected_build_systems.push(system);
            }
        }
        if metadata.is_none() && detected_build_systems.is_empty() {
            return Err(Error(format!(
                "No embedide.json or supported build manifest in {}; run `embed project init` (it can adopt an existing directory without touching its files)",
                root.display()
            )));
        }
        Ok(Self {
            root,
            metadata,
            detected_build_systems,
        })
    }

    /// Select a build system, refusing to guess when multiple manifests exist.
    pub fn build_system(&self, override_system: Option<BuildSystem>) -> Result<BuildSystem> {
        let configured = self
            .metadata
            .as_ref()
            .and_then(|v| v.get("buildSystem"))
            .and_then(Value::as_str)
            .map(str::parse)
            .transpose()?;
        let selected = override_system.or(configured);
        if let Some(system) = selected {
            if !self.detected_build_systems.contains(&system) {
                return Err(Error(format!(
                    "No {system} manifest in {}",
                    self.root.display()
                )));
            }
            return Ok(system);
        }
        match self.detected_build_systems.as_slice() {
            [system] => Ok(*system),
            [] => Err(Error("No supported build manifest; add Cargo.toml, Makefile, CMakeLists.txt or build.zig".into())),
            _ => Err(Error("Multiple build systems detected; select --build-system or set buildSystem in embedide.json".into())),
        }
    }
}

/// Register a directory without changing sources, manifests or existing metadata.
/// Uses exclusive creation so a concurrent init cannot overwrite another file.
pub fn init(path: &Path, build_system: Option<BuildSystem>) -> Result<Project> {
    fs::create_dir_all(path)?;
    let root = path.canonicalize()?;
    let name = root.file_name().unwrap_or_default().to_string_lossy();
    let mut metadata = json!({"name": name, "version": 1});
    if let Some(system) = build_system {
        metadata["buildSystem"] = json!(system);
    }
    let bytes = serde_json::to_vec_pretty(&metadata)?;
    let target = root.join("embedide.json");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .map_err(|e| {
            Error(format!(
                "Cannot create {} (existing files are never overwritten): {e}",
                target.display()
            ))
        })?;
    file.write_all(&bytes)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Project::open(&root)
}
