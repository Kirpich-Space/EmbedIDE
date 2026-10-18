//! Lazy directory access and guarded project filesystem operations.
use crate::{Error, Result};
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

/// A single visible directory entry. Symlinks are displayed but never traversed.
#[derive(Debug, Clone)]
pub struct Entry {
    /// Absolute path.
    pub path: PathBuf,
    /// Directory rather than regular file.
    pub directory: bool,
    /// Symlinks cannot be opened/mutated by this service.
    pub symlink: bool,
}
/// Read exactly one directory, bounded to 4096 entries. No recursive startup scan.
/// Common generated directories and simple .gitignore names are excluded.
pub fn read_directory(root: &Path, directory: &Path) -> Result<Vec<Entry>> {
    let directory = directory.canonicalize()?;
    if !directory.starts_with(root) {
        return Err(Error("Explorer path leaves workspace".into()));
    }
    let ignore = fs::read_to_string(root.join(".gitignore")).unwrap_or_default();
    let ignored = |name: &str| {
        [".git", "target", "node_modules", ".embedide-build"].contains(&name)
            || ignore
                .lines()
                .map(str::trim)
                .filter(|s| !s.starts_with(['#', '!']) && !s.contains('*'))
                .any(|s| s.trim_matches('/') == name)
    };
    let mut entries = Vec::new();
    for item in fs::read_dir(directory)? {
        let item = item?;
        if ignored(&item.file_name().to_string_lossy()) {
            continue;
        }
        if entries.len() == 4096 {
            return Err(Error(
                "Directory exceeds 4096 visible entries; use Open File with an explicit path"
                    .into(),
            ));
        }
        let kind = item.file_type()?;
        entries.push(Entry {
            path: item.path(),
            directory: kind.is_dir(),
            symlink: kind.is_symlink(),
        });
    }
    entries.sort_by(|a, b| {
        b.directory
            .cmp(&a.directory)
            .then_with(|| a.path.file_name().cmp(&b.path.file_name()))
    });
    Ok(entries)
}
/// Resolve a project-relative destination, rejecting escapes and symlink parents.
pub fn destination(root: &Path, relative: &str) -> Result<PathBuf> {
    let path = Path::new(relative);
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(Error(
            "Use a nonempty workspace-relative path without ..".into(),
        ));
    }
    let mut current = root.to_path_buf();
    let components: Vec<_> = path.components().collect();
    for component in &components[..components.len() - 1] {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(Error("Parent must be a real workspace directory".into()));
        }
    }
    current.push(components.last().expect("nonempty").as_os_str());
    Ok(current)
}
/// Create exclusively; never overwrite existing paths.
pub fn create(root: &Path, relative: &str, directory: bool) -> Result<PathBuf> {
    let path = destination(root, relative)?;
    if directory {
        fs::create_dir(&path)?;
    } else {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?
            .sync_all()?;
    }
    Ok(path)
}
/// Rename without replacement. Source must be inside root and not a symlink.
/// Caller must close documents under source first. Files use exclusive hard-link creation
/// then unlink; directories must be empty. A failed unlink may leave both names.
pub fn rename(root: &Path, source: &Path, relative: &str) -> Result<()> {
    guarded_source(root, source)?;
    let destination = destination(root, relative)?;
    if fs::symlink_metadata(&destination).is_ok() {
        return Err(Error("Rename destination already exists".into()));
    }
    if source.is_dir() {
        if fs::read_dir(source)?.next().is_some() {
            return Err(Error(
                "Only empty directories can be renamed safely by this frontend".into(),
            ));
        }
        fs::create_dir(&destination)?;
        fs::remove_dir(source).map_err(|e| {
            Error(format!(
                "Created {}; original directory could not be removed: {e}",
                destination.display()
            ))
        })?;
    } else {
        fs::hard_link(source, &destination).map_err(|e| {
            Error(format!(
                "Cannot rename without replacement (filesystem must support hard links): {e}"
            ))
        })?;
        fs::remove_file(source).map_err(|e| {
            Error(format!(
                "Created {}; original link could not be removed: {e}",
                destination.display()
            ))
        })?;
    }
    Ok(())
}
fn guarded_source(root: &Path, source: &Path) -> Result<()> {
    let relative = source
        .strip_prefix(root)
        .map_err(|_| Error("Path leaves workspace".into()))?;
    let relative = relative
        .to_str()
        .ok_or_else(|| Error("Filesystem operation requires a UTF-8 relative path".into()))?;
    let verified = destination(root, relative)?;
    let metadata = fs::symlink_metadata(verified)?;
    if metadata.file_type().is_symlink() {
        return Err(Error("Symlink operations are disabled".into()));
    }
    Ok(())
}
/// Delete a file or empty directory only, after frontend confirmation. Never recursive.
pub fn delete(root: &Path, path: &Path) -> Result<()> {
    guarded_source(root, path)?;
    if path.is_dir() {
        fs::remove_dir(path)?;
    } else {
        fs::remove_file(path)?;
    }
    Ok(())
}
