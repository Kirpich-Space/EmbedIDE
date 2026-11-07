//! What is sent to a provider. Only files the user names are attached;
//! the workspace is never uploaded wholesale. Exclusions (project `[ai]
//! exclude` plus built-in secret patterns) and a byte budget apply, and the
//! result lists exactly which bytes of which files are included.
use crate::{Error, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Patterns that are never sent, whatever the project says.
pub const ALWAYS_EXCLUDED: &[&str] = &[
    ".env",
    ".env.*",
    "*.pem",
    "*.key",
    "*.p12",
    "*.pfx",
    "id_rsa*",
    "id_ed25519*",
    "id_ecdsa*",
    "credentials*",
    ".git/**",
    "**/.git/**",
    "secrets/**",
    "**/secrets/**",
    ".netrc",
    "*.keystore",
];

/// One attached file.
#[derive(Debug, Clone, Serialize)]
pub struct ContextFile {
    /// Path as shown to the model (project-relative when possible).
    pub path: String,
    /// Bytes included.
    pub bytes: usize,
    /// True when the budget cut the file.
    pub truncated: bool,
}

/// A file that was not attached, and why.
#[derive(Debug, Clone, Serialize)]
pub struct Refused {
    /// Path.
    pub path: String,
    /// Reason.
    pub reason: String,
}

/// The attachment set.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Context {
    /// Included files.
    pub files: Vec<ContextFile>,
    /// Excluded files.
    pub refused: Vec<Refused>,
    /// The text block sent to the model.
    #[serde(skip)]
    pub text: String,
}

/// Simple glob: `*` (no `/`), `**` (anything), `?` (one char).
pub fn glob_match(pattern: &str, path: &str) -> bool {
    fn rec(p: &[u8], s: &[u8]) -> bool {
        match (p.first(), s.first()) {
            (None, None) => true,
            (None, Some(_)) => false,
            (Some(b'*'), _) if p.get(1) == Some(&b'*') => {
                let rest = if p.get(2) == Some(&b'/') {
                    &p[3..]
                } else {
                    &p[2..]
                };
                (0..=s.len()).any(|i| rec(rest, &s[i..]))
            }
            (Some(b'*'), _) => (0..=s.len())
                .take_while(|i| *i == 0 || s[i - 1] != b'/')
                .any(|i| rec(&p[1..], &s[i..])),
            (Some(b'?'), Some(c)) if *c != b'/' => rec(&p[1..], &s[1..]),
            (Some(a), Some(b)) if a == b => rec(&p[1..], &s[1..]),
            _ => false,
        }
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    rec(pattern.as_bytes(), path.as_bytes())
        || (!pattern.contains('/') && rec(pattern.as_bytes(), name.as_bytes()))
}

fn shown_path(root: Option<&Path>, p: &Path) -> String {
    let abs = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    root.and_then(|r| r.canonicalize().ok())
        .and_then(|r| {
            abs.strip_prefix(&r)
                .ok()
                .map(|x| x.to_string_lossy().replace('\\', "/"))
        })
        .unwrap_or_else(|| abs.to_string_lossy().replace('\\', "/"))
}

/// Attach files within `max_bytes`, applying exclusions.
pub fn gather(
    root: Option<&Path>,
    files: &[PathBuf],
    exclude: &[String],
    max_bytes: usize,
) -> Result<Context> {
    let mut ctx = Context::default();
    let mut budget = max_bytes;
    for f in files {
        let shown = shown_path(root, f);
        if let Some(pat) = ALWAYS_EXCLUDED
            .iter()
            .map(|s| s.to_string())
            .chain(exclude.iter().cloned())
            .find(|p| glob_match(p, &shown))
        {
            ctx.refused.push(Refused {
                path: shown,
                reason: format!("excluded by '{pat}'"),
            });
            continue;
        }
        let bytes = std::fs::read(f)
            .map_err(|e| Error::Config(format!("Cannot read {}: {e}", f.display())))?;
        if bytes.contains(&0) {
            ctx.refused.push(Refused {
                path: shown,
                reason: "binary file".into(),
            });
            continue;
        }
        if budget == 0 {
            ctx.refused.push(Refused {
                path: shown,
                reason: format!("context budget of {max_bytes} bytes used up"),
            });
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        let mut take = text.len().min(budget);
        while !text.is_char_boundary(take) {
            take -= 1;
        }
        budget -= take;
        ctx.text
            .push_str(&format!("File: {shown}\n```\n{}\n```\n\n", &text[..take]));
        ctx.files.push(ContextFile {
            path: shown,
            bytes: take,
            truncated: take < text.len(),
        });
    }
    Ok(ctx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs() {
        assert!(glob_match("*.pem", "certs/server.pem"));
        assert!(glob_match(".env", "app/.env"));
        assert!(glob_match("secrets/**", "secrets/a/b.txt"));
        assert!(glob_match("**/secrets/**", "x/secrets/b.txt"));
        assert!(glob_match("src/*.c", "src/main.c"));
        assert!(!glob_match("src/*.c", "src/sub/main.c"));
        assert!(glob_match("src/**/*.c", "src/sub/main.c"));
        assert!(!glob_match("*.pem", "notes.txt"));
    }

    #[test]
    fn exclusions_budget_and_binary() {
        let d = std::env::temp_dir().join(format!("embedide-ctx-{}", std::process::id()));
        std::fs::create_dir_all(d.join("secrets")).unwrap();
        std::fs::write(d.join("main.c"), "int main(void){return 0;}\n").unwrap();
        std::fs::write(d.join(".env"), "TOKEN=x").unwrap();
        std::fs::write(d.join("secrets/a.txt"), "x").unwrap();
        std::fs::write(d.join("blob.bin"), [0u8, 1, 2]).unwrap();
        std::fs::write(d.join("big.c"), "x".repeat(100)).unwrap();
        std::fs::write(d.join("private.h"), "secret").unwrap();
        let files: Vec<_> = [
            "main.c",
            ".env",
            "secrets/a.txt",
            "blob.bin",
            "big.c",
            "private.h",
        ]
        .iter()
        .map(|f| d.join(f))
        .collect();
        let c = gather(Some(&d), &files, &["private.h".into()], 60).unwrap();
        assert_eq!(
            c.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(),
            ["main.c", "big.c"]
        );
        assert!(c.files[1].truncated);
        assert_eq!(c.files.iter().map(|f| f.bytes).sum::<usize>(), 60);
        assert_eq!(c.refused.len(), 4);
        assert!(!c.text.contains("TOKEN"));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
