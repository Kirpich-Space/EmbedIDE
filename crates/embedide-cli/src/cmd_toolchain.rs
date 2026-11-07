//! `embedide toolchain …`
use crate::{fail, Out};
use clap::Subcommand;
use embedide_toolchains::{self as tc, CustomToolchain, DetectOptions, Language, Toolchain};
use embedide_workspace::{ConfigFile, UserConfig};
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum ToolchainCmd {
    /// List installed and configured toolchains with versions and targets.
    List {
        /// Only toolchains for this language (c, cpp, rust, zig, asm, fortran).
        #[arg(long)]
        language: Option<Language>,
        /// Do not run version queries (path lookup only).
        #[arg(long)]
        no_probe: bool,
    },
    /// Same as `list`, including tools that failed their version query.
    Detect,
    /// Details for one toolchain id (gcc, clang, zig-cc, arm-none-eabi-gcc, …).
    Info { id: String },
    /// Register your own compiler executable (paths with spaces are fine).
    Add {
        /// Your id for it, e.g. gcc-13 or my-arm-gcc.
        id: String,
        /// Kind: gcc, g++, clang, clang++, zig-cc, zig-c++, rustc, cargo, zig, gas, nasm, gfortran, flang, lfortran.
        #[arg(long)]
        kind: String,
        /// Absolute executable path.
        #[arg(long)]
        path: PathBuf,
    },
    /// Remove a toolchain registered with `add`.
    Remove { id: String },
    /// Set your default toolchain for a language (used when a project does not choose).
    Default { language: Language, id: String },
    /// Add a directory searched after PATH (e.g. a vendor toolchain bin/).
    SearchDir { dir: PathBuf },
    /// Print installation suggestions. Nothing is downloaded or installed.
    Recommend { language: Option<Language> },
}

/// Detected toolchains using the user's configuration.
pub fn registry(user: &UserConfig, probe: bool) -> Vec<Toolchain> {
    tc::detect(&DetectOptions {
        search_dirs: user.toolchains.search_dirs.clone(),
        custom: user.toolchains.custom.clone(),
        probe,
    })
}

fn row(t: &Toolchain) -> Value {
    json!({"id": t.id, "kind": t.kind, "languages": t.languages, "program": t.program, "prefix_args": t.prefix_args,
        "version": t.version, "target": t.target, "cross": t.cross, "origin": t.origin, "problem": t.problem})
}

fn table(out: &Out, list: &[Toolchain], defaults: &std::collections::BTreeMap<String, String>) {
    if list.is_empty() {
        out.line("No toolchains found. See `embedide toolchain recommend`.");
        return;
    }
    out.line(&format!(
        "{:<24} {:<9} {:<14} {:<6} {}",
        "ID", "KIND", "LANGUAGES", "TARGET", "VERSION / PATH"
    ));
    for t in list {
        let langs: Vec<_> = t.languages.iter().map(|l| l.id()).collect();
        let star = if defaults.values().any(|d| d == &t.id) {
            "*"
        } else {
            ""
        };
        let target = if t.cross { "cross" } else { "host" };
        let ver = t
            .problem
            .clone()
            .map(|p| format!("! {p}"))
            .or_else(|| t.version.clone())
            .unwrap_or_default();
        out.line(&format!(
            "{:<24} {:<9} {:<14} {:<6} {}",
            format!("{}{star}", t.id),
            t.kind,
            langs.join(","),
            target,
            ver
        ));
        out.line(&format!(
            "{:<24} {}{}",
            "",
            t.program.display(),
            t.target
                .as_deref()
                .map(|x| format!("  [{x}]"))
                .unwrap_or_default()
        ));
    }
    if !defaults.is_empty() {
        out.line("* = your default for a language");
    }
}

fn save_user(f: &ConfigFile) -> crate::CliResult<()> {
    f.save(|t| UserConfig::parse(t).map(|_| ()))
        .map_err(|e| fail(e.0))
}

pub fn run(cmd: ToolchainCmd, out: &Out) -> crate::CliResult<Value> {
    let user = UserConfig::load().map_err(|e| fail(e.0))?;
    match cmd {
        ToolchainCmd::List { language, no_probe } => {
            let list: Vec<_> = registry(&user, !no_probe)
                .into_iter()
                .filter(|t| language.is_none_or(|l| t.supports(l)))
                .collect();
            table(out, &list, &user.toolchains.defaults);
            Ok(
                json!({"toolchains": list.iter().map(row).collect::<Vec<_>>(), "defaults": user.toolchains.defaults}),
            )
        }
        ToolchainCmd::Detect => {
            let list = registry(&user, true);
            table(out, &list, &user.toolchains.defaults);
            let missing: Vec<_> = Language::ALL
                .iter()
                .filter(|l| !list.iter().any(|t| t.supports(**l) && t.problem.is_none()))
                .map(|l| l.id())
                .collect();
            if !missing.is_empty() {
                out.line(&format!(
                    "No toolchain for: {} (see `embedide toolchain recommend <language>`)",
                    missing.join(", ")
                ));
            }
            Ok(
                json!({"toolchains": list.iter().map(row).collect::<Vec<_>>(), "missing_languages": missing}),
            )
        }
        ToolchainCmd::Info { id } => {
            let list = registry(&user, true);
            let t = list.iter().find(|t| t.id == id).ok_or_else(|| {
                let known = tc::kind(&id)
                    .map(|k| {
                        format!(
                            " ('{}' is a known kind, but no such executable was found)",
                            k.id
                        )
                    })
                    .unwrap_or_default();
                fail(format!(
                    "No toolchain '{id}'{known}. Installed: {}",
                    list.iter()
                        .map(|t| t.id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })?;
            out.line(&format!("id:        {}", t.id));
            out.line(&format!("kind:      {} ({:?} driver)", t.kind, t.family));
            out.line(&format!(
                "languages: {}",
                t.languages
                    .iter()
                    .map(|l| l.id())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            out.line(&format!(
                "program:   {}{}",
                t.program.display(),
                if t.prefix_args.is_empty() {
                    String::new()
                } else {
                    format!(" {}", t.prefix_args.join(" "))
                }
            ));
            out.line(&format!(
                "version:   {}",
                t.version.as_deref().unwrap_or("?")
            ));
            out.line(&format!(
                "target:    {}{}",
                t.target.as_deref().unwrap_or("(not reported)"),
                if t.cross { " (cross)" } else { "" }
            ));
            out.line(&format!("origin:    {:?}", t.origin));
            if let Some(p) = &t.problem {
                out.line(&format!("problem:   {p}"));
            }
            Ok(row(t))
        }
        ToolchainCmd::Add { id, kind, path } => {
            let custom = CustomToolchain {
                id: id.clone(),
                kind,
                path,
            };
            tc::validate_custom(&custom).map_err(fail)?;
            if user.toolchains.custom.iter().any(|c| c.id == id) {
                return Err(fail(format!(
                    "Toolchain '{id}' already exists; remove it first"
                )));
            }
            let mut probe = registry(
                &UserConfig {
                    toolchains: embedide_workspace::config::ToolchainsConfig {
                        custom: vec![custom.clone()],
                        ..Default::default()
                    },
                    ..Default::default()
                },
                false,
            )
            .into_iter()
            .find(|t| t.id == id)
            .ok_or_else(|| fail("Cannot register toolchain".into()))?;
            tc::probe(&mut probe);
            if let Some(p) = &probe.problem {
                return Err(fail(format!(
                    "{} did not answer a version query: {p}",
                    custom.path.display()
                )));
            }
            let mut f = ConfigFile::open(&UserConfig::path().map_err(|e| fail(e.0))?)
                .map_err(|e| fail(e.0))?;
            let mut list = user.toolchains.custom.clone();
            list.push(custom);
            let value = toml_array(&list);
            f.set("toolchains.custom", &value).map_err(|e| fail(e.0))?;
            save_user(&f)?;
            out.line(&format!(
                "Added {id}: {}",
                probe.version.as_deref().unwrap_or("")
            ));
            Ok(row(&probe))
        }
        ToolchainCmd::Remove { id } => {
            let list: Vec<_> = user
                .toolchains
                .custom
                .iter()
                .filter(|c| c.id != id)
                .cloned()
                .collect();
            if list.len() == user.toolchains.custom.len() {
                return Err(fail(format!("No custom toolchain '{id}' (only toolchains added with `toolchain add` can be removed)")));
            }
            let mut f = ConfigFile::open(&UserConfig::path().map_err(|e| fail(e.0))?)
                .map_err(|e| fail(e.0))?;
            f.set("toolchains.custom", &toml_array(&list))
                .map_err(|e| fail(e.0))?;
            save_user(&f)?;
            out.line(&format!("Removed {id}"));
            Ok(json!({"removed": id}))
        }
        ToolchainCmd::Default { language, id } => {
            let list = registry(&user, false);
            let t = list
                .iter()
                .find(|t| t.id == id)
                .ok_or_else(|| fail(format!("No toolchain '{id}' installed or configured")))?;
            if !t.supports(language) {
                return Err(fail(format!(
                    "'{id}' ({}) does not compile {language}",
                    t.kind
                )));
            }
            let mut f = ConfigFile::open(&UserConfig::path().map_err(|e| fail(e.0))?)
                .map_err(|e| fail(e.0))?;
            f.set(
                &format!("toolchains.defaults.{}", language.id()),
                &format!("\"{id}\""),
            )
            .map_err(|e| fail(e.0))?;
            save_user(&f)?;
            out.line(&format!("Default {language} toolchain: {id}"));
            Ok(json!({"language": language, "id": id}))
        }
        ToolchainCmd::SearchDir { dir } => {
            if !dir.is_absolute() || !dir.is_dir() {
                return Err(fail(format!(
                    "{} must be an existing absolute directory",
                    dir.display()
                )));
            }
            let mut dirs = user.toolchains.search_dirs.clone();
            if !dirs.contains(&dir) {
                dirs.push(dir.clone());
            }
            let mut f = ConfigFile::open(&UserConfig::path().map_err(|e| fail(e.0))?)
                .map_err(|e| fail(e.0))?;
            let arr = format!(
                "[{}]",
                dirs.iter()
                    .map(|d| toml_str(&d.to_string_lossy()))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            f.set("toolchains.search_dirs", &arr)
                .map_err(|e| fail(e.0))?;
            save_user(&f)?;
            out.line(&format!("Search directory added: {}", dir.display()));
            Ok(json!({"search_dirs": dirs}))
        }
        ToolchainCmd::Recommend { language } => {
            let family = tc::package_family();
            let langs = language
                .map(|l| vec![l])
                .unwrap_or_else(|| Language::ALL.to_vec());
            let installed = registry(&user, false);
            let mut all = Vec::new();
            out.line(&format!(
                "Suggestions for package manager '{family}'. EmbedIDE does not run these commands."
            ));
            for l in langs {
                for r in tc::recommend(l, family) {
                    let have = installed.iter().any(|t| t.id == r.kind || t.kind == r.kind);
                    out.line(&format!(
                        "  {:<8} {:<18} {} {}",
                        l.id(),
                        r.kind,
                        if have { "[installed]" } else { "" },
                        r.command.as_deref().unwrap_or(r.note)
                    ));
                    all.push(json!({"language": l, "kind": r.kind, "installed": have, "command": r.command, "note": r.note}));
                }
            }
            Ok(json!({"package_manager": family, "recommendations": all, "executed": false}))
        }
    }
}

fn toml_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_default()
}

fn toml_array(list: &[CustomToolchain]) -> String {
    let items: Vec<_> = list
        .iter()
        .map(|c| {
            format!(
                "{{ id = {}, kind = {}, path = {} }}",
                toml_str(&c.id),
                toml_str(&c.kind),
                toml_str(&c.path.to_string_lossy())
            )
        })
        .collect();
    format!("[{}]", items.join(", "))
}
