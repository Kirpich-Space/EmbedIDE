//! UTF-8 file bindings and same-directory staged saves.
//!
//! Final symlinks are refused. Parent directories are resolved when binding.
//! Content fingerprint, identity and timestamps detect ordinary external edits.
//! This is not a lock or an adversarial-race-proof compare-and-swap. Permissions
//! are copied; Unix owner/group mismatch is refused. ACLs and extended attributes
//! are not preserved by API.
//! Rename/directory-sync guarantees depend on the OS, filesystem and device.
mod platform;
#[cfg(test)]
mod tests;

use crate::{EditorError, TextBuffer};
use std::{
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::SystemTime,
};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
struct Snapshot {
    len: u64,
    hash: u64,
    modified: Option<SystemTime>,
    identity: platform::Identity,
}

fn hash_bytes(hash: &mut u64, bytes: &[u8]) {
    // FNV-1a: a streaming change detector, not a cryptographic integrity claim.
    for byte in bytes {
        *hash = (*hash ^ u64::from(*byte)).wrapping_mul(1_099_511_628_211);
    }
}

fn entry(path: &Path) -> Result<fs::Metadata, EditorError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(EditorError::SymbolicLink);
    }
    if !metadata.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a regular file").into());
    }
    Ok(metadata)
}

fn same_metadata(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    a.len() == b.len()
        && a.modified().ok() == b.modified().ok()
        && platform::identity(a) == platform::identity(b)
        && platform::same_protection(a, b)
}

fn read(path: &Path, capture: bool) -> Result<(Snapshot, Vec<u8>), EditorError> {
    let initial = entry(path)?;
    let mut file = File::open(path)?;
    let before = file.metadata()?;
    if !same_metadata(&initial, &before) {
        return Err(EditorError::FileChanged);
    }
    let mut bytes = Vec::new();
    let mut hash = 14_695_981_039_346_656_037;
    let mut len = 0;
    let mut block = [0; 65536];
    loop {
        let count = file.read(&mut block)?;
        if count == 0 {
            break;
        }
        hash_bytes(&mut hash, &block[..count]);
        len += count as u64;
        if capture {
            bytes.extend_from_slice(&block[..count]);
        }
    }
    let after = file.metadata()?;
    if !same_metadata(&before, &after)
        || len != after.len()
        || !same_metadata(&after, &entry(path)?)
    {
        return Err(EditorError::FileChanged);
    }
    Ok((
        Snapshot {
            len,
            hash,
            modified: after.modified().ok(),
            identity: platform::identity(&after),
        },
        bytes,
    ))
}

fn resolved_path(path: &Path) -> Result<PathBuf, EditorError> {
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "file path has no filename"))?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    Ok(parent.canonicalize()?.join(name))
}

/// Result of a completed staged save. No universal power-loss guarantee implied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveReport {
    /// Parent-directory synchronization succeeded on this platform.
    pub directory_synced: bool,
    /// An existing file was replaced rather than a new entry created.
    pub replaced_existing: bool,
}

/// Path and last observed disk state; separate from mutable editor text.
#[derive(Debug)]
pub struct FileBinding {
    path: PathBuf,
    baseline: Option<Snapshot>,
}

impl FileBinding {
    /// Read a regular UTF-8 file. Invalid encodings produce an explicit error.
    pub fn open(path: &Path) -> Result<(Self, String), EditorError> {
        let path = resolved_path(path)?;
        let (baseline, bytes) = read(&path, true)?;
        let text =
            String::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok((
            Self {
                path,
                baseline: Some(baseline),
            },
            text,
        ))
    }
    /// Bind a not-yet-existing file in an existing directory, without writing it.
    pub fn create(path: &Path) -> Result<Self, EditorError> {
        let path = resolved_path(path)?;
        match fs::symlink_metadata(&path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self {
                path,
                baseline: None,
            }),
            Err(e) => Err(e.into()),
            Ok(_) => Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "new file path already exists",
            )
            .into()),
        }
    }
    /// Bound path with its parent resolved at open/create time.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn check(&self) -> Result<(), EditorError> {
        if let Some(expected) = &self.baseline {
            let actual = match read(&self.path, false) {
                Ok((snapshot, _)) => snapshot,
                Err(EditorError::Io(e)) if e.kind() == io::ErrorKind::NotFound => {
                    return Err(EditorError::FileChanged)
                }
                Err(e) => return Err(e),
            };
            if actual != *expected {
                return Err(EditorError::FileChanged);
            }
        } else {
            match fs::symlink_metadata(&self.path) {
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
                Ok(_) => return Err(EditorError::FileChanged),
            }
        }
        Ok(())
    }

    /// Write/sync a unique temporary file, recheck disk state, then commit.
    /// New files use exclusive hard-link creation to avoid overwriting a race.
    /// Existing-file replacement uses the platform rename adapter; the residual
    /// check-to-rename race requires future OS-specific locking/handle APIs.
    pub fn save(&mut self, buffer: &TextBuffer) -> Result<SaveReport, EditorError> {
        self.check()?;
        let original = if self.baseline.is_some() {
            let metadata = entry(&self.path)?;
            platform::validate_save(&metadata)?;
            if metadata.permissions().readonly() {
                return Err(
                    io::Error::new(io::ErrorKind::PermissionDenied, "file is read-only").into(),
                );
            }
            Some(metadata)
        } else {
            None
        };
        let parent = self.path.parent().ok_or(EditorError::NoPath)?;
        let (temporary, mut file) = Temporary::create(parent)?;
        buffer.write_to(&mut file)?;
        if let Some(metadata) = &original {
            file.set_permissions(metadata.permissions())?;
        }
        file.sync_all()?;
        let metadata = file.metadata()?;
        if let Some(original) = &original {
            platform::validate_replacement(original, &metadata)?;
        }
        let mut hash = 14_695_981_039_346_656_037;
        for chunk in buffer.chunks() {
            hash_bytes(&mut hash, chunk.as_bytes());
        }
        let saved = Snapshot {
            len: metadata.len(),
            hash,
            modified: metadata.modified().ok(),
            identity: platform::identity(&metadata),
        };
        drop(file); // Windows cannot rename an open temporary with every sharing mode.
        self.check()?;
        if let Some(original) = &original {
            // Also detect permission/owner/link changes while staging. The final
            // check-to-rename window still needs OS-specific handle APIs.
            if !platform::same_protection(original, &entry(&self.path)?) {
                return Err(EditorError::FileChanged);
            }
        }
        let replaced_existing = self.baseline.is_some();
        if replaced_existing {
            platform::replace(&temporary.0, &self.path)?;
        } else {
            fs::hard_link(&temporary.0, &self.path)?;
        }
        drop(temporary);
        self.baseline = Some(saved);
        Ok(SaveReport {
            directory_synced: platform::sync_directory(parent),
            replaced_existing,
        })
    }
}

struct Temporary(PathBuf);
impl Temporary {
    fn create(parent: &Path) -> io::Result<(Self, File)> {
        for _ in 0..64 {
            let number = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = parent.join(format!(".embedide-{}-{number}.tmp", std::process::id()));
            match platform::create_temporary(&path) {
                Ok(file) => return Ok((Self(path), file)),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "cannot reserve a save temporary",
        ))
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
