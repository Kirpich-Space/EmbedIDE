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
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Project => "project",
            Self::Extra => "extra",
            Self::Global => "global",
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
            };
            if cat.skills.iter().any(|s| s.name == info.name) {
                cat.shadowed.push(info);
            } else {
                cat.skills.push(info);
            }
        }
    }
    cat
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
    fn the_example_skills_parse_and_load() {
        let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/skills");
        let cfg = SkillsConfig {
            dirs: vec![examples],
            ..Default::default()
        };
        let cat = discover(None, &cfg);
        let mut names: Vec<&str> = cat
            .skills
            .iter()
            .filter(|s| s.scope == Scope::Extra)
            .map(|s| s.name.as_str())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "asm-review",
                "c-firmware",
                "embedded-bringup",
                "rust-crate",
                "zig-build"
            ]
        );
        for s in cat.skills.iter().filter(|s| s.scope == Scope::Extra) {
            assert!(s.warnings.is_empty(), "{}: {:?}", s.name, s.warnings);
            assert!(load(s, 64 * 1024).unwrap().body.len() > 200, "{}", s.name);
        }
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
            .filter(|s| s.scope != Scope::Global)
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
