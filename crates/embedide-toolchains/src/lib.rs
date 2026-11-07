//! Toolchain discovery and selection.
//!
//! EmbedIDE never imposes a compiler: it finds what is installed, reports
//! versions, paths and targets, and lets the user pick per project and per
//! profile. Nothing is downloaded or installed here; [`recommend`] only
//! returns suggestions as text.

use serde::Serialize;
use std::{
    collections::BTreeSet,
    env, fmt,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    str::FromStr,
    thread,
    time::{Duration, Instant},
};

/// Source language a toolchain can compile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    /// C
    C,
    /// C++
    Cpp,
    /// Rust
    Rust,
    /// Zig
    Zig,
    /// Assembly (GNU syntax through a C driver, GNU as, or NASM)
    Asm,
    /// Fortran
    Fortran,
}

impl Language {
    /// All supported languages.
    pub const ALL: [Language; 6] = [
        Language::C,
        Language::Cpp,
        Language::Rust,
        Language::Zig,
        Language::Asm,
        Language::Fortran,
    ];
    /// Stable identifier used in configuration files.
    pub fn id(self) -> &'static str {
        match self {
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::Rust => "rust",
            Self::Zig => "zig",
            Self::Asm => "asm",
            Self::Fortran => "fortran",
        }
    }
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

impl FromStr for Language {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "c" => Ok(Self::C),
            "cpp" | "c++" | "cxx" => Ok(Self::Cpp),
            "rust" | "rs" => Ok(Self::Rust),
            "zig" => Ok(Self::Zig),
            "asm" | "assembly" => Ok(Self::Asm),
            "fortran" | "f90" => Ok(Self::Fortran),
            _ => Err(format!(
                "Unknown language '{s}'; expected c, cpp, rust, zig, asm or fortran"
            )),
        }
    }
}

/// How a toolchain is driven; decides command-line conventions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Family {
    /// GCC driver conventions (gcc, g++, gfortran, cross gcc).
    Gcc,
    /// Clang driver conventions (clang, clang++, flang).
    Clang,
    /// `zig cc` / `zig c++` (Clang-compatible, bundled libc for many targets).
    ZigCc,
    /// rustc / cargo.
    Rust,
    /// The Zig compiler itself.
    Zig,
    /// GNU assembler.
    Gas,
    /// NASM.
    Nasm,
    /// LFortran.
    Lfortran,
}

/// A kind of toolchain EmbedIDE knows how to drive.
#[derive(Debug)]
pub struct Kind {
    /// Kind identifier (`gcc`, `zig-cc`, …).
    pub id: &'static str,
    /// Driver conventions.
    pub family: Family,
    /// Languages this kind compiles.
    pub languages: &'static [Language],
    /// Executable names, in preference order.
    pub programs: &'static [&'static str],
    /// Arguments that always precede user arguments (`zig cc` → `["cc"]`).
    pub prefix_args: &'static [&'static str],
    /// Arguments that print a version.
    pub version_args: &'static [&'static str],
    /// Supports `-dumpmachine` to report its default target.
    pub dump_machine: bool,
    /// Suffix used by triple-prefixed cross tools (`arm-none-eabi-gcc`).
    pub cross_suffix: Option<&'static str>,
}

use Language as L;

/// Every kind EmbedIDE can configure.
pub const KINDS: &[Kind] = &[
    Kind {
        id: "gcc",
        family: Family::Gcc,
        languages: &[L::C, L::Asm],
        programs: &["gcc"],
        prefix_args: &[],
        version_args: &["--version"],
        dump_machine: true,
        cross_suffix: Some("gcc"),
    },
    Kind {
        id: "g++",
        family: Family::Gcc,
        languages: &[L::Cpp],
        programs: &["g++"],
        prefix_args: &[],
        version_args: &["--version"],
        dump_machine: true,
        cross_suffix: Some("g++"),
    },
    Kind {
        id: "clang",
        family: Family::Clang,
        languages: &[L::C, L::Asm],
        programs: &["clang"],
        prefix_args: &[],
        version_args: &["--version"],
        dump_machine: true,
        cross_suffix: None,
    },
    Kind {
        id: "clang++",
        family: Family::Clang,
        languages: &[L::Cpp],
        programs: &["clang++"],
        prefix_args: &[],
        version_args: &["--version"],
        dump_machine: true,
        cross_suffix: None,
    },
    Kind {
        id: "zig-cc",
        family: Family::ZigCc,
        languages: &[L::C, L::Asm],
        programs: &["zig"],
        prefix_args: &["cc"],
        version_args: &["--version"],
        dump_machine: true,
        cross_suffix: None,
    },
    Kind {
        id: "zig-c++",
        family: Family::ZigCc,
        languages: &[L::Cpp],
        programs: &["zig"],
        prefix_args: &["c++"],
        version_args: &["--version"],
        dump_machine: true,
        cross_suffix: None,
    },
    Kind {
        id: "rustc",
        family: Family::Rust,
        languages: &[L::Rust],
        programs: &["rustc"],
        prefix_args: &[],
        version_args: &["--version"],
        dump_machine: false,
        cross_suffix: None,
    },
    Kind {
        id: "cargo",
        family: Family::Rust,
        languages: &[L::Rust],
        programs: &["cargo"],
        prefix_args: &[],
        version_args: &["--version"],
        dump_machine: false,
        cross_suffix: None,
    },
    Kind {
        id: "zig",
        family: Family::Zig,
        languages: &[L::Zig],
        programs: &["zig"],
        prefix_args: &[],
        version_args: &["version"],
        dump_machine: false,
        cross_suffix: None,
    },
    Kind {
        id: "gas",
        family: Family::Gas,
        languages: &[L::Asm],
        programs: &["as"],
        prefix_args: &[],
        version_args: &["--version"],
        dump_machine: false,
        cross_suffix: Some("as"),
    },
    Kind {
        id: "nasm",
        family: Family::Nasm,
        languages: &[L::Asm],
        programs: &["nasm"],
        prefix_args: &[],
        version_args: &["-v"],
        dump_machine: false,
        cross_suffix: None,
    },
    Kind {
        id: "gfortran",
        family: Family::Gcc,
        languages: &[L::Fortran],
        programs: &["gfortran"],
        prefix_args: &[],
        version_args: &["--version"],
        dump_machine: true,
        cross_suffix: Some("gfortran"),
    },
    Kind {
        id: "flang",
        family: Family::Clang,
        languages: &[L::Fortran],
        programs: &["flang-new", "flang"],
        prefix_args: &[],
        version_args: &["--version"],
        dump_machine: false,
        cross_suffix: None,
    },
    Kind {
        id: "lfortran",
        family: Family::Lfortran,
        languages: &[L::Fortran],
        programs: &["lfortran"],
        prefix_args: &[],
        version_args: &["--version"],
        dump_machine: false,
        cross_suffix: None,
    },
];

/// Look up a kind by id.
pub fn kind(id: &str) -> Option<&'static Kind> {
    KINDS.iter().find(|k| k.id == id)
}

/// Where a toolchain entry came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    /// Found on PATH.
    Path,
    /// Found in a user-configured search directory.
    SearchDir,
    /// Explicitly configured by the user with an executable path.
    Configured,
}

/// A user-defined toolchain: any executable of a known kind, any path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomToolchain {
    /// Unique id the user refers to (`gcc-13`, `my-arm-gcc`).
    pub id: String,
    /// Kind id (see [`KINDS`]).
    pub kind: String,
    /// Absolute executable path. Spaces are fine: no shell is involved.
    pub path: PathBuf,
}

/// One usable (or at least present) toolchain.
#[derive(Debug, Clone, Serialize)]
pub struct Toolchain {
    /// Identifier used in configuration (`gcc`, `arm-none-eabi-gcc`, `zig-cc`).
    pub id: String,
    /// Kind id.
    pub kind: &'static str,
    /// Driver conventions.
    pub family: Family,
    /// Languages it compiles.
    pub languages: Vec<Language>,
    /// Executable.
    pub program: PathBuf,
    /// Leading arguments (`["cc"]` for `zig cc`).
    pub prefix_args: Vec<String>,
    /// First line of the version output, when probed.
    pub version: Option<String>,
    /// Default target triple, when the tool reports one.
    pub target: Option<String>,
    /// True when the default target is not the host (a cross compiler).
    pub cross: bool,
    /// Where it was found.
    pub origin: Origin,
    /// Why probing failed, if it did.
    pub problem: Option<String>,
}

impl Toolchain {
    /// A command for this toolchain with its prefix arguments applied.
    pub fn command(&self) -> Command {
        let mut c = Command::new(&self.program);
        c.args(&self.prefix_args);
        c
    }
    /// Whether this toolchain compiles `language`.
    pub fn supports(&self, language: Language) -> bool {
        self.languages.contains(&language)
    }
}

/// Discovery inputs.
#[derive(Debug, Default, Clone)]
pub struct DetectOptions {
    /// Extra directories searched after PATH (e.g. a vendor toolchain).
    pub search_dirs: Vec<PathBuf>,
    /// Explicit user toolchains; they take precedence over discovered ids.
    pub custom: Vec<CustomToolchain>,
    /// Run version/target queries (each bounded by a timeout).
    pub probe: bool,
}

const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Discover toolchains on PATH, in search directories and from configuration.
pub fn detect(options: &DetectOptions) -> Vec<Toolchain> {
    let path_dirs: Vec<PathBuf> = env::var_os("PATH")
        .map(|p| env::split_paths(&p).filter(|d| d.is_absolute()).collect())
        .unwrap_or_default();
    let mut found: Vec<Toolchain> = Vec::new();
    let mut seen_programs: BTreeSet<(String, PathBuf)> = BTreeSet::new();
    let mut seen_ids: BTreeSet<String> = BTreeSet::new();

    for custom in &options.custom {
        if let Some(tc) = from_custom(custom) {
            seen_ids.insert(tc.id.clone());
            found.push(tc);
        }
    }

    let dirs: Vec<(PathBuf, Origin)> = path_dirs
        .iter()
        .map(|d| (d.clone(), Origin::Path))
        .chain(
            options
                .search_dirs
                .iter()
                .map(|d| (d.clone(), Origin::SearchDir)),
        )
        .collect();

    // Plain host tools (first match on PATH wins, like a shell would)
    for k in KINDS {
        for name in k.programs {
            if let Some((program, origin)) = dirs
                .iter()
                .find_map(|(d, o)| executable_in(d, name).map(|p| (p, *o)))
            {
                let key = (k.id.to_string(), canonical(&program));
                if !seen_ids.contains(k.id) && seen_programs.insert(key) {
                    seen_ids.insert(k.id.to_string());
                    found.push(new_toolchain(k.id.to_string(), k, program, origin));
                }
                break;
            }
        }
    }

    // Triple-prefixed tools: arm-none-eabi-gcc, riscv64-unknown-elf-gcc, …
    for (dir, origin) in &dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        for name in names {
            let stem = name.strip_suffix(".exe").unwrap_or(&name);
            for k in KINDS.iter().filter(|k| k.cross_suffix.is_some()) {
                let suffix = format!("-{}", k.cross_suffix.unwrap_or_default());
                let Some(triple) = stem.strip_suffix(&suffix) else {
                    continue;
                };
                if !looks_like_triple(triple) || seen_ids.contains(stem) {
                    continue;
                }
                let Some(program) = executable_in(dir, stem) else {
                    continue;
                };
                // Host-prefixed aliases (x86_64-pc-linux-gnu-gcc) duplicate plain gcc
                if seen_programs.iter().any(|(_, p)| *p == canonical(&program))
                    || (is_host_triple(triple) && seen_ids.contains(k.id))
                {
                    continue;
                }
                seen_programs.insert((stem.to_string(), canonical(&program)));
                seen_ids.insert(stem.to_string());
                let mut tc = new_toolchain(stem.to_string(), k, program, *origin);
                tc.target = Some(triple.to_string());
                tc.cross = !is_host_triple(triple);
                found.push(tc);
            }
        }
    }

    if options.probe {
        let handles: Vec<_> = found
            .into_iter()
            .map(|mut tc| {
                thread::spawn(move || {
                    probe(&mut tc);
                    tc
                })
            })
            .collect();
        found = handles.into_iter().filter_map(|h| h.join().ok()).collect();
    }
    found
}

fn new_toolchain(id: String, k: &'static Kind, program: PathBuf, origin: Origin) -> Toolchain {
    Toolchain {
        id,
        kind: k.id,
        family: k.family,
        languages: k.languages.to_vec(),
        program,
        prefix_args: k.prefix_args.iter().map(|s| s.to_string()).collect(),
        version: None,
        target: None,
        cross: false,
        origin,
        problem: None,
    }
}

fn from_custom(c: &CustomToolchain) -> Option<Toolchain> {
    let k = kind(&c.kind)?;
    let mut tc = new_toolchain(c.id.clone(), k, c.path.clone(), Origin::Configured);
    if !c.path.is_file() {
        tc.problem = Some(format!(
            "Configured path does not exist: {}",
            c.path.display()
        ));
    }
    Some(tc)
}

/// Validate a custom toolchain definition before saving it.
pub fn validate_custom(c: &CustomToolchain) -> Result<(), String> {
    if c.id.is_empty()
        || !c
            .id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || "-_.+".contains(ch))
    {
        return Err("Toolchain id must use letters, digits, '-', '_', '.' or '+'".into());
    }
    if kind(&c.kind).is_none() {
        let ids: Vec<_> = KINDS.iter().map(|k| k.id).collect();
        return Err(format!(
            "Unknown toolchain kind '{}'; expected one of {}",
            c.kind,
            ids.join(", ")
        ));
    }
    if !c.path.is_absolute() {
        return Err("Toolchain path must be absolute".into());
    }
    if !c.path.is_file() {
        return Err(format!("No executable at {}", c.path.display()));
    }
    Ok(())
}

/// Query version and default target. Bounded by a timeout; never interactive.
pub fn probe(tc: &mut Toolchain) {
    let Some(k) = kind(tc.kind) else { return };
    match run_capture(&tc.program, &tc.prefix_args, k.version_args, PROBE_TIMEOUT) {
        Ok(out) => {
            tc.version = out
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .map(str::to_owned);
            if tc.version.is_none() {
                tc.problem = Some("Version query printed nothing".into());
            }
        }
        Err(e) => {
            tc.problem = Some(e);
            return;
        }
    }
    if k.dump_machine && tc.target.is_none() {
        if let Ok(out) = run_capture(
            &tc.program,
            &tc.prefix_args,
            &["-dumpmachine"],
            PROBE_TIMEOUT,
        ) {
            let t = out.trim().to_string();
            if !t.is_empty() {
                tc.cross = !is_host_triple(&t);
                tc.target = Some(t);
            }
        }
    }
    if k.family == Family::Rust && k.id == "rustc" {
        if let Ok(out) = run_capture(&tc.program, &[], &["-vV"], PROBE_TIMEOUT) {
            tc.target = out
                .lines()
                .find_map(|l| l.strip_prefix("host: "))
                .map(|s| s.trim().to_string());
        }
    }
}

/// Run a tool with a timeout and return stdout+stderr (some tools print versions on stderr).
pub fn run_capture(
    program: &Path,
    prefix: &[String],
    args: &[&str],
    timeout: Duration,
) -> Result<String, String> {
    let mut child = Command::new(program)
        .args(prefix)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Cannot run {}: {e}", program.display()))?;
    let mut out = child.stdout.take().expect("piped");
    let mut err = child.stderr.take().expect("piped");
    let ro = thread::spawn(move || {
        let mut s = Vec::new();
        let _ = out.read_to_end(&mut s);
        s
    });
    let re = thread::spawn(move || {
        let mut s = Vec::new();
        let _ = err.read_to_end(&mut s);
        s
    });
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "{} did not answer within {}s",
                    program.display(),
                    timeout.as_secs()
                ));
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(e) => return Err(e.to_string()),
        }
    }
    let mut text = String::from_utf8_lossy(&ro.join().unwrap_or_default()).into_owned();
    let e = String::from_utf8_lossy(&re.join().unwrap_or_default()).into_owned();
    if text.trim().is_empty() {
        text = e;
    }
    Ok(text)
}

fn executable_in(dir: &Path, name: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    let candidate = dir.join(format!("{name}.exe"));
    #[cfg(not(windows))]
    let candidate = dir.join(name);
    if !candidate.is_file() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if candidate.metadata().ok()?.permissions().mode() & 0o111 == 0 {
            return None;
        }
    }
    Some(candidate)
}

fn canonical(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// `arch-vendor-os[-abi]` with 2–4 components, letters/digits/underscore.
fn looks_like_triple(s: &str) -> bool {
    let parts: Vec<_> = s.split('-').collect();
    (2..=4).contains(&parts.len())
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        && parts[0]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
}

/// Whether a target triple names this host (architecture and OS family).
pub fn is_host_triple(triple: &str) -> bool {
    let t = triple.to_ascii_lowercase();
    let arch_ok = match env::consts::ARCH {
        "x86_64" => t.starts_with("x86_64") || t.starts_with("amd64"),
        "aarch64" => t.starts_with("aarch64") || t.starts_with("arm64"),
        other => t.starts_with(other),
    };
    let os_ok = match env::consts::OS {
        "linux" => t.contains("linux"),
        "windows" => t.contains("windows") || t.contains("mingw") || t.contains("msvc"),
        "macos" => t.contains("darwin") || t.contains("apple") || t.contains("macos"),
        other => t.contains(other),
    };
    arch_ok && os_ok
}

/// A suggested installation command. EmbedIDE prints it; it never runs it.
#[derive(Debug, Clone, Serialize)]
pub struct Recommendation {
    /// Toolchain kind.
    pub kind: &'static str,
    /// Package manager command line for this OS, if known.
    pub command: Option<String>,
    /// Where to read more.
    pub note: &'static str,
}

/// Detect the host package manager family from /etc/os-release (Linux) or the OS.
pub fn package_family() -> &'static str {
    match env::consts::OS {
        "windows" => "winget",
        "macos" => "brew",
        _ => {
            let osr = std::fs::read_to_string("/etc/os-release")
                .unwrap_or_default()
                .to_ascii_lowercase();
            let ids: String = osr
                .lines()
                .filter(|l| l.starts_with("id=") || l.starts_with("id_like="))
                .collect::<Vec<_>>()
                .join(" ");
            if ids.contains("arch") || ids.contains("artix") || ids.contains("manjaro") {
                "pacman"
            } else if ids.contains("debian") || ids.contains("ubuntu") {
                "apt"
            } else if ids.contains("fedora") || ids.contains("rhel") {
                "dnf"
            } else if ids.contains("void") {
                "xbps"
            } else {
                "unknown"
            }
        }
    }
}

/// Installation suggestions for a language (text only, never executed).
pub fn recommend(language: Language, family: &str) -> Vec<Recommendation> {
    let pkg =
        |kind: &'static str, pacman: &str, apt: &str, dnf: &str, brew: &str, note: &'static str| {
            let command = match family {
                "pacman" if !pacman.is_empty() => Some(format!("sudo pacman -S {pacman}")),
                "apt" if !apt.is_empty() => Some(format!("sudo apt install {apt}")),
                "dnf" if !dnf.is_empty() => Some(format!("sudo dnf install {dnf}")),
                "brew" if !brew.is_empty() => Some(format!("brew install {brew}")),
                _ => None,
            };
            Recommendation {
                kind,
                command,
                note,
            }
        };
    match language {
        Language::C | Language::Asm => vec![
            pkg("gcc", "gcc", "gcc", "gcc", "gcc", "GNU C compiler"),
            pkg(
                "clang",
                "clang",
                "clang",
                "clang",
                "llvm",
                "LLVM C compiler",
            ),
            pkg(
                "zig-cc",
                "zig",
                "",
                "zig",
                "zig",
                "Zig's bundled Clang (zig cc); see ziglang.org/download",
            ),
            pkg(
                "arm-none-eabi-gcc",
                "arm-none-eabi-gcc arm-none-eabi-newlib",
                "gcc-arm-none-eabi",
                "arm-none-eabi-gcc-cs",
                "--cask gcc-arm-embedded",
                "Arm Cortex-M cross compiler",
            ),
            pkg(
                "nasm",
                "nasm",
                "nasm",
                "nasm",
                "nasm",
                "Netwide Assembler (x86)",
            ),
        ],
        Language::Cpp => vec![
            pkg("g++", "gcc", "g++", "gcc-c++", "gcc", "GNU C++ compiler"),
            pkg(
                "clang++",
                "clang",
                "clang",
                "clang",
                "llvm",
                "LLVM C++ compiler",
            ),
            pkg("zig-c++", "zig", "", "zig", "zig", "zig c++"),
        ],
        Language::Rust => vec![pkg(
            "rustc",
            "rustup",
            "",
            "rustup",
            "rustup",
            "Rust via rustup (rustup.rs); add embedded targets with `rustup target add`",
        )],
        Language::Zig => vec![pkg("zig", "zig", "", "zig", "zig", "ziglang.org/download")],
        Language::Fortran => vec![
            pkg(
                "gfortran",
                "gcc-fortran",
                "gfortran",
                "gcc-gfortran",
                "gcc",
                "GNU Fortran",
            ),
            pkg("flang", "flang", "flang", "flang", "flang", "LLVM Flang"),
            pkg(
                "lfortran",
                "",
                "",
                "",
                "lfortran",
                "LFortran (lfortran.org)",
            ),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_round_trip() {
        for l in Language::ALL {
            assert_eq!(l.id().parse::<Language>().unwrap(), l);
        }
        assert_eq!("C++".parse::<Language>().unwrap(), Language::Cpp);
        assert!("cobol".parse::<Language>().is_err());
    }

    #[test]
    fn zig_cc_and_cpp_are_distinct_kinds_with_prefix_args() {
        let cc = kind("zig-cc").unwrap();
        let cxx = kind("zig-c++").unwrap();
        assert_eq!(cc.prefix_args, ["cc"]);
        assert_eq!(cxx.prefix_args, ["c++"]);
        assert!(cc.languages.contains(&Language::C));
        assert!(cxx.languages.contains(&Language::Cpp));
    }

    #[test]
    fn triples_are_recognised() {
        assert!(looks_like_triple("arm-none-eabi"));
        assert!(looks_like_triple("riscv64-unknown-elf"));
        assert!(looks_like_triple("x86_64-w64-mingw32"));
        assert!(!looks_like_triple("gcc"));
        assert!(!looks_like_triple("c89"));
        assert!(!looks_like_triple("-a"));
    }

    #[test]
    fn cross_triples_are_not_host() {
        assert!(!is_host_triple("arm-none-eabi"));
        assert!(!is_host_triple("riscv64-unknown-elf"));
        if env::consts::ARCH == "x86_64" && env::consts::OS == "linux" {
            assert!(is_host_triple("x86_64-pc-linux-gnu"));
            assert!(is_host_triple("x86_64-linux-gnu"));
        }
    }

    #[test]
    fn custom_toolchains_are_validated() {
        let bad = CustomToolchain {
            id: "x y".into(),
            kind: "gcc".into(),
            path: "/bin/sh".into(),
        };
        assert!(validate_custom(&bad).is_err());
        let bad = CustomToolchain {
            id: "x".into(),
            kind: "msvc".into(),
            path: "/bin/sh".into(),
        };
        assert!(validate_custom(&bad)
            .unwrap_err()
            .contains("Unknown toolchain kind"));
        let bad = CustomToolchain {
            id: "x".into(),
            kind: "gcc".into(),
            path: "relative/gcc".into(),
        };
        assert!(validate_custom(&bad).is_err());
    }

    #[test]
    fn recommendations_are_text_only() {
        let r = recommend(Language::Fortran, "pacman");
        assert!(r
            .iter()
            .any(|x| x.command.as_deref() == Some("sudo pacman -S gcc-fortran")));
        assert!(recommend(Language::C, "unknown")
            .iter()
            .all(|x| x.command.is_none()));
    }
}
