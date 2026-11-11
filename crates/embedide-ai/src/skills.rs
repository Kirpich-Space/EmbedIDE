//! Skills: reusable instructions in `SKILL.md` files, following the Agent
//! Skills layout (`<dir>/<name>/SKILL.md` with `name` and `description`
//! frontmatter). Discovery reads only the frontmatter; the instructions are
//! loaded when the user activates a skill.
//!
//! Search order (the first skill with a name wins; later ones are shadowed):
//! 1. the project: `<project>/.embedide/skills/<name>/SKILL.md`
//! 2. `ai.skills.dirs`, in the configured order
//! 3. global: `<config dir>/skills/<name>/SKILL.md`
//!    (`~/.config/embedide/skills` on Linux; `EMBEDIDE_CONFIG_DIR` overrides)
//! 4. built in: EmbedIDE's own skills ([`BUILTIN`]), compiled into the binary
//!
//! With `ai.skills.auto` (default) agent tasks get `embedide-agent` (how to
//! work inside EmbedIDE) and the skills [`detect`] finds for the project
//! without `/skill`. Built-in skills are EmbedIDE's own instructions; a
//! skill with the same name in a directory above replaces one and is then
//! treated like any user skill.
//!
//! Skill text is untrusted: it is quoted into the agent's task as guidance
//! and never changes EmbedIDE's permission rules, policy or configuration.
//! `allowed-tools` in frontmatter is read and reported but grants nothing.
use crate::agent::SkillText;
use embedide_workspace::config::SkillsConfig;
use std::{
    collections::BTreeMap,
    fmt,
    path::{Path, PathBuf},
};

/// File name of a skill.
pub const FILE: &str = "SKILL.md";
/// Longest name.
pub const MAX_NAME: usize = 64;
/// Longest description.
pub const MAX_DESCRIPTION: usize = 1024;
/// Bytes read when only the frontmatter is needed.
const HEAD_BYTES: usize = 16 * 1024;

/// Where a skill was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scope {
    /// `.embedide/skills` in the project.
    Project,
    /// A directory from `ai.skills.dirs`.
    Extra,
    /// The user's global skills directory.
    Global,
    /// Compiled into EmbedIDE.
    Builtin,
}

/// Skills compiled into EmbedIDE: (name, SKILL.md).
pub const BUILTIN: [(&str, &str); 7] = [
    (
        "embedide-agent",
        include_str!("../skills/embedide-agent/SKILL.md"),
    ),
    ("c-cpp", include_str!("../skills/c-cpp/SKILL.md")),
    ("c-firmware", include_str!("../skills/c-firmware/SKILL.md")),
    ("rust-crate", include_str!("../skills/rust-crate/SKILL.md")),
    ("zig-build", include_str!("../skills/zig-build/SKILL.md")),
    ("asm-review", include_str!("../skills/asm-review/SKILL.md")),
    (
        "embedded-bringup",
        include_str!("../skills/embedded-bringup/SKILL.md"),
    ),
];

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Project => "project",
            Self::Extra => "extra",
            Self::Global => "global",
            Self::Builtin => "built-in",
        })
    }
}

/// Parsed frontmatter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Frontmatter {
    /// `name`.
    pub name: String,
    /// `description`.
    pub description: String,
    /// `license`, if given.
    pub license: Option<String>,
    /// `allowed-tools`, if given (informational only).
    pub allowed_tools: Vec<String>,
    /// Other top-level keys (kept, not interpreted).
    pub other: BTreeMap<String, String>,
    /// Byte offset where the instructions start.
    pub body_start: usize,
}

/// A discovered skill (instructions not loaded yet).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillInfo {
    /// Name.
    pub name: String,
    /// What it is for and when to use it.
    pub description: String,
    /// The skill directory.
    pub dir: PathBuf,
    /// Where it was found.
    pub scope: Scope,
    /// Notes about the file (ignored fields, …).
    pub warnings: Vec<String>,
    /// The SKILL.md text of a built-in skill.
    pub builtin: Option<&'static str>,
}

/// A skill directory that could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Broken {
    /// The directory or file.
    pub path: PathBuf,
    /// What is wrong.
    pub error: String,
}

/// Everything found.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    /// Usable skills, by precedence then name.
    pub skills: Vec<SkillInfo>,
    /// Skills hidden by a skill with the same name earlier in the search order.
    pub shadowed: Vec<SkillInfo>,
    /// Skill directories with errors.
    pub broken: Vec<Broken>,
    /// Directories searched, in order (existing or not).
    pub searched: Vec<(Scope, PathBuf)>,
}

fn unquote(v: &str) -> String {
    let v = v.trim();
    if v.len() >= 2
        && ((v.starts_with('"') && v.ends_with('"')) || (v.starts_with('\'') && v.ends_with('\'')))
    {
        let inner = &v[1..v.len() - 1];
        if v.starts_with('"') {
            return inner.replace("\\\"", "\"").replace("\\n", "\n");
        }
        return inner.replace("''", "'");
    }
    v.to_string()
}

/// Parse the frontmatter of a SKILL.md (a small YAML subset: `key: value`,
/// quoted values, `|`/`>` block scalars and `- item` lists; nested maps are
/// skipped).
pub fn parse(text: &str) -> Result<Frontmatter, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.split_inclusive('\n');
    let first = lines.next().unwrap_or("");
    if first.trim_end() != "---" {
        return Err(
            "SKILL.md must start with a '---' frontmatter block (name, description)".into(),
        );
    }
    let mut offset = first.len();
    let mut entries: Vec<(String, String, Vec<String>)> = Vec::new();
    let mut closed = false;
    let mut block: Option<(bool, Option<usize>)> = None; // (folded, indent)
    for raw in lines {
        offset += raw.len();
        let line = raw.trim_end_matches(['\n', '\r']);
        if line.trim_end() == "---" {
            closed = true;
            break;
        }
        let indent = line.len() - line.trim_start().len();
        if let Some((folded, ind)) = &mut block {
            if line.trim().is_empty() || indent > 0 {
                let ind = *ind.get_or_insert(indent.max(1));
                let content = line.get(ind.min(line.len())..).unwrap_or("").to_string();
                if let Some((_, v, _)) = entries.last_mut() {
                    if !v.is_empty() {
                        v.push(if *folded && !content.is_empty() {
                            ' '
                        } else {
                            '\n'
                        });
                    }
                    v.push_str(&content);
                }
                continue;
            }
            block = None;
        }
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        if indent > 0 {
            // List item of the previous key, or a nested map (skipped)
            if let (Some(item), Some((_, _, list))) =
                (line.trim_start().strip_prefix("- "), entries.last_mut())
            {
                list.push(unquote(item));
            }
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            return Err(format!("frontmatter line is not 'key: value': {line}"));
        };
        let value = value.trim();
        if value == "|" || value == ">" || value == "|-" || value == ">-" {
            block = Some((value.starts_with('>'), None));
            entries.push((key.trim().to_string(), String::new(), Vec::new()));
        } else if value.starts_with('[') && value.ends_with(']') {
            let items = value[1..value.len() - 1]
                .split(',')
                .map(unquote)
                .filter(|s| !s.is_empty())
                .collect();
            entries.push((key.trim().to_string(), String::new(), items));
        } else {
            entries.push((key.trim().to_string(), unquote(value), Vec::new()));
        }
    }
    if !closed {
        return Err("the frontmatter block is not closed with '---'".into());
    }
    let mut fm = Frontmatter {
        body_start: offset,
        ..Default::default()
    };
    for (key, value, list) in entries {
        match key.as_str() {
            "name" => fm.name = value.trim().to_string(),
            "description" => fm.description = value.trim().to_string(),
            "license" => fm.license = Some(value),
            "allowed-tools" | "allowed_tools" => {
                fm.allowed_tools = if list.is_empty() {
                    split_tools(&value)
                } else {
                    list
                }
            }
            _ => {
                fm.other.insert(key, value);
            }
        }
    }
    validate_name(&fm.name)?;
    if fm.description.is_empty() {
        return Err(
            "frontmatter needs a 'description' (what the skill does and when to use it)".into(),
        );
    }
    if fm.description.chars().count() > MAX_DESCRIPTION {
        return Err(format!(
            "description is longer than {MAX_DESCRIPTION} characters"
        ));
    }
    Ok(fm)
}

/// Split `Bash(git *), Read Edit` at commas and spaces outside parentheses.
fn split_tools(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0usize;
    for c in value.chars() {
        match c {
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                cur.push(c);
            }
            ',' | ' ' | '\t' if depth == 0 => {
                if !cur.trim().is_empty() {
                    out.push(cur.trim().to_string());
                }
                cur.clear();
            }
            c => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// Names: 1–64 lowercase letters, digits and hyphens, not starting or ending
/// with a hyphen.
pub fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("frontmatter needs a 'name'".into());
    }
    if name.len() > MAX_NAME
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        || name.starts_with('-')
        || name.ends_with('-')
    {
        return Err(format!(
            "invalid skill name '{name}': use 1-{MAX_NAME} lowercase letters, digits and hyphens"
        ));
    }
    Ok(())
}

fn read_head(path: &Path, limit: usize) -> Result<String, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| format!("cannot read {FILE}: {e}"))?;
    let mut data = Vec::new();
    file.take(limit as u64)
        .read_to_end(&mut data)
        .map_err(|e| format!("cannot read {FILE}: {e}"))?;
    // A cut in the middle of a character is fine for the frontmatter.
    match String::from_utf8(data) {
        Ok(s) => Ok(s),
        Err(e) => {
            let valid = e.utf8_error().valid_up_to();
            let bytes = e.into_bytes();
            if bytes.len() == limit && limit - valid < 4 {
                Ok(String::from_utf8_lossy(&bytes[..valid]).into_owned())
            } else {
                Err(format!("{FILE} is not UTF-8 text"))
            }
        }
    }
}

/// The directories searched, in precedence order.
pub fn search_dirs(project: Option<&Path>, cfg: &SkillsConfig) -> Vec<(Scope, PathBuf)> {
    let mut dirs = Vec::new();
    if let Some(p) = project {
        dirs.push((Scope::Project, p.join(".embedide").join("skills")));
    }
    for d in &cfg.dirs {
        let d = match (d.is_absolute(), project) {
            (false, Some(p)) => p.join(d),
            _ => d.clone(),
        };
        dirs.push((Scope::Extra, d));
    }
    if let Ok(c) = embedide_workspace::config::config_dir() {
        dirs.push((Scope::Global, c.join("skills")));
    }
    dirs
}

/// Find all skills (frontmatter only).
pub fn discover(project: Option<&Path>, cfg: &SkillsConfig) -> Catalog {
    let mut cat = Catalog {
        searched: search_dirs(project, cfg),
        ..Default::default()
    };
    if !cfg.enabled {
        return cat;
    }
    for (scope, dir) in cat.searched.clone() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let path = e.path();
            if !path.is_dir() {
                continue;
            }
            let dir_name = e.file_name().to_string_lossy().into_owned();
            if dir_name.starts_with('.') {
                continue;
            }
            let file = path.join(FILE);
            if !file.is_file() {
                cat.broken.push(Broken {
                    path: path.clone(),
                    error: format!("no {FILE} in this skill directory"),
                });
                continue;
            }
            let fm = read_head(&file, HEAD_BYTES).and_then(|t| parse(&t));
            let fm = match fm {
                Ok(fm) => fm,
                Err(error) => {
                    cat.broken.push(Broken { path: file, error });
                    continue;
                }
            };
            if fm.name != dir_name {
                cat.broken.push(Broken {
                    path: file,
                    error: format!(
                        "name '{}' does not match its directory '{dir_name}'",
                        fm.name
                    ),
                });
                continue;
            }
            let mut warnings = Vec::new();
            if !fm.allowed_tools.is_empty() {
                warnings.push(format!(
                    "allowed-tools ({}) is ignored: skills cannot grant permissions",
                    fm.allowed_tools.join(", ")
                ));
            }
            let info = SkillInfo {
                name: fm.name,
                description: fm.description,
                dir: path,
                scope,
                warnings,
                builtin: None,
            };
            if cat.skills.iter().any(|s| s.name == info.name) {
                cat.shadowed.push(info);
            } else {
                cat.skills.push(info);
            }
        }
    }
    for (name, text) in BUILTIN {
        let fm = parse(text).expect("built-in skills are valid");
        let info = SkillInfo {
            name: name.to_string(),
            description: fm.description,
            dir: PathBuf::from(format!("(built-in)/{name}")),
            scope: Scope::Builtin,
            warnings: Vec::new(),
            builtin: Some(text),
        };
        if cat.skills.iter().any(|s| s.name == info.name) {
            cat.shadowed.push(info);
        } else {
            cat.skills.push(info);
        }
    }
    cat
}

/// Skills that fit the project at `root`, in a stable order:
/// `embedide-agent` always, then by what the workspace contains (C/C++
/// sources, firmware markers, Cargo, Zig, assembly).
pub fn detect(root: &Path) -> Vec<&'static str> {
    #[derive(Default)]
    struct Seen {
        c: bool,
        firmware: bool,
        rust: bool,
        zig: bool,
        asm: bool,
    }
    let mut seen = Seen::default();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    let mut budget = 3000usize;
    let skip = [
        ".git",
        "target",
        "node_modules",
        ".zig-cache",
        "zig-cache",
        "zig-out",
        "build",
        ".venv",
        "__pycache__",
        ".embedide",
    ];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            if budget == 0 {
                break;
            }
            budget -= 1;
            let name = e.file_name().to_string_lossy().into_owned();
            let path = e.path();
            if path.is_dir() {
                if depth < 3 && !skip.contains(&name.as_str()) && !name.starts_with('.') {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            let lower = name.to_ascii_lowercase();
            let ext = lower.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
            match ext {
                "c" | "h" | "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" | "ino" => seen.c = true,
                "asm" | "s" | "nasm" => seen.asm = true,
                "ld" | "ioc" | "svd" => seen.firmware = true,
                _ => {}
            }
            match lower.as_str() {
                "cargo.toml" => seen.rust = true,
                "build.zig" | "build.zig.zon" => seen.zig = true,
                "platformio.ini" | "sdkconfig" | "memory.x" | "openocd.cfg" => seen.firmware = true,
                _ if lower.starts_with("startup_") => seen.firmware = true,
                "embedide.toml" | "makefile" | "cmakelists.txt" | "gnumakefile" => {
                    use std::io::Read;
                    let mut head = String::new();
                    if let Ok(f) = std::fs::File::open(&path) {
                        let _ = f.take(64 * 1024).read_to_string(&mut head);
                    }
                    let head = head.to_ascii_lowercase();
                    if [
                        "arm-none-eabi",
                        "cortex-m",
                        "board",
                        "riscv32",
                        "avr-gcc",
                        "xtensa",
                        "-mcpu=",
                        "stm32",
                        "nrf5",
                        "rp2040",
                        "esp32",
                    ]
                    .iter()
                    .any(|m| head.contains(m))
                    {
                        seen.firmware = true;
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = vec!["embedide-agent"];
    if seen.c {
        out.push("c-cpp");
    }
    if seen.c && seen.firmware {
        out.push("c-firmware");
    }
    if seen.rust {
        out.push("rust-crate");
    }
    if seen.zig {
        out.push("zig-build");
    }
    if seen.asm {
        out.push("asm-review");
    }
    if seen.firmware {
        out.push("embedded-bringup");
    }
    out
}

/// Skills for one agent task: the automatic ones (unless `ai.skills.auto`
/// is off, or a name is in `ai.skills.exclude` or `off`), then `active`.
/// Returns the skills and problems to report (a skill that failed to load).
pub fn for_task(
    root: &Path,
    cfg: &SkillsConfig,
    active: &[SkillText],
    off: &[String],
) -> (Vec<SkillText>, Vec<String>) {
    let mut out: Vec<SkillText> = Vec::new();
    let mut problems = Vec::new();
    if cfg.enabled && cfg.auto {
        let cat = discover(Some(root), cfg);
        for name in detect(root) {
            if cfg.exclude.iter().any(|e| e == name) || off.iter().any(|o| o == name) {
                continue;
            }
            match cat.get(name).and_then(|info| load(info, cfg.max_bytes)) {
                Ok(t) => out.push(t),
                Err(e) => problems.push(e),
            }
        }
    }
    for s in active {
        if !out.iter().any(|o| o.name == s.name) {
            out.push(s.clone());
        }
    }
    (out, problems)
}

impl Catalog {
    /// A skill by name, with a helpful error.
    pub fn get(&self, name: &str) -> Result<&SkillInfo, String> {
        if let Some(s) = self.skills.iter().find(|s| s.name == name) {
            return Ok(s);
        }
        if let Some(b) = self.broken.iter().find(|b| {
            b.path
                .components()
                .any(|c| c.as_os_str().to_string_lossy() == name)
        }) {
            return Err(format!(
                "Skill '{name}' is broken: {} ({})",
                b.error,
                b.path.display()
            ));
        }
        let known: Vec<&str> = self.skills.iter().map(|s| s.name.as_str()).collect();
        Err(if known.is_empty() {
            format!(
                "No skill '{name}'. No skills found; create {} (see /skills)",
                self.searched
                    .first()
                    .map(|(_, d)| d.join(name).join(FILE).display().to_string())
                    .unwrap_or_else(|| format!(".embedide/skills/{name}/{FILE}"))
            )
        } else {
            format!("No skill '{name}'. Available: {}", known.join(", "))
        })
    }
}

/// Load a skill's instructions (the body after the frontmatter).
pub fn load(info: &SkillInfo, max_bytes: usize) -> Result<SkillText, String> {
    if let Some(text) = info.builtin {
        let fm = parse(text)?;
        return Ok(SkillText {
            name: info.name.clone(),
            source: format!("built-in skill {}", info.name),
            body: text[fm.body_start..].trim().to_string(),
            trusted: true,
        });
    }
    let file = info.dir.join(FILE);
    let len = std::fs::metadata(&file)
        .map_err(|e| format!("cannot read {}: {e}", file.display()))?
        .len();
    if len > max_bytes as u64 {
        return Err(format!(
            "{} is {len} bytes, more than ai.skills.max_bytes ({max_bytes})",
            file.display()
        ));
    }
    let text = std::fs::read_to_string(&file)
        .map_err(|e| format!("cannot read {}: {e}", file.display()))?;
    let fm = parse(&text)?;
    let body = text[fm.body_start..].trim().to_string();
    if body.is_empty() {
        return Err(format!(
            "{} has no instructions after the frontmatter",
            file.display()
        ));
    }
    Ok(SkillText {
        name: info.name.clone(),
        source: format!("{} skill {}", info.scope, file.display()),
        body: format!(
            "{body}\n\n(Skill directory: {}. Other files there are only read if needed, under the normal permission rules.)",
            info.dir.display()
        ),
        trusted: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontmatter_parses_the_agent_skills_layout() {
        let fm = parse(
            "---\nname: c-embedded\ndescription: >\n  Firmware rules for\n  bare-metal C.\nlicense: MIT\nallowed-tools: Bash, Read\nmetadata:\n  version: \"1.0\"\n---\n# Body\nUse volatile.\n",
        )
        .unwrap();
        assert_eq!(fm.name, "c-embedded");
        assert_eq!(fm.description, "Firmware rules for bare-metal C.");
        assert_eq!(fm.license.as_deref(), Some("MIT"));
        assert_eq!(fm.allowed_tools, ["Bash", "Read"]);
        assert!(fm.other.contains_key("metadata"));
        let body = "---\nname: x\ndescription: 'it''s fine'\n---\nbody";
        let fm = parse(body).unwrap();
        assert_eq!(fm.description, "it's fine");
        assert_eq!(&body[fm.body_start..], "body");
        let fm = parse("---\nname: y\ndescription: d\nallowed-tools:\n  - Bash\n  - Edit\n---\n")
            .unwrap();
        assert_eq!(fm.allowed_tools, ["Bash", "Edit"]);
    }

    #[test]
    fn invalid_frontmatter_is_reported_clearly() {
        for (text, needle) in [
            ("# no frontmatter", "must start"),
            ("---\nname: x\ndescription: d\n", "not closed"),
            ("---\ndescription: d\n---\n", "needs a 'name'"),
            (
                "---\nname: Bad_Name\ndescription: d\n---\n",
                "invalid skill name",
            ),
            ("---\nname: ok\n---\n", "description"),
            ("---\nname ok\n---\n", "key: value"),
        ] {
            let e = parse(text).unwrap_err();
            assert!(e.contains(needle), "{text:?}: {e}");
        }
        assert!(parse(&format!(
            "---\nname: x\ndescription: {}\n---\n",
            "d".repeat(1100)
        ))
        .is_err());
    }

    fn skill(root: &Path, name: &str, description: &str, body: &str) {
        let d = root.join(name);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join(FILE),
            format!("---\nname: {name}\ndescription: {description}\n---\n{body}"),
        )
        .unwrap();
    }

    #[test]
    fn built_in_skills_parse_load_and_are_trusted() {
        let cat = discover(None, &SkillsConfig::default());
        for (name, _) in BUILTIN {
            let info = cat
                .skills
                .iter()
                .chain(&cat.shadowed)
                .find(|s| s.name == name && s.scope == Scope::Builtin)
                .unwrap_or_else(|| panic!("{name} missing"));
            let text = load(info, 64 * 1024).unwrap();
            assert!(text.trusted, "{name}");
            assert!(text.body.len() > 500, "{name} is detailed");
            assert!(
                !text.body.starts_with("---"),
                "{name}: frontmatter stripped"
            );
        }
        let agent = load(cat.get("embedide-agent").unwrap(), 64 * 1024).unwrap();
        assert!(
            agent.body.contains("Never ask\nthe user to paste code")
                || agent.body.contains("Use them.")
        );
        assert!(agent.body.contains("timeout 10 ./app"));
    }

    #[test]
    fn skills_are_chosen_by_what_the_project_contains() {
        let base = std::env::temp_dir().join(format!("embedide-detect-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let host = base.join("host");
        std::fs::create_dir_all(host.join("src")).unwrap();
        std::fs::write(host.join("src/main.c"), "int main(void){return 0;}\n").unwrap();
        std::fs::write(
            host.join("Makefile"),
            "app: src/main.c\n\tcc -o app src/main.c\n",
        )
        .unwrap();
        assert_eq!(detect(&host), ["embedide-agent", "c-cpp"]);
        let fw = base.join("fw");
        std::fs::create_dir_all(&fw).unwrap();
        std::fs::write(fw.join("main.c"), "").unwrap();
        std::fs::write(fw.join("start.s"), "").unwrap();
        std::fs::write(fw.join("Makefile"), "CC = arm-none-eabi-gcc\n").unwrap();
        assert_eq!(
            detect(&fw),
            [
                "embedide-agent",
                "c-cpp",
                "c-firmware",
                "asm-review",
                "embedded-bringup"
            ]
        );
        let mixed = base.join("mixed");
        std::fs::create_dir_all(mixed.join("target/debug")).unwrap();
        std::fs::write(mixed.join("Cargo.toml"), "[package]\n").unwrap();
        std::fs::write(mixed.join("build.zig"), "").unwrap();
        std::fs::write(mixed.join("target/debug/junk.c"), "").unwrap();
        assert_eq!(
            detect(&mixed),
            ["embedide-agent", "rust-crate", "zig-build"]
        );
        assert_eq!(detect(&base.join("empty-missing")), ["embedide-agent"]);
        // for_task: automatic skills, exclusions, a project override, user skills appended
        let override_dir = host.join(".embedide/skills/c-cpp");
        std::fs::create_dir_all(&override_dir).unwrap();
        std::fs::write(
            override_dir.join(FILE),
            "---\nname: c-cpp\ndescription: ours\n---\nTeam rules.\n",
        )
        .unwrap();
        let user = SkillText {
            name: "mine".into(),
            source: "x".into(),
            body: "b".into(),
            trusted: false,
        };
        let (skills, problems) = for_task(
            &host,
            &SkillsConfig::default(),
            std::slice::from_ref(&user),
            &[],
        );
        assert!(problems.is_empty(), "{problems:?}");
        let names: Vec<(&str, bool)> = skills
            .iter()
            .map(|s| (s.name.as_str(), s.trusted))
            .collect();
        assert_eq!(
            names,
            [("embedide-agent", true), ("c-cpp", false), ("mine", false)]
        );
        assert!(
            skills[1].body.starts_with("Team rules."),
            "the project's c-cpp replaced the built-in one"
        );
        let cfg = SkillsConfig {
            exclude: vec!["c-cpp".into()],
            ..Default::default()
        };
        let (skills, _) = for_task(&host, &cfg, &[], &["embedide-agent".into()]);
        assert!(skills.is_empty());
        let (skills, _) = for_task(
            &host,
            &SkillsConfig {
                auto: false,
                ..Default::default()
            },
            &[user],
            &[],
        );
        assert_eq!(skills.len(), 1);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn project_skills_shadow_extra_and_global_ones() {
        let base = std::env::temp_dir().join(format!("embedide-skills-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let project = base.join("proj");
        let extra = base.join("extra");
        let project_skills = project.join(".embedide/skills");
        skill(&project_skills, "rust", "project rust", "Project rules");
        skill(&extra, "rust", "extra rust", "Extra rules");
        skill(&extra, "zig", "zig rules", "Use zig build");
        std::fs::create_dir_all(project_skills.join("empty")).unwrap();
        skill(&project_skills, "mismatch", "x", "y");
        std::fs::write(
            project_skills.join("mismatch").join(FILE),
            "---\nname: other\ndescription: d\n---\nbody",
        )
        .unwrap();
        let cfg = SkillsConfig {
            dirs: vec![extra.clone()],
            ..Default::default()
        };
        let cat = discover(Some(&project), &cfg);
        let names: Vec<(&str, Scope)> = cat
            .skills
            .iter()
            .filter(|s| s.scope < Scope::Global)
            .map(|s| (s.name.as_str(), s.scope))
            .collect();
        assert_eq!(names, [("rust", Scope::Project), ("zig", Scope::Extra)]);
        assert_eq!(cat.shadowed.len(), 1);
        assert_eq!(cat.shadowed[0].description, "extra rust");
        assert_eq!(
            cat.broken
                .iter()
                .filter(|b| b.path.starts_with(&base))
                .count(),
            2,
            "{:?}",
            cat.broken
        );
        let text = load(cat.get("rust").unwrap(), 64 * 1024).unwrap();
        assert!(text.body.starts_with("Project rules"));
        assert!(text.source.starts_with("project skill"));
        assert!(cat
            .get("nope")
            .unwrap_err()
            .contains("Available: rust, zig"));
        assert!(cat.get("mismatch").unwrap_err().contains("broken"));
        assert!(load(cat.get("zig").unwrap(), 10)
            .unwrap_err()
            .contains("max_bytes"));
        let off = discover(
            Some(&project),
            &SkillsConfig {
                enabled: false,
                ..cfg
            },
        );
        assert!(off.skills.is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }
}
