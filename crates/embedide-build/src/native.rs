//! Native builds: EmbedIDE drives the selected compilers directly.
use crate::{fortran, select, Error, Prepared, Request, Result, Selection};
use embedide_core::ProcessSpec;
use embedide_toolchains::{Family, Language, Toolchain};
use embedide_workspace::{BuildKind, Manifest, UserConfig};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::SystemTime,
};

/// How one source file is compiled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SourceKind {
    C,
    Cpp,
    /// GNU-syntax assembly (`.S` preprocessed, `.s` plain)
    Gas,
    /// NASM syntax
    Nasm,
    Fortran,
}

fn classify(path: &Path) -> Option<SourceKind> {
    let ext = path.extension()?.to_str()?;
    Some(match ext {
        "c" => SourceKind::C,
        "cpp" | "cc" | "cxx" | "c++" | "C" => SourceKind::Cpp,
        "S" | "s" | "sx" => SourceKind::Gas,
        "asm" | "nasm" => SourceKind::Nasm,
        "f" | "for" | "f90" | "f95" | "f03" | "f08" | "F" | "F90" | "F95" | "F03" | "F08" => {
            SourceKind::Fortran
        }
        _ => return None,
    })
}

fn lang_of(k: SourceKind) -> Language {
    match k {
        SourceKind::C => Language::C,
        SourceKind::Cpp => Language::Cpp,
        SourceKind::Gas | SourceKind::Nasm => Language::Asm,
        SourceKind::Fortran => Language::Fortran,
    }
}

const SKIP_DIRS: &[&str] = &[
    "target",
    "zig-out",
    ".zig-cache",
    "zig-cache",
    "node_modules",
    ".git",
];

fn collect(dir: &Path, output: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| {
            Error(format!(
                "Cannot read source directory {}: {e}",
                dir.display()
            ))
        })?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        let path = e.path();
        let ft = e.file_type()?;
        if ft.is_dir() {
            if name.starts_with('.') || SKIP_DIRS.contains(&name.as_str()) || path == output {
                continue;
            }
            collect(&path, output, out)?;
        } else if ft.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// Parse a Make-style depfile into prerequisite paths.
pub(crate) fn parse_depfile(text: &str) -> Vec<PathBuf> {
    let joined = text.replace("\\\r\n", " ").replace("\\\n", " ");
    let mut deps = Vec::new();
    for rule in joined.lines() {
        let Some(idx) = rule
            .find(": ")
            .or_else(|| rule.strip_suffix(':').map(|r| r.len()))
        else {
            continue;
        };
        let rest = &rule[(idx + 1).min(rule.len())..];
        let mut cur = String::new();
        let mut chars = rest.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\\' if chars.peek() == Some(&' ') => {
                    cur.push(' ');
                    chars.next();
                }
                '$' if chars.peek() == Some(&'$') => {
                    cur.push('$');
                    chars.next();
                }
                c if c.is_whitespace() => {
                    if !cur.is_empty() {
                        deps.push(PathBuf::from(std::mem::take(&mut cur)));
                    }
                }
                c => cur.push(c),
            }
        }
        if !cur.is_empty() {
            deps.push(PathBuf::from(cur));
        }
    }
    deps
}

fn stale(
    obj: &Path,
    src: &Path,
    dep: Option<&Path>,
    extra: &[PathBuf],
    cmd_file: &Path,
    cmd: &str,
) -> bool {
    let Some(obj_t) = mtime(obj) else { return true };
    if std::fs::read_to_string(cmd_file).ok().as_deref() != Some(cmd) {
        return true;
    }
    if mtime(src).is_none_or(|t| t > obj_t) {
        return true;
    }
    // Dependencies known without a depfile (Fortran module providers and include files).
    if extra.iter().any(|d| mtime(d).is_none_or(|t| t > obj_t)) {
        return true;
    }
    if let Some(dep) = dep {
        let Ok(text) = std::fs::read_to_string(dep) else {
            return true;
        };
        for d in parse_depfile(&text) {
            let d = if d.is_absolute() {
                d
            } else {
                src.parent().unwrap_or(Path::new(".")).join(d)
            };
            if mtime(&d).is_none_or(|t| t > obj_t) {
                return true;
            }
        }
    }
    false
}

/// Put Fortran sources in module dependency order (after the other sources) and collect what each
/// one depends on, so that edits to a module rebuild its users.
fn order_fortran(
    sources: Vec<(PathBuf, SourceKind)>,
    include_dirs: &[PathBuf],
    deps: &mut BTreeMap<PathBuf, Vec<PathBuf>>,
) -> Result<Vec<(PathBuf, SourceKind)>> {
    let (fort, mut rest): (Vec<_>, Vec<_>) = sources
        .into_iter()
        .partition(|(_, k)| *k == SourceKind::Fortran);
    if fort.is_empty() {
        return Ok(rest);
    }
    let mut units = Vec::with_capacity(fort.len());
    for (p, _) in &fort {
        let bytes =
            std::fs::read(p).map_err(|e| Error(format!("Cannot read {}: {e}", p.display())))?;
        units.push(fortran::Unit {
            path: p.clone(),
            scan: fortran::scan(&String::from_utf8_lossy(&bytes), fortran::is_fixed_form(p)),
        });
    }
    let plan = fortran::plan(&units, include_dirs)?;
    for (u, d) in units.iter().zip(plan.deps) {
        deps.insert(u.path.clone(), d);
    }
    rest.extend(plan.order.into_iter().map(|i| fort[i].clone()));
    Ok(rest)
}

fn cmd_text(spec: &ProcessSpec) -> String {
    serde_json::to_string(&(&spec.program, &spec.args)).unwrap_or_default()
}

fn triple_args(sel: &Selection, triple: Option<&str>) -> Vec<String> {
    match (triple, sel.family) {
        (Some(t), Family::Clang) => vec![format!("--target={t}")],
        (Some(t), Family::ZigCc) => vec!["-target".into(), t.into()],
        _ => Vec::new(),
    }
}

fn s(v: &[String]) -> impl Iterator<Item = String> + '_ {
    v.iter().cloned()
}

pub(crate) fn prepare(
    root: &Path,
    m: &Manifest,
    user: &UserConfig,
    toolchains: &[Toolchain],
    req: &Request,
) -> Result<Prepared> {
    debug_assert_eq!(m.build.system, BuildKind::Native);
    let profile = m.profile(&req.profile)?;
    let out = m.output_dir(root, &req.profile);
    let output_root = root.join(&m.build.output);
    let triple = m.target.triple.as_deref();
    let lang = m.language()?;
    let default_name = m
        .build
        .artifact
        .clone()
        .unwrap_or_else(|| m.project.name.clone());
    let artifact = out.join(&default_name);

    // Single-invocation languages
    if matches!(lang, Language::Zig | Language::Rust) {
        let sel = select::choose(m, user, req, toolchains, lang)?;
        let entry = m
            .build
            .sources
            .iter()
            .map(|d| {
                root.join(d).join(if lang == Language::Zig {
                    "main.zig"
                } else {
                    "main.rs"
                })
            })
            .find(|p| p.is_file())
            .ok_or_else(|| {
                Error(format!(
                    "No main.{} in {}",
                    if lang == Language::Zig { "zig" } else { "rs" },
                    m.build.sources.join(", ")
                ))
            })?;
        let release = req.profile == "release";
        let mut args: Vec<String> = sel.prefix_args.clone();
        if lang == Language::Zig {
            args.extend([
                "build-exe".into(),
                entry.to_string_lossy().into_owned(),
                format!("-femit-bin={}", artifact.display()),
                "--cache-dir".into(),
                out.join("zig-cache").to_string_lossy().into_owned(),
                "-O".into(),
                if release { "ReleaseSafe" } else { "Debug" }.into(),
            ]);
            if let Some(t) = triple {
                args.extend(["-target".into(), t.into()]);
            }
        } else {
            args.extend([
                entry.to_string_lossy().into_owned(),
                "-o".into(),
                artifact.to_string_lossy().into_owned(),
                "--edition".into(),
                "2021".into(),
            ]);
            args.extend(if release {
                vec!["-C".into(), "opt-level=2".into()]
            } else {
                vec!["-g".into()]
            });
            if let Some(t) = triple {
                args.extend(["--target".into(), t.into()]);
            }
        }
        std::fs::create_dir_all(&out)?;
        return Ok(Prepared {
            root: root.to_path_buf(),
            system: BuildKind::Native,
            profile: req.profile.clone(),
            toolchains: vec![sel.clone()],
            steps: vec![ProcessSpec {
                program: sel.program.to_string_lossy().into_owned(),
                args,
            }],
            env: m.env.clone(),
            artifact: Some(artifact),
            up_to_date: 0,
            stamps: Vec::new(),
            output_root: Some(output_root),
        });
    }

    let mut files = Vec::new();
    for d in &m.build.sources {
        let dir = root.join(d);
        if !dir.is_dir() {
            return Err(Error(format!(
                "Source directory {} does not exist",
                dir.display()
            )));
        }
        collect(&dir, &output_root, &mut files)?;
    }
    let sources: Vec<(PathBuf, SourceKind)> = files
        .into_iter()
        .filter_map(|p| classify(&p).map(|k| (p, k)))
        .collect();
    if sources.is_empty() {
        return Err(Error(format!(
            "No C/C++/assembly/Fortran sources under {}",
            m.build.sources.join(", ")
        )));
    }
    let include_dirs: Vec<PathBuf> = m.build.include.iter().map(|i| root.join(i)).collect();
    let mut fortran_deps: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
    let sources = order_fortran(sources, &include_dirs, &mut fortran_deps)?;
    let needed: BTreeSet<SourceKind> = sources.iter().map(|(_, k)| *k).collect();
    let mut sel: BTreeMap<SourceKind, Selection> = BTreeMap::new();
    for k in &needed {
        let mut s = select::choose(m, user, req, toolchains, lang_of(*k))?;
        if *k == SourceKind::Nasm && s.family != Family::Nasm {
            s = select::resolve(toolchains, "nasm", Language::Asm, triple)
                .map_err(|e| Error(format!(".asm files need NASM: {}", e.0)))?;
        }
        if *k == SourceKind::Gas && s.family == Family::Nasm {
            return Err(Error("GNU assembly (.S/.s) cannot be built with NASM; choose a C driver or GNU as for asm".into()));
        }
        sel.insert(*k, s);
    }

    let includes: Vec<String> = m
        .build
        .include
        .iter()
        .map(|i| format!("-I{}", root.join(i).display()))
        .collect();
    let defines: Vec<String> = m
        .build
        .defines
        .iter()
        .chain(&profile.defines)
        .map(|d| format!("-D{d}"))
        .collect();
    let obj_dir = out.join("obj");
    let mod_dir = out.join("mod");
    let mut steps = Vec::new();
    let mut stamps = Vec::new();
    let mut objects = Vec::new();
    let mut up_to_date = 0;
    let mut any_compiled = false;

    for (src, kind) in &sources {
        let rel = src
            .strip_prefix(root)
            .unwrap_or(src)
            .to_string_lossy()
            .replace(['/', '\\'], "__");
        let obj = obj_dir.join(format!("{rel}.o"));
        let dep = obj_dir.join(format!("{rel}.d"));
        let cmd_file = obj_dir.join(format!("{rel}.cmd"));
        let t = &sel[kind];
        let mut args: Vec<String> = t.prefix_args.clone();
        let uses_dep;
        match (kind, t.family) {
            (SourceKind::Nasm, _) => {
                let fmt = "elf64";
                args.extend([
                    "-f".into(),
                    fmt.into(),
                    src.to_string_lossy().into_owned(),
                    "-o".into(),
                    obj.to_string_lossy().into_owned(),
                ]);
                if profile.flags.iter().any(|f| f == "-g") {
                    args.push("-g".into());
                }
                uses_dep = false;
            }
            (SourceKind::Gas, Family::Gas) => {
                if src.extension().is_some_and(|e| e == "S" || e == "sx") {
                    return Err(Error(format!("{} needs the C preprocessor; GNU as cannot build .S files. Choose gcc/clang/zig-cc for asm", src.display())));
                }
                args.extend(s(&m.target.flags));
                if profile.flags.iter().any(|f| f == "-g") {
                    args.push("-g".into());
                }
                args.extend([
                    "-o".into(),
                    obj.to_string_lossy().into_owned(),
                    src.to_string_lossy().into_owned(),
                ]);
                uses_dep = false;
            }
            (_, Family::Lfortran) => {
                args.extend([
                    "-c".into(),
                    src.to_string_lossy().into_owned(),
                    "-o".into(),
                    obj.to_string_lossy().into_owned(),
                    "-J".into(),
                    mod_dir.to_string_lossy().into_owned(),
                ]);
                uses_dep = false;
            }
            (_, Family::Gcc | Family::Clang | Family::ZigCc) => {
                args.extend([
                    "-c".into(),
                    src.to_string_lossy().into_owned(),
                    "-o".into(),
                    obj.to_string_lossy().into_owned(),
                ]);
                args.extend(s(&m.target.flags));
                args.extend(triple_args(t, triple));
                args.extend(s(&m.build.flags));
                args.extend(s(&profile.flags));
                args.extend(includes.iter().cloned());
                args.extend(defines.iter().cloned());
                if *kind == SourceKind::Fortran {
                    match t.kind {
                        "gfortran" => {
                            args.extend(["-J".into(), mod_dir.to_string_lossy().into_owned()])
                        }
                        "flang" => args
                            .extend(["-module-dir".into(), mod_dir.to_string_lossy().into_owned()]),
                        _ => {}
                    }
                }
                // Fortran has no depfile (`gfortran -MMD` needs `-cpp`): module dependencies come from
                // the source scan in `order_fortran`.
                uses_dep = *kind != SourceKind::Fortran;
                if uses_dep {
                    args.extend([
                        "-MMD".into(),
                        "-MF".into(),
                        dep.to_string_lossy().into_owned(),
                    ]);
                }
            }
            (_, f) => {
                return Err(Error(format!(
                    "Toolchain '{}' ({f:?}) cannot compile {}",
                    t.id,
                    src.display()
                )))
            }
        }
        let spec = ProcessSpec {
            program: t.program.to_string_lossy().into_owned(),
            args,
        };
        let text = cmd_text(&spec);
        let extra = fortran_deps.get(src).map(Vec::as_slice).unwrap_or(&[]);
        if stale(
            &obj,
            src,
            uses_dep.then_some(dep.as_path()),
            extra,
            &cmd_file,
            &text,
        ) {
            steps.push(spec);
            stamps.push((cmd_file, text));
            any_compiled = true;
        } else {
            up_to_date += 1;
        }
        objects.push(obj);
    }

    // Link with the driver of the "highest" language present
    let linker = if needed.contains(&SourceKind::Fortran) {
        sel[&SourceKind::Fortran].clone()
    } else if needed.contains(&SourceKind::Cpp) {
        sel[&SourceKind::Cpp].clone()
    } else if let Some(c) = sel.get(&SourceKind::C) {
        c.clone()
    } else if let Some(g) = sel
        .get(&SourceKind::Gas)
        .filter(|g| matches!(g.family, Family::Gcc | Family::Clang | Family::ZigCc))
    {
        g.clone()
    } else {
        select::choose(m, user, req, toolchains, Language::C).map_err(|e| {
            Error(format!(
                "Linking assembly objects needs a C driver: {}",
                e.0
            ))
        })?
    };
    let mut args: Vec<String> = linker.prefix_args.clone();
    args.extend(objects.iter().map(|o| o.to_string_lossy().into_owned()));
    args.extend(["-o".into(), artifact.to_string_lossy().into_owned()]);
    if linker.family != Family::Lfortran {
        args.extend(s(&m.target.flags));
        args.extend(triple_args(&linker, triple));
        args.extend(s(&m.build.link_flags));
        args.extend(s(&profile.link_flags));
        if let Some(ls) = &m.build.linker_script {
            args.extend(["-T".into(), root.join(ls).to_string_lossy().into_owned()]);
        }
        args.extend(m.build.libs.iter().map(|l| format!("-l{l}")));
    }
    let link = ProcessSpec {
        program: linker.program.to_string_lossy().into_owned(),
        args,
    };
    let link_text = cmd_text(&link);
    let link_cmd = out.join(format!("{default_name}.cmd"));
    let link_stale = any_compiled
        || mtime(&artifact).is_none()
        || std::fs::read_to_string(&link_cmd).ok().as_deref() != Some(link_text.as_str())
        || objects.iter().any(|o| mtime(o) > mtime(&artifact));
    if link_stale {
        steps.push(link);
        stamps.push((link_cmd, link_text));
    }
    std::fs::create_dir_all(&obj_dir)?;
    if needed.contains(&SourceKind::Fortran) {
        std::fs::create_dir_all(&mod_dir)?;
    }
    let mut toolchains: Vec<Selection> = sel.into_values().collect();
    if !toolchains.iter().any(|t| t.id == linker.id) {
        toolchains.push(linker);
    }
    Ok(Prepared {
        root: root.to_path_buf(),
        system: BuildKind::Native,
        profile: req.profile.clone(),
        toolchains,
        steps,
        env: m.env.clone(),
        artifact: Some(artifact),
        up_to_date,
        stamps,
        output_root: Some(output_root),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depfiles_with_spaces_and_continuations() {
        let d = "/p/obj/main.o: /p/src/main.c /p/My\\ Headers/a.h \\\n /p/include/b.h\n";
        let deps = parse_depfile(d);
        assert_eq!(
            deps,
            vec![
                PathBuf::from("/p/src/main.c"),
                PathBuf::from("/p/My Headers/a.h"),
                PathBuf::from("/p/include/b.h")
            ]
        );
        let win = "C:/p/obj/main.o: C:/p/src/main.c\n";
        assert_eq!(parse_depfile(win), vec![PathBuf::from("C:/p/src/main.c")]);
    }

    #[test]
    fn source_classification() {
        assert_eq!(classify(Path::new("a.c")), Some(SourceKind::C));
        assert_eq!(classify(Path::new("a.cpp")), Some(SourceKind::Cpp));
        assert_eq!(classify(Path::new("a.S")), Some(SourceKind::Gas));
        assert_eq!(classify(Path::new("a.asm")), Some(SourceKind::Nasm));
        assert_eq!(classify(Path::new("a.F90")), Some(SourceKind::Fortran));
        assert_eq!(classify(Path::new("a.h")), None);
    }
}
