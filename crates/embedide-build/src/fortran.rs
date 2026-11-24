//! Fortran module dependency scanning.
//!
//! Fortran compilers write one `.mod` file per module, and a `use` statement needs it to exist, so
//! files must be compiled in dependency order. Compilers do not report this ahead of time (and
//! `gfortran -MMD` needs `-cpp`), so EmbedIDE reads the source itself: it finds `module`,
//! `submodule`, `use` and `include` statements and sorts the files topologically.

use crate::{Error, Result};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

/// What one Fortran source file defines and needs.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Scan {
    /// Lower-case names of modules and submodules defined in the file.
    pub provides: Vec<String>,
    /// Lower-case names of modules (and parent modules of submodules) the file needs.
    pub uses: Vec<String>,
    /// Files named by `include '...'` statements.
    pub includes: Vec<String>,
}

/// Source form of a file: free-form, or fixed-form (`.f`, `.for`, `.ftn` and their upper-case variants).
pub(crate) fn is_fixed_form(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("f" | "F" | "for" | "FOR" | "ftn" | "FTN")
    )
}

/// Whether the extension conventionally enables the C preprocessor in Fortran (`-cpp`).
pub(crate) fn needs_cpp(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("F" | "FOR" | "FTN" | "F90" | "F95" | "F03" | "F08" | "F18" | "fpp" | "FPP")
    )
}

/// Remove a trailing `!` comment, leaving `!` inside character literals alone.
fn strip_comment(line: &str) -> &str {
    let mut quote: Option<char> = None;
    for (i, c) in line.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (None, '\'' | '"') => quote = Some(c),
            (None, '!') => return &line[..i],
            _ => {}
        }
    }
    line
}

/// Split a line into statements at `;` outside character literals.
fn statements(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut quote: Option<char> = None;
    let mut start = 0;
    for (i, c) in line.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (None, '\'' | '"') => quote = Some(c),
            (None, ';') => {
                out.push(&line[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&line[start..]);
    out
}

/// Leading identifier of `s` (letters, digits, `_`), lower-cased.
fn ident(s: &str) -> Option<(String, &str)> {
    let end = s
        .char_indices()
        .find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '_'))
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    (end > 0).then(|| (s[..end].to_ascii_lowercase(), &s[end..]))
}

/// Case-insensitive keyword at the start of `s`, followed by a non-identifier character.
fn keyword<'a>(s: &'a str, kw: &str) -> Option<&'a str> {
    let head = s.get(..kw.len())?;
    if !head.eq_ignore_ascii_case(kw) {
        return None;
    }
    let rest = &s[kw.len()..];
    match rest.chars().next() {
        Some(c) if c.is_ascii_alphanumeric() || c == '_' => None,
        _ => Some(rest),
    }
}

/// First quoted string in `s`.
fn quoted(s: &str) -> Option<String> {
    let q = s.find(['\'', '"'])?;
    let quote = s[q..].chars().next()?;
    let rest = &s[q + 1..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_string())
}

/// Scan Fortran source text. `fixed` selects fixed-form comment rules (`C`, `c`, `*` in column 1).
pub(crate) fn scan(text: &str, fixed: bool) -> Scan {
    let mut r = Scan::default();
    for raw in text.lines() {
        if fixed && raw.starts_with(['C', 'c', '*', 'd', 'D']) {
            continue;
        }
        let code = strip_comment(raw);
        // Fixed form: columns 1-5 hold a label and column 6 a continuation mark; statements start in column 7.
        let body = if fixed {
            code.get(6..).unwrap_or("")
        } else {
            code
        };
        for stmt in statements(body) {
            let st = stmt.trim();
            if st.is_empty() {
                continue;
            }
            if let Some(rest) = keyword(st, "include") {
                if let Some(f) = quoted(rest) {
                    r.includes.push(f);
                }
            } else if let Some(rest) = keyword(st, "use") {
                // use [, intrinsic | non_intrinsic ::] name [, only: ...]
                let mut rest = rest.trim_start();
                let mut intrinsic = false;
                if let Some(after) = rest.strip_prefix(',') {
                    let after = after.trim_start();
                    if let Some((word, tail)) = ident(after) {
                        intrinsic = word == "intrinsic";
                        rest = tail.trim_start();
                    }
                }
                let rest = rest.strip_prefix("::").unwrap_or(rest).trim_start();
                if let Some((name, _)) = ident(rest) {
                    if !intrinsic {
                        r.uses.push(name);
                    }
                }
            } else if let Some(rest) = keyword(st, "submodule") {
                // submodule (parent[:ancestor]) name
                let rest = rest.trim_start();
                if let Some(inner) = rest.strip_prefix('(') {
                    if let Some(close) = inner.find(')') {
                        for part in inner[..close].split(':') {
                            if let Some((p, _)) = ident(part.trim()) {
                                r.uses.push(p);
                            }
                        }
                        if let Some((name, _)) = ident(inner[close + 1..].trim_start()) {
                            r.provides.push(name);
                        }
                    }
                }
            } else if let Some(rest) = keyword(st, "module") {
                // `module name` only: `module procedure x` and `module subroutine x` are not definitions.
                if let Some((name, tail)) = ident(rest.trim_start()) {
                    let kw = ["procedure", "subroutine", "function"];
                    if tail.trim().is_empty() && !kw.contains(&name.as_str()) {
                        r.provides.push(name);
                    }
                }
            }
        }
    }
    r
}

/// One file of the project with its scan result.
#[derive(Debug, Clone)]
pub(crate) struct Unit {
    pub path: PathBuf,
    pub scan: Scan,
}

/// Result of ordering: compile order (indices into the input) and, per file, every source file
/// whose change should rebuild it (transitive module providers and resolved `include` files).
#[derive(Debug)]
pub(crate) struct Plan {
    pub order: Vec<usize>,
    pub deps: Vec<Vec<PathBuf>>,
}

/// Order `units` so that every module is compiled before the files that use it.
/// `include_dirs` are searched (after the file's own directory) for `include` files.
pub(crate) fn plan(units: &[Unit], include_dirs: &[PathBuf]) -> Result<Plan> {
    let mut provider: BTreeMap<&str, usize> = BTreeMap::new();
    for (i, u) in units.iter().enumerate() {
        for m in &u.scan.provides {
            if let Some(&j) = provider.get(m.as_str()) {
                if j != i {
                    return Err(Error(format!(
                        "Fortran module '{m}' is defined in both {} and {}",
                        units[j].path.display(),
                        u.path.display()
                    )));
                }
            }
            provider.insert(m, i);
        }
    }
    // Direct dependencies between files (modules from outside the project, such as
    // `iso_fortran_env` or `omp_lib`, have no provider and are left to the compiler).
    let direct: Vec<BTreeSet<usize>> = units
        .iter()
        .enumerate()
        .map(|(i, u)| {
            u.scan
                .uses
                .iter()
                .filter_map(|m| provider.get(m.as_str()).copied())
                .filter(|&j| j != i)
                .collect()
        })
        .collect();
    // Kahn's algorithm; ready files keep their original (directory) order.
    let mut remaining: Vec<BTreeSet<usize>> = direct.clone();
    let mut done = vec![false; units.len()];
    let mut order = Vec::with_capacity(units.len());
    while order.len() < units.len() {
        let next = (0..units.len()).find(|&i| !done[i] && remaining[i].is_empty());
        let Some(i) = next else {
            let stuck: Vec<String> = (0..units.len())
                .filter(|&i| !done[i])
                .map(|i| units[i].path.display().to_string())
                .collect();
            return Err(Error(format!(
                "Fortran modules depend on each other in a cycle: {}",
                stuck.join(", ")
            )));
        };
        done[i] = true;
        order.push(i);
        for r in remaining.iter_mut() {
            r.remove(&i);
        }
    }
    // Transitive closure for staleness.
    let deps = (0..units.len())
        .map(|i| {
            let mut seen = BTreeSet::new();
            let mut stack: Vec<usize> = direct[i].iter().copied().collect();
            while let Some(j) = stack.pop() {
                if seen.insert(j) {
                    stack.extend(direct[j].iter().copied());
                }
            }
            let mut files: Vec<PathBuf> = seen.iter().map(|&j| units[j].path.clone()).collect();
            for k in std::iter::once(i).chain(seen.iter().copied()) {
                for inc in &units[k].scan.includes {
                    if let Some(p) = resolve_include(&units[k].path, inc, include_dirs) {
                        files.push(p);
                    }
                }
            }
            files
        })
        .collect();
    Ok(Plan { order, deps })
}

fn resolve_include(from: &Path, name: &str, include_dirs: &[PathBuf]) -> Option<PathBuf> {
    let own = from.parent().unwrap_or(Path::new(".")).join(name);
    std::iter::once(own)
        .chain(include_dirs.iter().map(|d| d.join(name)))
        .find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(name: &str, text: &str) -> Unit {
        Unit {
            path: PathBuf::from(name),
            scan: scan(text, is_fixed_form(Path::new(name))),
        }
    }

    #[test]
    fn finds_modules_uses_submodules_and_includes() {
        let s = scan(
            "module Physics\n  use grid_mod, only: n\n  use, intrinsic :: iso_c_binding\n  USE :: other_mod\n  include 'consts.inc'\ncontains\n  module procedure foo\nend module physics\n",
            false,
        );
        assert_eq!(s.provides, vec!["physics"]);
        assert_eq!(s.uses, vec!["grid_mod", "other_mod"]);
        assert_eq!(s.includes, vec!["consts.inc"]);
        let sub = scan("submodule (parent:grand) child\nend submodule\n", false);
        assert_eq!(sub.provides, vec!["child"]);
        assert_eq!(sub.uses, vec!["parent", "grand"]);
    }

    #[test]
    fn module_procedures_and_prefixed_functions_are_not_definitions() {
        let s = scan(
            "module m\ninterface\n  module subroutine s(x)\n  module integer function f(x)\nend interface\nend module m\n",
            false,
        );
        assert_eq!(s.provides, vec!["m"]);
    }

    #[test]
    fn comments_strings_and_semicolons() {
        let s = scan(
            "! use hidden\nprint *, 'use not_a_module'\nuse a; use b ! trailing use c\n",
            false,
        );
        assert_eq!(s.uses, vec!["a", "b"]);
    }

    #[test]
    fn fixed_form_comment_lines_are_ignored() {
        let s = scan(
            "C     use hidden\n      use real_mod\n*     use hidden2\n",
            true,
        );
        assert_eq!(s.uses, vec!["real_mod"]);
    }

    #[test]
    fn orders_providers_first_and_keeps_directory_order_otherwise() {
        let units = vec![
            unit("a_main.f90", "program p\nuse physics\nend program\n"),
            unit("grid_mod.f90", "module grid_mod\nend module\n"),
            unit("physics.f90", "module physics\nuse grid_mod\nend module\n"),
            unit("zeta.f90", "subroutine z\nend subroutine\n"),
        ];
        let p = plan(&units, &[]).unwrap();
        let names: Vec<_> = p
            .order
            .iter()
            .map(|&i| units[i].path.to_str().unwrap())
            .collect();
        assert_eq!(
            names,
            vec!["grid_mod.f90", "physics.f90", "a_main.f90", "zeta.f90"]
        );
        // a_main depends transitively on grid_mod too.
        let main_deps: Vec<_> = p.deps[0].iter().map(|d| d.to_str().unwrap()).collect();
        assert!(main_deps.contains(&"physics.f90") && main_deps.contains(&"grid_mod.f90"));
    }

    #[test]
    fn cycles_and_duplicate_modules_are_errors() {
        let cyc = vec![
            unit("a.f90", "module a\nuse b\nend module\n"),
            unit("b.f90", "module b\nuse a\nend module\n"),
        ];
        assert!(plan(&cyc, &[]).unwrap_err().0.contains("cycle"));
        let dup = vec![
            unit("a.f90", "module m\nend module\n"),
            unit("b.f90", "module m\nend module\n"),
        ];
        assert!(plan(&dup, &[]).unwrap_err().0.contains("defined in both"));
    }

    #[test]
    fn unknown_modules_are_left_to_the_compiler() {
        let units = vec![unit(
            "a.f90",
            "program p\nuse omp_lib\nuse iso_fortran_env\nend program\n",
        )];
        assert_eq!(plan(&units, &[]).unwrap().order, vec![0]);
    }
}
