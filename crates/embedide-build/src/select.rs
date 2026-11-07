//! Toolchain selection: the user's choice wins; automatic choice only for
//! plain host builds, and never silently for a cross target.
use crate::{Error, Request, Result};
use embedide_toolchains::{Family, Language, Toolchain};
use embedide_workspace::{Manifest, UserConfig};
use serde::Serialize;
use std::path::PathBuf;

/// The toolchain used for one language.
#[derive(Debug, Clone, Serialize)]
pub struct Selection {
    /// Language.
    pub language: Language,
    /// Toolchain id.
    pub id: String,
    /// Kind id.
    pub kind: &'static str,
    /// Driver family.
    pub family: Family,
    /// Executable.
    pub program: PathBuf,
    /// Leading arguments (`cc` for `zig cc`).
    pub prefix_args: Vec<String>,
    /// Version line, if probed.
    pub version: Option<String>,
    /// The compiler's default target.
    pub default_target: Option<String>,
    /// Why it was chosen.
    pub reason: &'static str,
}

/// The explicit choice for a language, in priority order:
/// command line → profile → project → user default.
pub fn explicit_choice(
    m: Option<&Manifest>,
    user: &UserConfig,
    req: &Request,
    lang: Language,
) -> Option<String> {
    req.toolchain
        .get(lang.id())
        .cloned()
        .or_else(|| m.and_then(|m| m.toolchain_for(&req.profile, lang)))
        .or_else(|| user.toolchains.defaults.get(lang.id()).cloned())
}

fn reason_of(m: Option<&Manifest>, req: &Request, lang: Language) -> &'static str {
    if req.toolchain.contains_key(lang.id()) {
        "command line"
    } else if m
        .and_then(|m| m.profiles.get(&req.profile))
        .is_some_and(|p| p.toolchain.contains_key(lang.id()))
    {
        "profile"
    } else if m.is_some_and(|m| m.toolchain.contains_key(lang.id())) {
        "project"
    } else {
        "user default"
    }
}

/// Resolve a toolchain id for a language and an optional target triple.
pub fn resolve(
    toolchains: &[Toolchain],
    id: &str,
    lang: Language,
    triple: Option<&str>,
) -> Result<Selection> {
    let Some(tc) = toolchains.iter().find(|t| t.id == id) else {
        let options: Vec<_> = toolchains
            .iter()
            .filter(|t| t.supports(lang))
            .map(|t| t.id.as_str())
            .collect();
        return Err(Error(format!(
            "Toolchain '{id}' is not installed or not configured. Available for {lang}: {}. See `embedide toolchain recommend {lang}`; EmbedIDE does not install compilers",
            if options.is_empty() { "none".to_string() } else { options.join(", ") }
        )));
    };
    if !tc.supports(lang) {
        let langs: Vec<_> = tc.languages.iter().map(|l| l.id()).collect();
        return Err(Error(format!(
            "Toolchain '{id}' ({}) compiles {}, not {lang}",
            tc.kind,
            langs.join("/")
        )));
    }
    if let Some(problem) = &tc.problem {
        return Err(Error(format!("Toolchain '{id}' is not usable: {problem}")));
    }
    if let Some(triple) = triple {
        match tc.family {
            // GCC binaries are built for one target; they cannot retarget with a flag
            Family::Gcc | Family::Gas => {
                let own = tc.target.as_deref().unwrap_or("");
                if !own.is_empty() && !own.starts_with(triple) && !triple.starts_with(own) {
                    return Err(Error(format!(
                        "'{id}' targets {own}, but the project targets {triple}. GCC cannot switch targets: choose a {triple}-prefixed GCC, clang or zig-cc"
                    )));
                }
            }
            Family::Lfortran => {
                return Err(Error(format!("LFortran cross-compilation is not supported by EmbedIDE; target {triple} needs gfortran or flang")));
            }
            Family::Nasm | Family::Clang | Family::ZigCc | Family::Rust | Family::Zig => {}
        }
    }
    Ok(Selection {
        language: lang,
        id: tc.id.clone(),
        kind: tc.kind,
        family: tc.family,
        program: tc.program.clone(),
        prefix_args: tc.prefix_args.clone(),
        version: tc.version.clone(),
        default_target: tc.target.clone(),
        reason: "explicit",
    })
}

const HOST_PREFERENCE: &[(Language, &[&str])] = &[
    (Language::C, &["gcc", "clang", "zig-cc"]),
    (Language::Cpp, &["g++", "clang++", "zig-c++"]),
    (Language::Asm, &["gcc", "clang", "zig-cc"]),
    (Language::Fortran, &["gfortran", "flang", "lfortran"]),
    (Language::Zig, &["zig"]),
    (Language::Rust, &["rustc"]),
];

/// A board project needs a cross GCC; a host GCC would only fail later on -mcpu.
fn check_board(m: &Manifest, toolchains: &[Toolchain], s: &Selection) -> Result<()> {
    let Some(board) = &m.target.board else {
        return Ok(());
    };
    let host_gcc = matches!(s.family, Family::Gcc | Family::Gas)
        && toolchains.iter().any(|t| t.id == s.id && !t.cross);
    if host_gcc {
        return Err(Error(format!(
            "'{}' is a host compiler ({}), but this project targets board {board}. Choose a cross compiler such as arm-none-eabi-gcc",
            s.id,
            s.default_target.as_deref().unwrap_or("host")
        )));
    }
    Ok(())
}

/// Choose the toolchain for a language in a manifest build.
pub fn choose(
    m: &Manifest,
    user: &UserConfig,
    req: &Request,
    toolchains: &[Toolchain],
    lang: Language,
) -> Result<Selection> {
    let triple = m.target.triple.as_deref();
    if let Some(id) = explicit_choice(Some(m), user, req, lang) {
        let mut s = resolve(toolchains, &id, lang, triple)?;
        check_board(m, toolchains, &s)?;
        s.reason = reason_of(Some(m), req, lang);
        return Ok(s);
    }
    // Assembly in .S files goes through the C driver the user picked for C
    if lang == Language::Asm {
        if let Some(id) = explicit_choice(Some(m), user, req, Language::C) {
            if let Ok(mut s) = resolve(toolchains, &id, Language::Asm, triple) {
                s.reason = "same as C";
                return Ok(s);
            }
        }
    }
    if triple.is_some() || !m.target.flags.is_empty() {
        return Err(Error(format!(
            "This project has a [target]; choose its {lang} toolchain explicitly (e.g. `embedide build --toolchain {lang}=<id>` or [toolchain] {lang} = \"…\"). Installed: {}",
            toolchains.iter().filter(|t| t.supports(lang)).map(|t| t.id.as_str()).collect::<Vec<_>>().join(", ")
        )));
    }
    let prefs = HOST_PREFERENCE
        .iter()
        .find(|(l, _)| *l == lang)
        .map(|(_, p)| *p)
        .unwrap_or(&[]);
    for id in prefs {
        if let Some(tc) = toolchains
            .iter()
            .find(|t| t.id == *id && t.problem.is_none() && !t.cross)
        {
            let mut s = resolve(std::slice::from_ref(tc), id, lang, None)?;
            s.reason = "first installed";
            return Ok(s);
        }
    }
    Err(Error(format!(
        "No {lang} toolchain found. Install one (see `embedide toolchain recommend {lang}`) or add yours with `embedide toolchain add`"
    )))
}
