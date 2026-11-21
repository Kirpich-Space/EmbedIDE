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
/// Runs in O(pattern * path) time with no exponential backtracking.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    fn match_bytes(p: &[u8], s: &[u8]) -> bool {
        #[derive(Clone, Copy)]
        enum Tok {
            StarStar,
            Star,
            Any,
            Lit(u8),
        }
        let mut toks = Vec::with_capacity(p.len());
        let mut i = 0;
        while i < p.len() {
            if p[i] == b'*' && p.get(i + 1) == Some(&b'*') {
                toks.push(Tok::StarStar);
                i += if p.get(i + 2) == Some(&b'/') { 3 } else { 2 };
            } else if p[i] == b'*' {
                toks.push(Tok::Star);
                i += 1;
            } else if p[i] == b'?' {
                toks.push(Tok::Any);
                i += 1;
            } else {
                toks.push(Tok::Lit(p[i]));
                i += 1;
            }
        }
        let mut dp = vec![false; s.len() + 1];
        dp[0] = true;
        for tok in toks {
            let mut next = vec![false; s.len() + 1];
            match tok {
                Tok::StarStar => {
                    let mut seen = false;
                    for j in 0..=s.len() {
                        seen |= dp[j];
                        next[j] = seen;
                    }
                }
                Tok::Star => {
                    let mut seen = false;
                    for j in 0..=s.len() {
                        if j > 0 && s[j - 1] == b'/' {
                            seen = false;
                        }
                        seen |= dp[j];
                        next[j] = seen;
                    }
                }
                Tok::Any => {
                    for j in 0..s.len() {
                        if dp[j] && s[j] != b'/' {
                            next[j + 1] = true;
                        }
                    }
                }
                Tok::Lit(ch) => {
                    for j in 0..s.len() {
                        if dp[j] && s[j] == ch {
                            next[j + 1] = true;
                        }
                    }
                }
            }
            dp = next;
        }
        dp[s.len()]
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    match_bytes(pattern.as_bytes(), path.as_bytes())
        || (!pattern.contains('/') && match_bytes(pattern.as_bytes(), name.as_bytes()))
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
        let file = std::fs::File::open(f)
            .map_err(|e| Error::Config(format!("Cannot read {}: {e}", f.display())))?;
        let meta = file
            .metadata()
            .map_err(|e| Error::Config(format!("Cannot read {}: {e}", f.display())))?;
        if !meta.is_file() {
            ctx.refused.push(Refused {
                path: shown,
                reason: "not a regular file".into(),
            });
            continue;
        }
        // Bound the read so huge binary files cannot exhaust memory.
        let read_cap = budget.clamp(8192, 16 * 1024 * 1024).saturating_add(1);
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut std::io::Read::take(file, read_cap as u64), &mut bytes)
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
        let more_on_disk = meta.len() > bytes.len() as u64;
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
            truncated: more_on_disk || take < text.len(),
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
        // Pathological pattern must complete instantly (no exponential backtracking).
        assert!(!glob_match(
            "**/**/**/**/**/**/**/**/**/x",
            "a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p/q/r/s/t/u/v/w/y"
        ));
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
