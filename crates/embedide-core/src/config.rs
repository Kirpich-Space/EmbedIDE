//! Versioned native settings layered over preserved project metadata.
use crate::{BuildOptions, BuildPlan, Error, Project, Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

/// Named compiler profile; defaults keep builds local/offline.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Profile {
    /// Cargo profile name (dev, release or a project-defined profile).
    pub cargo_profile: String,
    /// Cargo feature names.
    pub features: Vec<String>,
    /// CMake configuration name.
    pub cmake_type: String,
    /// Existing Make target, if selected.
    pub make_target: Option<String>,
}
impl Default for Profile {
    fn default() -> Self {
        Self {
            cargo_profile: "dev".into(),
            features: Vec::new(),
            cmake_type: "Debug".into(),
            make_target: None,
        }
    }
}
/// Hardware identity is separate from the compiler architecture/triple.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Target {
    /// Display name of the user's board.
    pub name: String,
    /// Compiler target, e.g. thumbv6m-none-eabi.
    pub compiler_target: Option<String>,
    /// Exact probe-rs chip name; never guessed from board labels.
    pub chip: Option<String>,
    /// Exact probe selector VID:PID:serial; mandatory for flashing.
    pub probe: Option<String>,
    /// ELF artifact relative to project root (or absolute).
    pub artifact: Option<String>,
}
/// Serial framing; validation occurs before opening the port.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SerialConfig {
    /// Explicit device, selected by user or configuration.
    pub port: String,
    /// Bits per second.
    pub baud: u32,
    /// 5, 6, 7 or 8.
    pub data_bits: u8,
    /// none, odd or even.
    pub parity: String,
    /// 1 or 2.
    pub stop_bits: u8,
    /// none, software or hardware.
    pub flow_control: String,
    /// none, lf or crlf.
    pub line_ending: String,
    /// Disable outgoing text.
    pub receive_only: bool,
}
impl Default for SerialConfig {
    fn default() -> Self {
        Self {
            port: String::new(),
            baud: 115200,
            data_bits: 8,
            parity: "none".into(),
            stop_bits: 1,
            flow_control: "none".into(),
            line_ending: "lf".into(),
            receive_only: true,
        }
    }
}
/// Automatic indentation in the editor.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct IndentConfig {
    /// Language-aware indentation on Enter and for typed closing delimiters.
    pub auto: bool,
    /// Columns per indentation level, 1..=16.
    pub width: usize,
    /// detect (follow each file, Makefiles use tabs), spaces or tabs.
    pub style: String,
}
impl Default for IndentConfig {
    fn default() -> Self {
        Self {
            auto: true,
            width: 4,
            style: "detect".into(),
        }
    }
}
/// One language server: a program started without a shell.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct LspServerConfig {
    /// Executable name or path; an empty string disables the server.
    pub command: String,
    /// Literal arguments.
    pub args: Vec<String>,
    /// `initializationOptions` sent to the server.
    pub initialization_options: Option<serde_json::Value>,
}
/// Code completion in the editor.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CompletionConfig {
    /// Show suggestions while typing (Ctrl+Space works either way).
    pub auto: bool,
    /// Identifier characters typed before suggestions appear automatically, 1..=8.
    pub min_chars: usize,
    /// Ask language servers (rust-analyzer, clangd, zls) when they are installed.
    pub lsp: bool,
    /// Per-language server overrides: rust, c, cpp, zig.
    pub servers: BTreeMap<String, LspServerConfig>,
}
impl Default for CompletionConfig {
    fn default() -> Self {
        Self {
            auto: true,
            min_chars: 2,
            lsp: true,
            servers: BTreeMap::new(),
        }
    }
}
/// Languages that can have a configured language server.
pub const LSP_LANGUAGES: [&str; 4] = ["rust", "c", "cpp", "zig"];
/// GDB/MI with an optional owned OpenOCD server. Starting never loads firmware.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DebugConfig {
    /// GDB executable, with cross debugger explicitly configurable.
    pub gdb: String,
    /// Optional OpenOCD executable. None uses an existing server.
    pub openocd: Option<String>,
    /// OpenOCD -f files; their contents are executable project configuration.
    pub configs: Vec<String>,
    /// Host:port, passed as a single MI argument.
    pub remote: String,
    /// ELF symbols, relative to root or absolute.
    pub elf: String,
}
impl Default for DebugConfig {
    fn default() -> Self {
        Self {
            gdb: "gdb".into(),
            openocd: None,
            configs: Vec::new(),
            remote: "127.0.0.1:3333".into(),
            elf: String::new(),
        }
    }
}
/// User-defined task with argument boundaries preserved.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskConfig {
    /// Executable path or PATH name.
    pub program: String,
    /// Literal arguments; no implicit shell.
    #[serde(default)]
    pub args: Vec<String>,
}
/// Native configuration stored in embedide.json `native`, overridden by embedide.local.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NativeConfig {
    /// Schema version, currently 1.
    pub version: u32,
    /// dark, light or mono.
    pub theme: String,
    /// Tab display width, 1..=16.
    pub tab_width: usize,
    /// Named build profiles.
    pub profiles: BTreeMap<String, Profile>,
    /// Configured boards only, never a claim of hardware support.
    pub targets: Vec<Target>,
    /// Environment passed only to explicitly started tasks/builds.
    pub environment: BTreeMap<String, String>,
    /// Map tool names to executable paths.
    pub tools: BTreeMap<String, String>,
    /// CMake output directory.
    pub build_directory: String,
    /// Optional CMake generator.
    pub cmake_generator: Option<String>,
    /// Optional CMake toolchain file.
    pub cmake_toolchain: Option<String>,
    /// Serial connection options.
    pub serial: SerialConfig,
    /// Debugger connection options.
    pub debug: DebugConfig,
    /// Explicitly invoked tasks.
    pub tasks: BTreeMap<String, TaskConfig>,
    /// Global command bindings; documented command IDs mapped to F1..F12, ctrl+letter or alt+digit.
    pub keys: BTreeMap<String, String>,
    /// Capture the mouse in the IDE (click to focus/place the cursor, wheel to
    /// scroll). Off by default: while captured, the terminal's own text
    /// selection usually needs Shift+drag.
    pub mouse: bool,
    /// Automatic indentation.
    pub indent: IndentConfig,
    /// Code completion and language servers.
    pub completion: CompletionConfig,
}
impl Default for NativeConfig {
    fn default() -> Self {
        Self {
            version: 1,
            theme: "mono".into(),
            tab_width: 4,
            profiles: BTreeMap::from([
                ("debug".into(), Profile::default()),
                (
                    "release".into(),
                    Profile {
                        cargo_profile: "release".into(),
                        cmake_type: "Release".into(),
                        ..Profile::default()
                    },
                ),
            ]),
            targets: Vec::new(),
            environment: BTreeMap::new(),
            tools: BTreeMap::new(),
            build_directory: ".embedide-build".into(),
            cmake_generator: None,
            cmake_toolchain: None,
            serial: SerialConfig::default(),
            debug: DebugConfig::default(),
            tasks: BTreeMap::new(),
            keys: BTreeMap::new(),
            mouse: false,
            indent: IndentConfig::default(),
            completion: CompletionConfig::default(),
        }
    }
}
fn merge(base: &mut serde_json::Value, overlay: serde_json::Value) {
    if let (Some(base), Some(overlay)) = (base.as_object_mut(), overlay.as_object()) {
        for (key, value) in overlay {
            if let Some(existing) = base.get_mut(key) {
                merge(existing, value.clone());
            } else {
                base.insert(key.clone(), value.clone());
            }
        }
    } else {
        *base = overlay;
    }
}
impl NativeConfig {
    /// Load local-only files; never modifies unknown project metadata.
    pub fn load(root: &Path) -> Result<Self> {
        let mut value = serde_json::to_value(Self::default())?;
        let project = root.join("embedide.json");
        match std::fs::read(&project) {
            Ok(bytes) => {
                let project: serde_json::Value = serde_json::from_slice(&bytes)?;
                if let Some(native) = project.get("native") {
                    merge(&mut value, native.clone());
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        match std::fs::read(root.join("embedide.local.json")) {
            Ok(bytes) => merge(&mut value, serde_json::from_slice(&bytes)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        let config: Self = serde_json::from_value(value)
            .map_err(|e| Error(format!("Invalid native configuration: {e}")))?;
        if config.version != 1
            || !(1..=16).contains(&config.tab_width)
            || !["dark", "light", "mono"].contains(&config.theme.as_str())
        {
            return Err(Error(
                "Expected native version=1, tab_width=1..16, theme=dark|light|mono".into(),
            ));
        }
        let mut bindings = std::collections::BTreeSet::new();
        for binding in config.keys.values() {
            let binding = binding.to_ascii_lowercase();
            let valid = binding
                .strip_prefix('f')
                .and_then(|n| n.parse::<u8>().ok())
                .is_some_and(|n| (1..=12).contains(&n))
                || binding.strip_prefix("ctrl+").is_some_and(|letter| {
                    letter == "space"
                        || letter.len() == 1 && letter.as_bytes()[0].is_ascii_lowercase()
                })
                || binding
                    .strip_prefix("ctrl+alt+")
                    .or_else(|| binding.strip_prefix("alt+"))
                    .is_some_and(|digit| digit.len() == 1 && digit.as_bytes()[0].is_ascii_digit());
            if !valid || !bindings.insert(binding) {
                return Err(Error(
                    "Key bindings must be unique F1..F12, ctrl+a..ctrl+z, ctrl+space, alt+0..alt+9 or ctrl+alt+0..ctrl+alt+9 chords"
                        .into(),
                ));
            }
        }
        if !(1..=16).contains(&config.indent.width)
            || !["detect", "spaces", "tabs"].contains(&config.indent.style.as_str())
        {
            return Err(Error(
                "Expected indent.width=1..16 and indent.style=detect|spaces|tabs".into(),
            ));
        }
        if !(1..=8).contains(&config.completion.min_chars) {
            return Err(Error("Expected completion.min_chars=1..8".into()));
        }
        if let Some(language) = config
            .completion
            .servers
            .keys()
            .find(|l| !LSP_LANGUAGES.contains(&l.as_str()))
        {
            return Err(Error(format!(
                "completion.servers.{language}: language servers are configurable for {}",
                LSP_LANGUAGES.join(", ")
            )));
        }
        if config.profiles.is_empty() {
            return Err(Error("At least one build profile is required".into()));
        }
        for target in &config.targets {
            if target.name.trim().is_empty() {
                return Err(Error("Target name must not be empty".into()));
            }
        }
        Ok(config)
    }
    /// Extend the existing adapter plan with explicit target/profile/tool overrides.
    pub fn build_plan(
        &self,
        project: &Project,
        profile: &str,
        target: Option<usize>,
        check: bool,
    ) -> Result<BuildPlan> {
        let profile = self
            .profiles
            .get(profile)
            .ok_or_else(|| Error("Unknown build profile".into()))?;
        let mut plan = crate::plan_build(
            project,
            &BuildOptions {
                system: None,
                release: false,
                jobs: 2,
                online: false,
            },
        )?;
        match plan.system {
            crate::BuildSystem::Cargo => {
                let args = &mut plan.steps[0].args;
                if check {
                    args[0] = "check".into();
                }
                args.push("--message-format=json".into());
                if profile.cargo_profile != "dev" {
                    args.extend(["--profile".into(), profile.cargo_profile.clone()]);
                }
                if !profile.features.is_empty() {
                    args.extend(["--features".into(), profile.features.join(",")]);
                }
                if let Some(triple) = target
                    .and_then(|i| self.targets.get(i))
                    .and_then(|t| t.compiler_target.as_ref())
                {
                    args.extend(["--target".into(), triple.clone()]);
                }
            }
            crate::BuildSystem::Cmake => {
                let configure = &mut plan.steps[0].args;
                configure[3] = self.build_directory.clone();
                configure[4] = format!("-DCMAKE_BUILD_TYPE={}", profile.cmake_type);
                if let Some(generator) = &self.cmake_generator {
                    configure.extend(["-G".into(), generator.clone()]);
                }
                if let Some(file) = &self.cmake_toolchain {
                    configure.push(format!("-DCMAKE_TOOLCHAIN_FILE={file}"));
                }
                plan.steps[1].args[1] = self.build_directory.clone();
                plan.steps[1].args[3] = profile.cmake_type.clone();
            }
            crate::BuildSystem::Make => {
                if let Some(target) = &profile.make_target {
                    if target.starts_with('-') {
                        return Err(Error("Make target cannot be an option".into()));
                    }
                    plan.steps[0].args.push(target.clone());
                }
            }
            crate::BuildSystem::Zig => {}
        }
        for step in &mut plan.steps {
            if let Some(path) = self.tools.get(&step.program) {
                step.program = path.clone();
            }
        }
        Ok(plan)
    }
}
