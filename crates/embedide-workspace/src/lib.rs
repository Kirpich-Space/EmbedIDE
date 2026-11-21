//! Project manifests, user configuration and templates.
//!
//! A project is described by `embedide.toml` (schema 1). Existing projects
//! with only `embedide.json`, a Makefile, CMakeLists.txt, Cargo.toml or
//! build.zig keep working through the legacy path in `embedide-core`.
//! Nothing here overwrites user files: creation uses exclusive opens and
//! edits rewrite only the file being configured, atomically.

pub mod config;
pub mod launch;
pub mod manifest;
pub mod templates;

pub use config::{config_dir, ConfigFile, UserConfig};
pub use launch::{resolve_launch, Launch, LaunchFile};
pub use manifest::{BuildKind, Manifest, Profile, MANIFEST_FILE};

use std::fmt;

/// Workspace error with an actionable message.
#[derive(Debug)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self(e.to_string())
    }
}

/// Result type of this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Write a file atomically (temp file + rename), user-only permissions on Unix.
pub fn write_atomic(path: &std::path::Path, bytes: &[u8], private: bool) -> Result<()> {
    use std::{
        io::Write,
        sync::atomic::{AtomicU64, Ordering},
    };
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let (tmp, mut f) = {
        let mut created = None;
        for _ in 0..64 {
            let n = SEQ.fetch_add(1, Ordering::Relaxed);
            let candidate = dir.join(format!(".{name}.{}.{n}.tmp", std::process::id()));
            let mut opts = std::fs::OpenOptions::new();
            opts.write(true).create_new(true);
            #[cfg(unix)]
            if private {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600);
            }
            #[cfg(not(unix))]
            let _ = private;
            match opts.open(&candidate) {
                Ok(file) => {
                    created = Some((candidate, file));
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.into()),
            }
        }
        created.ok_or_else(|| {
            Error(format!(
                "Cannot reserve a temporary file in {}",
                dir.display()
            ))
        })?
    };
    if let Err(e) = f.write_all(bytes).and_then(|()| f.sync_all()) {
        drop(f);
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    drop(f);
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })?;
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

/// Keys that look like credentials must never be stored in project files.
pub fn looks_secret(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    let word = |w: &str| k == w || k.ends_with(&format!("_{w}")) || k.ends_with(&format!("-{w}"));
    [
        "api_key",
        "apikey",
        "api-key",
        "secret",
        "password",
        "passwd",
        "credential",
    ]
    .iter()
    .any(|w| k.contains(w))
        || word("token")
        || word("key")
}

#[cfg(test)]
mod tests {
    #[test]
    fn secret_detection_has_no_false_positive_on_limits() {
        for s in [
            "api_key",
            "OPENAI_API_KEY",
            "token",
            "access_token",
            "client_secret",
            "key",
            "x-api-key",
        ] {
            assert!(super::looks_secret(s), "{s}");
        }
        for s in ["max_tokens", "keymap", "keys", "tokenizer", "model"] {
            assert!(!super::looks_secret(s), "{s}");
        }
    }
}
