//! `embedide.toml` — the versioned project manifest.
//!
//! ```toml
//! schema = 1
//!
//! [project]
//! name = "demo"
//! language = "c"
//!
//! [build]
//! system = "native"        # native | make | cmake | cargo | zig
//! sources = ["src"]
//! include = ["include"]
//! output = "build"
//!
//! [toolchain]              # one toolchain id per language
//! c = "gcc"                # or "clang", "zig-cc", "arm-none-eabi-gcc", a custom id…
//!
//! [profiles.debug]
//! flags = ["-O0", "-g"]
//!
//! [profiles.clang-release] # several configurations of the same project
//! flags = ["-O2"]
//! toolchain = { c = "clang" }
//! ```
use crate::{Error, Result};
use embedide_toolchains::Language;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};

/// File name of the manifest.
pub const MANIFEST_FILE: &str = "embedide.toml";

/// How a project is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BuildKind {
    /// EmbedIDE compiles sources itself with the selected toolchains.
    Native,
    /// Existing Makefile.
    Make,
    /// Existing CMakeLists.txt.
    Cmake,
    /// Existing Cargo.toml.
    Cargo,
    /// Existing build.zig.
    Zig,
}

/// `[project]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectSection {
    /// Project name; also the default artifact name.
    pub name: String,
    /// Primary language.
    pub language: String,
    /// Free text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// `[build]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct BuildSection {
    /// Build driver.
    pub system: BuildKind,
    /// Source directories (recursive), relative to the project root.
    pub sources: Vec<String>,
    /// Include directories.
    pub include: Vec<String>,
    /// Output root, relative to the project; `<output>/<profile>/…`.
    pub output: String,
    /// Artifact file name (defaults to the project name).
    pub artifact: Option<String>,
    /// Preprocessor defines (`NAME` or `NAME=VALUE`).
    pub defines: Vec<String>,
    /// Flags for every C/C++/ASM/Fortran compile.
    pub flags: Vec<String>,
    /// Extra link flags.
    pub link_flags: Vec<String>,
    /// Libraries to link (`m` → `-lm`).
    pub libs: Vec<String>,
    /// Linker script (firmware), relative to the project.
    pub linker_script: Option<String>,
}

impl Default for BuildSection {
    fn default() -> Self {
        Self {
            system: BuildKind::Native,
            sources: vec!["src".into()],
            include: Vec::new(),
            output: "build".into(),
            artifact: None,
            defines: Vec::new(),
            flags: Vec::new(),
            link_flags: Vec::new(),
            libs: Vec::new(),
            linker_script: None,
        }
    }
}

/// `[target]` — what the code runs on, independent of the compiler.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct TargetSection {
    /// Target triple passed as `--target`/`-target` to Clang, Zig and rustc.
    pub triple: Option<String>,
    /// Architecture flags (`-mcpu=cortex-m4 -mthumb …`) for compile and link.
    pub flags: Vec<String>,
    /// Board id from the catalog, informational.
    pub board: Option<String>,
}

/// One build configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Profile {
    /// Compile flags added for this profile.
    pub flags: Vec<String>,
    /// Defines added for this profile.
    pub defines: Vec<String>,
    /// Link flags added for this profile.
    pub link_flags: Vec<String>,
    /// Per-language toolchain overrides for this profile.
    pub toolchain: BTreeMap<String, String>,
    /// Cargo profile name for cargo projects (`dev`, `release`, custom).
    pub cargo_profile: Option<String>,
}

/// `[ai]` — what may be sent to an AI provider.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct AiSection {
    /// Glob-like patterns (`*`, `**`) of files never sent as context.
    pub exclude: Vec<String>,
}

/// The whole manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Schema version; 1.
    pub schema: u32,
    /// `[project]`
    pub project: ProjectSection,
    /// `[build]`
    #[serde(default)]
    pub build: BuildSection,
    /// `[toolchain]`: language id → toolchain id.
    #[serde(default)]
    pub toolchain: BTreeMap<String, String>,
    /// `[target]`
    #[serde(default)]
    pub target: TargetSection,
    /// `[profiles.<name>]`
    #[serde(default)]
    pub profiles: BTreeMap<String, Profile>,
    /// Environment for builds and `embedide terminal`.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// `[ai]`
    #[serde(default)]
    pub ai: AiSection,
    /// `[debug]` — kept verbatim for debugger front ends; never auto-started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debug: Option<toml::Table>,
}

fn relative_inside(field: &str, p: &str) -> Result<()> {
    let path = Path::new(p);
    if p.is_empty()
        || path.is_absolute()
        || path.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::Prefix(_) | Component::RootDir
            )
        })
    {
        return Err(Error(format!(
            "{field} must be a relative path inside the project, got '{p}'"
        )));
    }
    Ok(())
}

impl Manifest {
    /// A new manifest with debug and release profiles.
    pub fn new(name: &str, language: Language, system: BuildKind) -> Self {
        let mut profiles = BTreeMap::new();
        profiles.insert(
            "debug".into(),
            Profile {
                flags: vec!["-O0".into(), "-g".into()],
                cargo_profile: Some("dev".into()),
                ..Default::default()
            },
        );
        profiles.insert(
            "release".into(),
            Profile {
                flags: vec!["-O2".into()],
                defines: vec!["NDEBUG".into()],
                cargo_profile: Some("release".into()),
                ..Default::default()
            },
        );
        Self {
            schema: 1,
            project: ProjectSection {
                name: name.into(),
                language: language.id().into(),
                description: None,
            },
            build: BuildSection {
                system,
                ..Default::default()
            },
            toolchain: BTreeMap::new(),
            target: TargetSection::default(),
            profiles,
            env: BTreeMap::new(),
            ai: AiSection::default(),
            debug: None,
        }
    }

    /// Parse and validate manifest text.
    pub fn parse(text: &str) -> Result<Self> {
        let m: Self =
            toml::from_str(text).map_err(|e| Error(format!("Invalid {MANIFEST_FILE}: {e}")))?;
        m.validate()?;
        Ok(m)
    }

    /// Read `<root>/embedide.toml`; `Ok(None)` when absent.
    pub fn load(root: &Path) -> Result<Option<Self>> {
        match std::fs::read_to_string(root.join(MANIFEST_FILE)) {
            Ok(text) => Self::parse(&text).map(Some),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Error(format!(
                "Cannot read {}: {e}",
                root.join(MANIFEST_FILE).display()
            ))),
        }
    }

    /// Check structure and path safety.
    pub fn validate(&self) -> Result<()> {
        if self.schema != 1 {
            return Err(Error(format!(
                "Unsupported {MANIFEST_FILE} schema {}; this EmbedIDE reads schema 1",
                self.schema
            )));
        }
        let n = &self.project.name;
        if n.is_empty()
            || !n
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
            || n.starts_with('.')
        {
            return Err(Error(
                "project.name must use letters, digits, '-', '_' or '.'".into(),
            ));
        }
        self.language()?;
        for (lang, _) in self
            .toolchain
            .iter()
            .chain(self.profiles.values().flat_map(|p| p.toolchain.iter()))
        {
            lang.parse::<Language>().map_err(Error)?;
        }
        for s in &self.build.sources {
            relative_inside("build.sources", s)?;
        }
        for s in &self.build.include {
            relative_inside("build.include", s)?;
        }
        relative_inside("build.output", &self.build.output)?;
        if self
            .build
            .sources
            .iter()
            .any(|s| Path::new(s) == Path::new(&self.build.output))
        {
            return Err(Error(
                "build.output must differ from every source directory".into(),
            ));
        }
        if let Some(a) = &self.build.artifact {
            if a.is_empty() || a.contains(['/', '\\']) || a.starts_with('.') {
                return Err(Error("build.artifact must be a plain file name".into()));
            }
        }
        if let Some(ls) = &self.build.linker_script {
            relative_inside("build.linker_script", ls)?;
        }
        if self.profiles.is_empty() {
            return Err(Error("At least one [profiles.<name>] is required".into()));
        }
        for name in self.profiles.keys() {
            if name.is_empty()
                || !name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_".contains(c))
            {
                return Err(Error(format!("Invalid profile name '{name}'")));
            }
        }
        for (k, v) in &self.env {
            if k.is_empty() || k.contains('=') || k.contains('\0') || v.contains('\0') {
                return Err(Error(format!("Invalid environment variable '{k}'")));
            }
        }
        Ok(())
    }

    /// Primary language.
    pub fn language(&self) -> Result<Language> {
        self.project.language.parse().map_err(Error)
    }

    /// Profile by name, with a helpful error.
    pub fn profile(&self, name: &str) -> Result<&Profile> {
        self.profiles.get(name).ok_or_else(|| {
            Error(format!(
                "Unknown profile '{name}'; available: {}",
                self.profiles.keys().cloned().collect::<Vec<_>>().join(", ")
            ))
        })
    }

    /// Toolchain id selected for a language: profile override, then project.
    pub fn toolchain_for(&self, profile: &str, language: Language) -> Option<String> {
        self.profiles
            .get(profile)
            .and_then(|p| p.toolchain.get(language.id()))
            .or_else(|| self.toolchain.get(language.id()))
            .cloned()
    }

    /// Serialize as TOML.
    pub fn to_toml(&self) -> Result<String> {
        toml::to_string_pretty(self).map_err(|e| Error(e.to_string()))
    }

    /// Output directory for a profile.
    pub fn output_dir(&self, root: &Path, profile: &str) -> PathBuf {
        root.join(&self.build.output).join(profile)
    }

    /// Warnings that do not block a build (e.g. credentials in env).
    pub fn warnings(&self) -> Vec<String> {
        self.env
            .keys()
            .filter(|k| crate::looks_secret(k))
            .map(|k| format!("env.{k} looks like a credential; prefer setting it in your shell, not in {MANIFEST_FILE}"))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_manifest_round_trips() {
        let m = Manifest::new("demo", Language::C, BuildKind::Native);
        let text = m.to_toml().unwrap();
        let back = Manifest::parse(&text).unwrap();
        assert_eq!(back.project.name, "demo");
        assert_eq!(back.profiles.len(), 2);
    }

    #[test]
    fn unsafe_paths_and_unknown_fields_are_rejected() {
        let base = "schema = 1\n[project]\nname = \"d\"\nlanguage = \"c\"\n[profiles.debug]\n";
        assert!(
            Manifest::parse(&format!("{base}[build]\noutput = \"../x\"\n"))
                .unwrap_err()
                .0
                .contains("inside the project")
        );
        assert!(Manifest::parse(&format!("{base}[build]\nsources = [\"/etc\"]\n")).is_err());
        assert!(Manifest::parse(&format!(
            "{base}[build]\nsources = [\"src\"]\noutput = \"src\"\n"
        ))
        .is_err());
        assert!(Manifest::parse(&format!("{base}bogus = 1\n")).is_err());
        assert!(Manifest::parse(&base.replace("schema = 1", "schema = 2"))
            .unwrap_err()
            .0
            .contains("schema 2"));
        assert!(Manifest::parse(&base.replace("\"c\"", "\"cobol\"")).is_err());
    }

    #[test]
    fn profile_toolchain_overrides_project_choice() {
        let mut m = Manifest::new("d", Language::C, BuildKind::Native);
        m.toolchain.insert("c".into(), "gcc".into());
        m.profiles
            .get_mut("release")
            .unwrap()
            .toolchain
            .insert("c".into(), "zig-cc".into());
        assert_eq!(
            m.toolchain_for("debug", Language::C).as_deref(),
            Some("gcc")
        );
        assert_eq!(
            m.toolchain_for("release", Language::C).as_deref(),
            Some("zig-cc")
        );
        assert_eq!(m.toolchain_for("release", Language::Cpp), None);
    }

    #[test]
    fn credential_looking_env_is_warned() {
        let mut m = Manifest::new("d", Language::C, BuildKind::Native);
        m.env.insert("OPENAI_API_KEY".into(), "x".into());
        assert_eq!(m.warnings().len(), 1);
    }
}
