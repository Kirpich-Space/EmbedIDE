//! Portable `Makefile` generation from `embedide.toml` or detected project sources.
//!
//! Generates explicit per-object rules using POSIX/BSD/GNU-compatible syntax so
//! the output works with both Linux GNU `make` and FreeBSD `make` / `gmake`.
//! Fortran projects encode exact topological `.mod` dependencies between object
//! targets. Existing files are never overwritten unless `force` is set.

use crate::{fortran, Error, Result};
use embedide_toolchains::Language;
use embedide_workspace::Manifest;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as FmtWrite,
    io::Write,
    path::{Component, Path, PathBuf},
};

/// Options for [`write`].
#[derive(Debug, Clone)]
pub struct MakefileOptions {
    /// Profile whose flags and defines seed the default Makefile variables.
    pub profile: String,
    /// Output path relative to the project root (default: `Makefile`).
    pub output: PathBuf,
    /// Replace an existing output file when true.
    pub force: bool,
}

impl Default for MakefileOptions {
    fn default() -> Self {
        Self {
            profile: "debug".into(),
            output: PathBuf::from("Makefile"),
            force: false,
        }
    }
}

/// Summary of a generated Makefile written to disk.
#[derive(Debug, Clone, Serialize)]
pub struct GeneratedMakefile {
    /// Path written on disk.
    pub path: PathBuf,
    /// Primary project language.
    pub language: Language,
    /// Source files included in the generated rules.
    pub sources: Vec<String>,
    /// True when an existing file was replaced via `force`.
    pub overwritten: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SrcKind {
    C,
    Cpp,
    Gas,
    Nasm,
    Fortran,
    Rust,
    Zig,
    R,
}

fn classify(path: &Path) -> Option<SrcKind> {
    let ext = path.extension()?.to_str()?;
    Some(match ext {
        "c" => SrcKind::C,
        "cpp" | "cc" | "cxx" | "c++" | "C" => SrcKind::Cpp,
        "s" | "S" | "sx" => SrcKind::Gas,
        "asm" | "nasm" => SrcKind::Nasm,
        "f" | "for" | "ftn" | "f90" | "f95" | "f03" | "f08" | "f18" | "fpp" | "F" | "FOR"
        | "FTN" | "F90" | "F95" | "F03" | "F08" | "F18" | "FPP" => SrcKind::Fortran,
        "rs" => SrcKind::Rust,
        "zig" => SrcKind::Zig,
        "r" | "R" => SrcKind::R,
        _ => return None,
    })
}

const SKIP_DIRS: &[&str] = &[
    "build",
    "target",
    "zig-out",
    ".zig-cache",
    "zig-cache",
    "node_modules",
    ".git",
];

fn collect_files(dir: &Path, skip: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| Error(format!("Cannot read {}: {e}", dir.display())))?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        let path = e.path();
        let ft = e.file_type()?;
        if ft.is_dir() {
            if name.starts_with('.') || SKIP_DIRS.contains(&name.as_str()) || path == skip {
                continue;
            }
            collect_files(&path, skip, out)?;
        } else if ft.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

/// Reject characters that would break Make rules or smuggle recipe commands.
fn check_make_token(token: &str, label: &str) -> Result<()> {
    if token.is_empty() || token.contains(['\n', '\r', '\0', ';', '`', '$', '|', '&', '\'', '"']) {
        return Err(Error(format!(
            "Cannot embed {label} {token:?} safely in a generated Makefile"
        )));
    }
    Ok(())
}

fn rel_posix(root: &Path, path: &Path) -> Result<String> {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let text = rel.to_string_lossy().replace('\\', "/");
    if text.contains(char::is_whitespace) {
        return Err(Error(format!(
            "Make rules cannot safely handle whitespace in path '{text}'"
        )));
    }
    check_make_token(&text, "path")?;
    Ok(text)
}

fn obj_name(rel: &str) -> String {
    format!("{}.o", rel.replace(['/', '\\'], "__"))
}

fn join_flags(flags: &[String], label: &str) -> Result<String> {
    for f in flags {
        check_make_token(f, label)?;
    }
    Ok(flags.join(" "))
}

fn compiler_for(
    manifest: Option<&Manifest>,
    profile: &str,
    lang: Language,
    default: &str,
) -> String {
    let chosen = manifest.and_then(|m| m.toolchain_for(profile, lang));
    match chosen.as_deref() {
        Some("zig-cc") => "zig cc".into(),
        Some("zig-c++") => "zig c++".into(),
        Some(other) if !other.contains(['\n', '\r', ';', '`', '$']) => other.into(),
        _ => default.into(),
    }
}

/// Render a portable `Makefile` string for `root` (with optional `embedide.toml`).
pub fn render(
    root: &Path,
    manifest: Option<&Manifest>,
    profile_name: &str,
) -> Result<(String, Language, Vec<String>)> {
    let project_name = manifest
        .map(|m| m.project.name.clone())
        .or_else(|| {
            root.file_name()
                .and_then(|n| n.to_str())
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| "app".into());
    let artifact_name = manifest
        .and_then(|m| m.build.artifact.clone())
        .unwrap_or_else(|| project_name.clone());
    check_make_token(&artifact_name, "artifact name")?;
    let output_dir = manifest
        .map(|m| m.build.output.clone())
        .unwrap_or_else(|| "build".into());
    check_make_token(&output_dir, "output directory")?;

    let source_dirs: Vec<PathBuf> = match manifest {
        Some(m) => m.build.sources.iter().map(|s| root.join(s)).collect(),
        None if root.join("src").is_dir() => vec![root.join("src")],
        None => vec![root.to_path_buf()],
    };
    let output_root = root.join(&output_dir);
    let mut raw_files = Vec::new();
    for d in &source_dirs {
        if d.is_dir() {
            collect_files(d, &output_root, &mut raw_files)?;
        }
    }
    let classified: Vec<(PathBuf, SrcKind)> = raw_files
        .into_iter()
        .filter_map(|p| classify(&p).map(|k| (p, k)))
        .collect();

    let language = match manifest {
        Some(m) => m.language().map_err(|e| Error(e.0))?,
        None => {
            if root.join("Cargo.toml").is_file() {
                Language::Rust
            } else if root.join("build.zig").is_file() {
                Language::Zig
            } else if classified.iter().any(|(_, k)| *k == SrcKind::Fortran) {
                Language::Fortran
            } else if classified.iter().any(|(_, k)| *k == SrcKind::R) {
                Language::R
            } else if classified.iter().any(|(_, k)| *k == SrcKind::Cpp) {
                Language::Cpp
            } else if classified.iter().any(|(_, k)| *k == SrcKind::Zig) {
                Language::Zig
            } else if classified.iter().any(|(_, k)| *k == SrcKind::Rust) {
                Language::Rust
            } else {
                Language::C
            }
        }
    };

    let profile = match manifest {
        Some(m) => m.profile(profile_name).map_err(|e| Error(e.0))?.clone(),
        None => embedide_workspace::Profile::default(),
    };

    let mut out = String::new();
    let _ = writeln!(out, "# Generated by EmbedIDE for project '{project_name}'");
    let _ = writeln!(
        out,
        "# Portable across GNU make (Linux) and BSD/GNU make (FreeBSD)."
    );
    let _ = writeln!(
        out,
        "# Override variables on the command line, e.g.: make CC=clang"
    );
    let _ = writeln!(out);

    // Rust (Cargo or standalone rustc)
    if language == Language::Rust {
        let sources: Vec<String> = classified
            .iter()
            .filter(|(_, k)| *k == SrcKind::Rust)
            .map(|(p, _)| rel_posix(root, p))
            .collect::<Result<_>>()?;
        if root.join("Cargo.toml").is_file() {
            let _ = writeln!(out, "CARGO ?= cargo");
            let _ = writeln!(out);
            let _ = writeln!(out, ".PHONY: all release test run clean help");
            let _ = writeln!(out);
            let _ = writeln!(out, "all:");
            let _ = writeln!(out, "\t$(CARGO) build");
            let _ = writeln!(out);
            let _ = writeln!(out, "release:");
            let _ = writeln!(out, "\t$(CARGO) build --release");
            let _ = writeln!(out);
            let _ = writeln!(out, "test:");
            let _ = writeln!(out, "\t$(CARGO) test");
            let _ = writeln!(out);
            let _ = writeln!(out, "run:");
            let _ = writeln!(out, "\t$(CARGO) run");
            let _ = writeln!(out);
            let _ = writeln!(out, "clean:");
            let _ = writeln!(out, "\t$(CARGO) clean");
        } else {
            let entry = sources
                .iter()
                .find(|s| s.ends_with("main.rs"))
                .cloned()
                .unwrap_or_else(|| "src/main.rs".into());
            let _ = writeln!(
                out,
                "RUSTC ?= {}",
                compiler_for(manifest, profile_name, Language::Rust, "rustc")
            );
            let _ = writeln!(out, "BUILD_DIR ?= {output_dir}/{profile_name}");
            let _ = writeln!(out, "TARGET := $(BUILD_DIR)/{artifact_name}");
            let _ = writeln!(out);
            let _ = writeln!(out, ".PHONY: all run clean help");
            let _ = writeln!(out);
            let _ = writeln!(out, "all: $(TARGET)");
            let _ = writeln!(out);
            let _ = writeln!(out, "$(TARGET): {entry}");
            let _ = writeln!(out, "\t@mkdir -p $(BUILD_DIR)");
            let _ = writeln!(out, "\t$(RUSTC) --edition 2021 -g {entry} -o $(TARGET)");
            let _ = writeln!(out);
            let _ = writeln!(out, "run: $(TARGET)");
            let _ = writeln!(out, "\t./$(TARGET)");
            let _ = writeln!(out);
            let _ = writeln!(out, "clean:");
            let _ = writeln!(out, "\trm -rf {output_dir}");
        }
        write_help(&mut out);
        return Ok((out, language, sources));
    }

    // Zig (build.zig or standalone zig build-exe)
    if language == Language::Zig {
        let sources: Vec<String> = classified
            .iter()
            .filter(|(_, k)| *k == SrcKind::Zig)
            .map(|(p, _)| rel_posix(root, p))
            .collect::<Result<_>>()?;
        let _ = writeln!(
            out,
            "ZIG ?= {}",
            compiler_for(manifest, profile_name, Language::Zig, "zig")
        );
        if root.join("build.zig").is_file() {
            let _ = writeln!(out);
            let _ = writeln!(out, ".PHONY: all run test clean help");
            let _ = writeln!(out);
            let _ = writeln!(out, "all:");
            let _ = writeln!(out, "\t$(ZIG) build");
            let _ = writeln!(out);
            let _ = writeln!(out, "run:");
            let _ = writeln!(out, "\t$(ZIG) build run");
            let _ = writeln!(out);
            let _ = writeln!(out, "test:");
            let _ = writeln!(out, "\t$(ZIG) build test");
            let _ = writeln!(out);
            let _ = writeln!(out, "clean:");
            let _ = writeln!(out, "\trm -rf zig-out .zig-cache");
        } else {
            let entry = sources
                .iter()
                .find(|s| s.ends_with("main.zig"))
                .cloned()
                .unwrap_or_else(|| "src/main.zig".into());
            let opt = if profile_name == "release" {
                "ReleaseSafe"
            } else {
                "Debug"
            };
            let _ = writeln!(out, "BUILD_DIR ?= {output_dir}/{profile_name}");
            let _ = writeln!(out, "TARGET := $(BUILD_DIR)/{artifact_name}");
            let _ = writeln!(out, "ZIG_OPT ?= {opt}");
            let _ = writeln!(out);
            let _ = writeln!(out, ".PHONY: all run clean help");
            let _ = writeln!(out);
            let _ = writeln!(out, "all: $(TARGET)");
            let _ = writeln!(out);
            let _ = writeln!(out, "$(TARGET): {entry}");
            let _ = writeln!(out, "\t@mkdir -p $(BUILD_DIR)");
            let _ = writeln!(
                out,
                "\t$(ZIG) build-exe {entry} -O $(ZIG_OPT) -femit-bin=$(TARGET) --cache-dir $(BUILD_DIR)/zig-cache"
            );
            let _ = writeln!(out);
            let _ = writeln!(out, "run: $(TARGET)");
            let _ = writeln!(out, "\t./$(TARGET)");
            let _ = writeln!(out);
            let _ = writeln!(out, "clean:");
            let _ = writeln!(out, "\trm -rf {output_dir}");
        }
        write_help(&mut out);
        return Ok((out, language, sources));
    }

    // R (Rscript syntax-check & run)
    if language == Language::R {
        let sources: Vec<String> = classified
            .iter()
            .filter(|(_, k)| *k == SrcKind::R)
            .map(|(p, _)| rel_posix(root, p))
            .collect::<Result<_>>()?;
        if sources.is_empty() {
            return Err(Error(
                "No R (.R/.r) sources found to generate a Makefile".into(),
            ));
        }
        let entry = sources
            .iter()
            .find(|s| s.to_ascii_lowercase().ends_with("main.r"))
            .cloned()
            .unwrap_or_else(|| sources[0].clone());
        let _ = writeln!(out, "RSCRIPT ?= Rscript");
        let _ = writeln!(out, "ENTRY ?= {entry}");
        let _ = writeln!(out);
        let _ = writeln!(out, ".PHONY: all check run clean help");
        let _ = writeln!(out);
        let _ = writeln!(out, "all: check");
        let _ = writeln!(out);
        let _ = writeln!(out, "check:");
        for src in &sources {
            let _ = writeln!(
                out,
                "\t$(RSCRIPT) --vanilla -e \"invisible(parse(file=commandArgs(trailingOnly=TRUE)[1]))\" --args {src}"
            );
        }
        let _ = writeln!(out);
        let _ = writeln!(out, "run:");
        let _ = writeln!(out, "\t$(RSCRIPT) --vanilla $(ENTRY)");
        let _ = writeln!(out);
        let _ = writeln!(out, "clean:");
        let _ = writeln!(out, "\trm -rf {output_dir}");
        write_help(&mut out);
        return Ok((out, language, sources));
    }

    // Native compiled languages: C, C++, Assembly, Fortran (including mixed builds)
    let compiled: Vec<(PathBuf, SrcKind)> = classified
        .into_iter()
        .filter(|(_, k)| {
            matches!(
                k,
                SrcKind::C | SrcKind::Cpp | SrcKind::Gas | SrcKind::Nasm | SrcKind::Fortran
            )
        })
        .collect();
    if compiled.is_empty() {
        return Err(Error(
            "No C, C++, assembly or Fortran source files found to generate a Makefile".into(),
        ));
    }

    // Order Fortran sources topologically and collect inter-object module dependencies.
    let include_dirs: Vec<PathBuf> = manifest
        .map(|m| m.build.include.iter().map(|i| root.join(i)).collect())
        .unwrap_or_default();
    let (fort, mut ordered): (Vec<_>, Vec<_>) = compiled
        .into_iter()
        .partition(|(_, k)| *k == SrcKind::Fortran);
    let mut fort_obj_deps: BTreeMap<PathBuf, Vec<String>> = BTreeMap::new();
    if !fort.is_empty() {
        let mut units = Vec::with_capacity(fort.len());
        for (p, _) in &fort {
            let bytes =
                std::fs::read(p).map_err(|e| Error(format!("Cannot read {}: {e}", p.display())))?;
            units.push(fortran::Unit {
                path: p.clone(),
                scan: fortran::scan(&String::from_utf8_lossy(&bytes), fortran::is_fixed_form(p)),
            });
        }
        let plan = fortran::plan(&units, &include_dirs)?;
        for (u, dep_paths) in units.iter().zip(plan.deps) {
            let mut dep_objs = Vec::new();
            for dp in dep_paths {
                if fort.iter().any(|(fp, _)| fp == &dp) {
                    let rel = rel_posix(root, &dp)?;
                    dep_objs.push(format!("$(OBJ_DIR)/{}", obj_name(&rel)));
                } else if dp.starts_with(root) {
                    dep_objs.push(rel_posix(root, &dp)?);
                }
            }
            fort_obj_deps.insert(u.path.clone(), dep_objs);
        }
        ordered.extend(plan.order.into_iter().map(|i| fort[i].clone()));
    }

    let needed: BTreeSet<SrcKind> = ordered.iter().map(|(_, k)| *k).collect();
    let mut common_flags: Vec<String> = Vec::new();
    let mut link_flags: Vec<String> = Vec::new();
    let mut cpp_flags: Vec<String> = Vec::new();
    let mut libs: Vec<String> = Vec::new();
    if let Some(m) = manifest {
        common_flags.extend(m.target.flags.iter().cloned());
        common_flags.extend(m.build.flags.iter().cloned());
        common_flags.extend(profile.flags.iter().cloned());
        link_flags.extend(m.target.flags.iter().cloned());
        link_flags.extend(m.build.link_flags.iter().cloned());
        link_flags.extend(profile.link_flags.iter().cloned());
        if let Some(ls) = &m.build.linker_script {
            check_make_token(ls, "linker script")?;
            link_flags.push(format!("-T{ls}"));
        }
        for inc in &m.build.include {
            check_make_token(inc, "include directory")?;
            cpp_flags.push(format!("-I{inc}"));
        }
        for def in m.build.defines.iter().chain(&profile.defines) {
            check_make_token(def, "define")?;
            cpp_flags.push(format!("-D{def}"));
        }
        for lib in &m.build.libs {
            check_make_token(lib, "library")?;
            libs.push(format!("-l{lib}"));
        }
    } else {
        common_flags.extend(["-Wall".into(), "-Wextra".into(), "-g".into(), "-O0".into()]);
    }

    let flags_str = join_flags(&common_flags, "compiler flag")?;
    let cppflags_str = join_flags(&cpp_flags, "preprocessor flag")?;
    let ldflags_str = join_flags(&link_flags, "linker flag")?;
    let ldlibs_str = join_flags(&libs, "library flag")?;

    let cc = compiler_for(manifest, profile_name, Language::C, "cc");
    let cxx = compiler_for(manifest, profile_name, Language::Cpp, "c++");
    let fc = compiler_for(manifest, profile_name, Language::Fortran, "gfortran");
    let nasm = compiler_for(manifest, profile_name, Language::Asm, "nasm");

    if needed.contains(&SrcKind::C) || needed.contains(&SrcKind::Gas) {
        let _ = writeln!(out, "CC = {cc}");
    }
    if needed.contains(&SrcKind::Cpp) {
        let _ = writeln!(out, "CXX = {cxx}");
    }
    if needed.contains(&SrcKind::Fortran) {
        let _ = writeln!(out, "FC = {fc}");
    }
    if needed.contains(&SrcKind::Nasm) {
        let _ = writeln!(out, "NASM ?= {nasm}");
    }

    let linker_var = if needed.contains(&SrcKind::Fortran) {
        "$(FC)"
    } else if needed.contains(&SrcKind::Cpp) {
        "$(CXX)"
    } else {
        "$(CC)"
    };
    if linker_var == "$(CC)" && !needed.contains(&SrcKind::C) && !needed.contains(&SrcKind::Gas) {
        let _ = writeln!(out, "CC = {cc}");
    }

    let _ = writeln!(out, "BUILD_DIR ?= {output_dir}/{profile_name}");
    let _ = writeln!(out, "OBJ_DIR := $(BUILD_DIR)/obj");
    if needed.contains(&SrcKind::Fortran) {
        let _ = writeln!(out, "MOD_DIR := $(BUILD_DIR)/mod");
    }
    let _ = writeln!(out, "TARGET := $(BUILD_DIR)/{artifact_name}");
    if !cppflags_str.is_empty() {
        let _ = writeln!(out, "CPPFLAGS ?= {cppflags_str}");
    } else {
        let _ = writeln!(out, "CPPFLAGS ?=");
    }
    if needed.contains(&SrcKind::C) || needed.contains(&SrcKind::Gas) {
        let _ = writeln!(out, "CFLAGS ?= {flags_str}");
    }
    if needed.contains(&SrcKind::Cpp) {
        let _ = writeln!(out, "CXXFLAGS ?= {flags_str}");
    }
    if needed.contains(&SrcKind::Fortran) {
        let _ = writeln!(out, "FFLAGS ?= {flags_str}");
    }
    let _ = writeln!(out, "LDFLAGS ?= {ldflags_str}");
    let _ = writeln!(out, "LDLIBS ?= {ldlibs_str}");
    let _ = writeln!(out);

    let mut rel_sources = Vec::with_capacity(ordered.len());
    let mut obj_targets = Vec::with_capacity(ordered.len());
    for (src, _) in &ordered {
        let rel = rel_posix(root, src)?;
        obj_targets.push(format!("$(OBJ_DIR)/{}", obj_name(&rel)));
        rel_sources.push(rel);
    }

    let _ = writeln!(out, "OBJS := {}", obj_targets.join(" "));
    let _ = writeln!(out);
    let _ = writeln!(out, ".PHONY: all run clean help");
    let _ = writeln!(out);
    let _ = writeln!(out, "all: $(TARGET)");
    let _ = writeln!(out);
    let _ = writeln!(out, "$(TARGET): $(OBJS)");
    let _ = writeln!(out, "\t@mkdir -p $(BUILD_DIR)");
    let _ = writeln!(
        out,
        "\t{linker_var} $(OBJS) $(LDFLAGS) $(LDLIBS) -o $(TARGET)"
    );
    let _ = writeln!(out);

    let mod_flag = if fc.contains("flang") {
        "-module-dir $(MOD_DIR)"
    } else {
        "-J $(MOD_DIR)"
    };
    let has_cpp_flag = common_flags.iter().any(|f| f == "-cpp" || f == "-fpp");

    for ((src, kind), (rel, obj)) in ordered
        .iter()
        .zip(rel_sources.iter().zip(obj_targets.iter()))
    {
        let extra_deps = fort_obj_deps
            .get(src)
            .filter(|v| !v.is_empty())
            .map(|v| format!(" {}", v.join(" ")))
            .unwrap_or_default();
        let _ = writeln!(out, "{obj}: {rel}{extra_deps}");
        if *kind == SrcKind::Fortran {
            let _ = writeln!(out, "\t@mkdir -p $(OBJ_DIR) $(MOD_DIR)");
            let cpp = if fortran::needs_cpp(src) && !has_cpp_flag {
                " -cpp"
            } else {
                ""
            };
            let _ = writeln!(
                out,
                "\t$(FC) $(CPPFLAGS) $(FFLAGS){cpp} {mod_flag} -c {rel} -o {obj}"
            );
        } else {
            let _ = writeln!(out, "\t@mkdir -p $(OBJ_DIR)");
            match kind {
                SrcKind::C | SrcKind::Gas => {
                    let _ = writeln!(out, "\t$(CC) $(CPPFLAGS) $(CFLAGS) -c {rel} -o {obj}");
                }
                SrcKind::Cpp => {
                    let _ = writeln!(out, "\t$(CXX) $(CPPFLAGS) $(CXXFLAGS) -c {rel} -o {obj}");
                }
                SrcKind::Nasm => {
                    let _ = writeln!(out, "\t$(NASM) -f elf64 {rel} -o {obj}");
                }
                SrcKind::Fortran | SrcKind::Rust | SrcKind::Zig | SrcKind::R => unreachable!(),
            }
        }
        let _ = writeln!(out);
    }

    let _ = writeln!(out, "run: $(TARGET)");
    let _ = writeln!(out, "\t./$(TARGET)");
    let _ = writeln!(out);
    let _ = writeln!(out, "clean:");
    let _ = writeln!(out, "\trm -rf {output_dir}");
    write_help(&mut out);

    Ok((out, language, rel_sources))
}

fn write_help(out: &mut String) {
    let _ = writeln!(out);
    let _ = writeln!(out, "help:");
    let _ = writeln!(out, "\t@echo \"Targets: all, run, clean, help\"");
}

/// Generate and write a `Makefile` inside `root`.
///
/// Refuses to overwrite an existing file unless `opts.force` is true, and
/// rejects output paths that escape `root`.
pub fn write(
    root: &Path,
    manifest: Option<&Manifest>,
    opts: &MakefileOptions,
) -> Result<GeneratedMakefile> {
    if opts.output.is_absolute()
        || opts.output.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(Error(format!(
            "Makefile output path '{}' must stay inside the project root",
            opts.output.display()
        )));
    }
    let path = root.join(&opts.output);
    let existed = path.exists();
    if existed && !opts.force {
        return Err(Error(format!(
            "{} already exists; pass --force (or :makefile! in TUI) to replace it",
            path.display()
        )));
    }
    let (content, language, sources) = render(root, manifest, &opts.profile)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if opts.force {
        let tmp = path.with_extension("embedide.tmp");
        std::fs::write(&tmp, content.as_bytes())?;
        std::fs::rename(&tmp, &path)?;
    } else {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| Error(format!("Cannot create {}: {e}", path.display())))?;
        f.write_all(content.as_bytes())?;
    }
    Ok(GeneratedMakefile {
        path,
        language,
        sources,
        overwritten: existed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use embedide_workspace::BuildKind;
    use std::process::Command;

    #[test]
    fn generates_and_builds_c_and_fortran_makefiles_with_real_make() {
        let dir = std::env::temp_dir().join(format!("embedide-mk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("src/main.c"),
            "#include <stdio.h>\nint main(void){ puts(\"mk-ok\"); return 0; }\n",
        )
        .unwrap();
        let m = Manifest::new("mkdemo", Language::C, BuildKind::Native);
        let gen = write(&dir, Some(&m), &MakefileOptions::default()).unwrap();
        assert!(!gen.overwritten);
        assert_eq!(gen.language, Language::C);
        assert_eq!(gen.sources, vec!["src/main.c"]);

        // Refuses overwrite without force.
        assert!(write(&dir, Some(&m), &MakefileOptions::default()).is_err());
        let forced = write(
            &dir,
            Some(&m),
            &MakefileOptions {
                force: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(forced.overwritten);

        // If `make` and `cc` are installed on the host, build and run the generated Makefile!
        if Command::new("make").arg("--version").output().is_ok()
            && Command::new("cc").arg("--version").output().is_ok()
        {
            let status = Command::new("make")
                .current_dir(&dir)
                .status()
                .expect("run make");
            assert!(status.success(), "generated C Makefile failed to build");
            let run = Command::new(dir.join("build/debug/mkdemo"))
                .output()
                .expect("run built binary");
            assert!(run.status.success());
            assert!(String::from_utf8_lossy(&run.stdout).contains("mk-ok"));
        }

        // Fortran multi-module project: verify dependency edge in Makefile and build with gfortran.
        let fdir = dir.join("fort");
        std::fs::create_dir_all(fdir.join("src")).unwrap();
        std::fs::write(
            fdir.join("src/a_main.f90"),
            "program main\n  use z_mod\n  print *, answer()\nend program\n",
        )
        .unwrap();
        std::fs::write(
            fdir.join("src/z_mod.f90"),
            "module z_mod\ncontains\n  integer function answer()\n    answer = 42\n  end function\nend module\n",
        )
        .unwrap();
        let fm = Manifest::new("fmk", Language::Fortran, BuildKind::Native);
        let fgen = write(&fdir, Some(&fm), &MakefileOptions::default()).unwrap();
        let mk_text = std::fs::read_to_string(&fgen.path).unwrap();
        assert!(
            mk_text.contains(
                "$(OBJ_DIR)/src__a_main.f90.o: src/a_main.f90 $(OBJ_DIR)/src__z_mod.f90.o"
            ),
            "Fortran module dependency edge missing:\n{mk_text}"
        );
        if Command::new("make").arg("--version").output().is_ok()
            && Command::new("gfortran").arg("--version").output().is_ok()
        {
            let status = Command::new("make")
                .current_dir(&fdir)
                .status()
                .expect("run make for fortran");
            assert!(
                status.success(),
                "generated Fortran Makefile failed to build"
            );
            let run = Command::new(fdir.join("build/debug/fmk"))
                .output()
                .expect("run built fortran binary");
            assert!(run.status.success());
            assert!(String::from_utf8_lossy(&run.stdout).contains("42"));
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_path_traversal_and_generates_r_makefile() {
        let dir = std::env::temp_dir().join(format!("embedide-mk-r-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/main.R"), "cat('hi\\n')\n").unwrap();
        let bad = MakefileOptions {
            output: PathBuf::from("../Makefile"),
            ..Default::default()
        };
        assert!(write(&dir, None, &bad).is_err());

        let gen = write(&dir, None, &MakefileOptions::default()).unwrap();
        assert_eq!(gen.language, Language::R);
        let text = std::fs::read_to_string(&gen.path).unwrap();
        assert!(text.contains("RSCRIPT ?= Rscript"));
        assert!(text.contains("parse(file=commandArgs(trailingOnly=TRUE)[1])"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
