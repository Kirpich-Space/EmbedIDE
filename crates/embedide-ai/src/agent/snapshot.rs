//! Workspace snapshot taken before an agent turn, so the user can review
//! exactly what changed (whatever tool or command changed it) and revert a
//! file. Generated directories are skipped; large files are tracked by size
//! and modification time only (changes are reported, not revertible).
use crate::edit::unified_diff;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::SystemTime,
};

/// Files larger than this are tracked by metadata only.
pub const MAX_FILE: u64 = 1024 * 1024;
/// Files tracked at all (beyond this the snapshot is partial and new files
/// are not reported).
const MAX_ENTRIES: usize = 200_000;

/// Directories never snapshotted (build output, caches, VCS internals).
const SKIP_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    ".zig-cache",
    "zig-cache",
    "zig-out",
    "__pycache__",
    ".venv",
    "build",
    ".cache",
    ".mypy_cache",
    ".pytest_cache",
    ".hg",
    ".svn",
];

#[derive(Debug, Clone, PartialEq, Eq)]
enum Entry {
    Content(Vec<u8>),
    Large(u64, Option<SystemTime>),
}

/// How a file changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    /// Created.
    Added,
    /// Content changed.
    Modified,
    /// Removed.
    Deleted,
}

impl ChangeKind {
    /// Short label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Modified => "modified",
            Self::Deleted => "deleted",
        }
    }
}

/// One changed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// Path relative to the workspace.
    pub path: PathBuf,
    /// What happened.
    pub kind: ChangeKind,
    /// Whether [`Snapshot::revert`] can restore it.
    pub revertible: bool,
    /// Fingerprint of the file as the agent left it (revert refuses when
    /// the file changed again since).
    after: Option<u64>,
}

/// Files of a workspace at one moment.
#[derive(Debug, Clone)]
pub struct Snapshot {
    root: PathBuf,
    files: BTreeMap<PathBuf, Entry>,
    /// Why some files are tracked by metadata only (limits reached), if so.
    pub incomplete: Option<String>,
    /// The walk stopped early: files it did not reach are not compared.
    partial: bool,
}

fn fingerprint(data: &[u8]) -> u64 {
    // FNV-1a: only used to notice a later change, not for security.
    let mut h: u64 = 0xcbf29ce484222325;
    for b in data {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    h ^ data.len() as u64
}

fn skip_dir(name: &str) -> bool {
    SKIP_DIRS.contains(&name) || name.starts_with("cmake-build-")
}

/// Walk `root` (no symlinks followed), calling `f` for each regular file.
fn walk(root: &Path, mut f: impl FnMut(&Path, &std::fs::Metadata) -> bool) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let Ok(meta) = e.path().symlink_metadata() else {
                continue;
            };
            let name = e.file_name().to_string_lossy().into_owned();
            if meta.is_dir() {
                if !skip_dir(&name) {
                    stack.push(e.path());
                }
            } else if meta.is_file() && !f(&e.path(), &meta) {
                return;
            }
        }
    }
}

impl Snapshot {
    /// Record `root`, keeping at most `max_bytes` of content in `max_files` files.
    pub fn take(root: &Path, max_bytes: u64, max_files: usize) -> Self {
        let mut files = BTreeMap::new();
        let mut bytes = 0u64;
        let mut incomplete = None;
        let mut partial = false;
        walk(root, |path, meta| {
            if files.len() >= MAX_ENTRIES {
                partial = true;
                incomplete = Some(format!(
                    "more than {MAX_ENTRIES} files; the rest is not tracked"
                ));
                return false;
            }
            let Ok(rel) = path.strip_prefix(root) else {
                return true;
            };
            let over_count = files.len() >= max_files;
            let entry = if meta.len() > MAX_FILE || over_count || bytes + meta.len() > max_bytes {
                if over_count {
                    incomplete = Some(format!("more than {max_files} files"));
                } else if meta.len() <= MAX_FILE {
                    incomplete = Some(format!("more than {max_bytes} bytes of files"));
                }
                Entry::Large(meta.len(), meta.modified().ok())
            } else {
                match std::fs::read(path) {
                    Ok(data) => {
                        bytes += data.len() as u64;
                        Entry::Content(data)
                    }
                    Err(_) => return true,
                }
            };
            files.insert(rel.to_path_buf(), entry);
            true
        });
        Self {
            root: root.to_path_buf(),
            files,
            incomplete,
            partial,
        }
    }

    /// Workspace root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Number of files recorded.
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// Whether nothing was recorded.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Files that differ from the snapshot now.
    pub fn changes(&self) -> Vec<Change> {
        let mut out = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        walk(&self.root, |path, meta| {
            let Ok(rel) = path.strip_prefix(&self.root) else {
                return true;
            };
            let rel = rel.to_path_buf();
            seen.insert(rel.clone());
            let now = || std::fs::read(path).ok();
            match self.files.get(&rel) {
                None if self.partial => {}
                None => {
                    let after = (meta.len() <= MAX_FILE).then(now).flatten();
                    out.push(Change {
                        path: rel,
                        kind: ChangeKind::Added,
                        revertible: after.is_some(),
                        after: after.as_deref().map(fingerprint),
                    });
                }
                Some(Entry::Content(before)) => {
                    if meta.len() != before.len() as u64 || now().as_deref() != Some(before) {
                        let after = (meta.len() <= MAX_FILE).then(now).flatten();
                        out.push(Change {
                            path: rel,
                            kind: ChangeKind::Modified,
                            revertible: true,
                            after: after.as_deref().map(fingerprint),
                        });
                    }
                }
                Some(Entry::Large(len, modified)) => {
                    if meta.len() != *len || meta.modified().ok() != *modified {
                        out.push(Change {
                            path: rel,
                            kind: ChangeKind::Modified,
                            revertible: false,
                            after: None,
                        });
                    }
                }
            }
            true
        });
        for (rel, entry) in &self.files {
            if !seen.contains(rel) {
                out.push(Change {
                    path: rel.clone(),
                    kind: ChangeKind::Deleted,
                    revertible: matches!(entry, Entry::Content(_)),
                    after: None,
                });
            }
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        out
    }

    /// A unified diff of one change (text files), or a one-line note.
    pub fn diff(&self, change: &Change) -> String {
        let name = change.path.to_string_lossy().replace('\\', "/");
        let before = match self.files.get(&change.path) {
            Some(Entry::Content(b)) => Some(b.clone()),
            Some(Entry::Large(..)) => return format!("{name}: large file changed (no diff)\n"),
            None => None,
        };
        let after = std::fs::read(self.root.join(&change.path)).ok();
        let text = |b: &Option<Vec<u8>>| match b {
            None => Some(String::new()),
            Some(b) if b.contains(&0) => None,
            Some(b) => String::from_utf8(b.clone()).ok(),
        };
        match (text(&before), text(&after)) {
            (Some(a), Some(b)) => unified_diff(&name, &a, &b),
            _ => format!("{name}: binary file {}\n", change.kind.label()),
        }
    }

    /// Restore one changed file to its snapshot state (delete it if it was
    /// added). Refused when the file changed again after the agent's turn.
    pub fn revert(&self, change: &Change) -> Result<(), String> {
        let path = self.root.join(&change.path);
        if !change.revertible {
            return Err(format!(
                "{} cannot be reverted (large or unreadable file)",
                change.path.display()
            ));
        }
        let current = std::fs::read(&path).ok();
        if change.kind != ChangeKind::Deleted && current.as_deref().map(fingerprint) != change.after
        {
            return Err(format!(
                "{} changed again after the agent's turn; not reverted",
                change.path.display()
            ));
        }
        match (change.kind, self.files.get(&change.path)) {
            (ChangeKind::Added, _) => std::fs::remove_file(&path)
                .map_err(|e| format!("Cannot remove {}: {e}", change.path.display())),
            (_, Some(Entry::Content(data))) => {
                if change.kind == ChangeKind::Deleted && current.is_some() {
                    return Err(format!(
                        "{} exists again; not overwritten",
                        change.path.display()
                    ));
                }
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("Cannot create {}: {e}", parent.display()))?;
                }
                embedide_workspace::write_atomic(&path, data, false).map_err(|e| e.0)
            }
            _ => Err(format!("{} is not in the snapshot", change.path.display())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_are_found_diffed_and_reverted() {
        let dir = std::env::temp_dir().join(format!("embedide-snap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("build")).unwrap();
        std::fs::write(dir.join("src/main.c"), "int main(void) { return 0; }\n").unwrap();
        std::fs::write(dir.join("old.txt"), "old\n").unwrap();
        std::fs::write(dir.join("keep.txt"), "same\n").unwrap();
        let snap = Snapshot::take(&dir, 1 << 20, 100);
        assert_eq!(snap.len(), 3);
        // The agent edits, adds, deletes; build output is ignored.
        std::fs::write(dir.join("src/main.c"), "int main(void) { return 1; }\n").unwrap();
        std::fs::write(
            dir.join("src/add.c"),
            "int add(int a, int b) { return a + b; }\n",
        )
        .unwrap();
        std::fs::remove_file(dir.join("old.txt")).unwrap();
        std::fs::write(dir.join("build/app.o"), [0u8, 1, 2]).unwrap();
        let changes = snap.changes();
        let summary: Vec<(String, ChangeKind)> = changes
            .iter()
            .map(|c| (c.path.display().to_string(), c.kind))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("old.txt".into(), ChangeKind::Deleted),
                ("src/add.c".into(), ChangeKind::Added),
                ("src/main.c".into(), ChangeKind::Modified),
            ]
        );
        let diff = snap.diff(&changes[2]);
        assert!(diff.contains("-int main(void) { return 0; }"), "{diff}");
        assert!(diff.contains("+int main(void) { return 1; }"), "{diff}");
        assert!(snap.diff(&changes[1]).contains("+int add"));
        // The user edits a file afterwards: revert refuses to clobber it.
        std::fs::write(dir.join("src/main.c"), "user edit\n").unwrap();
        assert!(snap
            .revert(&changes[2])
            .unwrap_err()
            .contains("changed again"));
        for c in &changes[..2] {
            snap.revert(c).unwrap();
        }
        assert!(!dir.join("src/add.c").exists());
        assert_eq!(
            std::fs::read_to_string(dir.join("old.txt")).unwrap(),
            "old\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn limits_make_the_snapshot_incomplete() {
        let dir = std::env::temp_dir().join(format!("embedide-snap-lim-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..5 {
            std::fs::write(dir.join(format!("f{i}.c")), "x".repeat(100)).unwrap();
        }
        let snap = Snapshot::take(&dir, 250, 100);
        assert!(snap.incomplete.is_some());
        let snap = Snapshot::take(&dir, 1 << 20, 3);
        assert!(snap.incomplete.is_some());
        assert_eq!(
            snap.len(),
            5,
            "files over the limit are tracked by metadata"
        );
        assert!(snap.changes().is_empty(), "nothing is reported as added");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
