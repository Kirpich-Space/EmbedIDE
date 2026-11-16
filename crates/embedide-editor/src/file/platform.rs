//! Isolated platform details for file identity, replacement and directory sync.
use crate::EditorError;
use std::{fs, io, path::Path};

// Keep staged contents private even when the source permits fewer readers than
// the process umask. Widening to the source permissions happens after writing.
pub(super) fn create_temporary(path: &Path) -> io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

pub(super) fn same_protection(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        a.mode() == b.mode()
            && a.uid() == b.uid()
            && a.gid() == b.gid()
            && a.nlink() == b.nlink()
            && same_change_time(a, b)
    }
    #[cfg(not(unix))]
    {
        a.permissions() == b.permissions()
    }
}

/// Whether the inode change time (`ctime`) is unchanged. On the BSDs it is not compared: UFS with soft
/// updates finishes a pending inode update a few milliseconds after a `rename`, so the `ctime` of a file
/// this editor has just saved still moves once, and comparing it reported our own save as "file changed
/// externally" (measured on FreeBSD 15: the second save within a second failed about half the time).
/// Everything else that can change without touching the contents is still compared: mode, owner, group,
/// link count, size, modification time and identity.
#[cfg(unix)]
fn same_change_time(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    if cfg!(any(
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "netbsd",
        target_os = "openbsd"
    )) {
        return true;
    }
    a.ctime() == b.ctime() && a.ctime_nsec() == b.ctime_nsec()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Identity(u64, u64);

pub(super) fn identity(metadata: &fs::Metadata) -> Identity {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Identity(metadata.dev(), metadata.ino())
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        Identity(0, 0)
    }
}

pub(super) fn validate_save(metadata: &fs::Metadata) -> Result<(), EditorError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() > 1 {
            return Err(EditorError::Unsupported(
                "saving multiply-linked files would break link identity",
            ));
        }
    }
    let _ = metadata;
    Ok(())
}

pub(super) fn replace(source: &Path, target: &Path) -> io::Result<()> {
    fs::rename(source, target)
}

pub(super) fn validate_replacement(
    original: &fs::Metadata,
    temporary: &fs::Metadata,
) -> Result<(), EditorError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if original.uid() != temporary.uid() || original.gid() != temporary.gid() {
            return Err(EditorError::Unsupported(
                "replacement would change Unix owner/group; save is refused",
            ));
        }
    }
    let _ = (original, temporary);
    Ok(())
}

pub(super) fn sync_directory(path: &Path) -> bool {
    #[cfg(unix)]
    {
        fs::File::open(path)
            .and_then(|file| file.sync_all())
            .is_ok()
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        false
    }
}
