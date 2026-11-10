//! Resolve the path given to `embed <path>` into a workspace and a document.
//!
//! Resolution only reads metadata: nothing is created or written. Paths stay
//! platform-native (`OsStr`) throughout, so spaces, Cyrillic and other
//! Unicode names and non-UTF-8 Unix names survive unchanged.
use crate::{Error, Result, MANIFEST_FILE};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// The document a launch path names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchFile {
    /// An existing regular file, canonical (symbolic links resolved).
    Existing(PathBuf),
    /// A file that does not exist yet, in an existing directory (canonical
    /// parent). It is created only when the user saves it.
    New(PathBuf),
}

/// Workspace root plus an optional document to open in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    /// Canonical workspace directory.
    pub root: PathBuf,
    /// Document to open, if the path named a file.
    pub file: Option<LaunchFile>,
}

/// Resolve `path` (relative to `cwd` unless absolute).
///
/// * a directory becomes the workspace;
/// * a file opens in the nearest enclosing directory with `embedide.toml`,
///   otherwise in `cwd` when the file is inside it, otherwise in its own
///   directory — a project manifest is never required;
/// * a missing file in an existing directory becomes a new, unsaved document;
/// * a missing parent directory or a dangling symbolic link is an error.
pub fn resolve_launch(path: &Path, cwd: &Path) -> Result<Launch> {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    match fs::metadata(&joined) {
        Ok(meta) if meta.is_dir() => Ok(Launch {
            root: joined.canonicalize()?,
            file: None,
        }),
        Ok(meta) if meta.is_file() => {
            let file = joined.canonicalize()?;
            let dir = file.parent().unwrap_or(&file).to_path_buf();
            Ok(Launch {
                root: project_root(&dir, &cwd),
                file: Some(LaunchFile::Existing(file)),
            })
        }
        Ok(_) => Err(Error(format!(
            "{} is not a regular file or directory",
            path.display()
        ))),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            if fs::symlink_metadata(&joined).is_ok() {
                return Err(Error(format!(
                    "{} is a symbolic link to a missing target",
                    path.display()
                )));
            }
            let name = joined
                .file_name()
                .ok_or_else(|| Error(format!("{} does not name a file", path.display())))?;
            let parent = joined
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(&cwd);
            let dir = match fs::metadata(parent) {
                Ok(meta) if meta.is_dir() => parent.canonicalize()?,
                Ok(_) => {
                    return Err(Error(format!(
                        "Cannot create {}: {} is not a directory",
                        path.display(),
                        parent.display()
                    )))
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    return Err(Error(format!(
                        "Cannot create {}: parent directory {} does not exist",
                        path.display(),
                        parent.display()
                    )))
                }
                Err(e) => return Err(e.into()),
            };
            Ok(Launch {
                root: project_root(&dir, &cwd),
                file: Some(LaunchFile::New(dir.join(name))),
            })
        }
        Err(e) => Err(Error(format!("Cannot open {}: {e}", path.display()))),
    }
}

fn project_root(dir: &Path, cwd: &Path) -> PathBuf {
    if let Some(project) = dir.ancestors().find(|a| a.join(MANIFEST_FILE).is_file()) {
        return project.to_path_buf();
    }
    if dir.starts_with(cwd) {
        cwd.to_path_buf()
    } else {
        dir.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "embedide-launch-{name}-{}-Кириллица с пробелом",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    #[test]
    fn directories_files_and_new_files_resolve_without_writing() {
        let cwd = temp("basic");
        fs::create_dir(cwd.join("src")).unwrap();
        fs::write(cwd.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(cwd.join("file with spaces.cpp"), "").unwrap();
        fs::write(cwd.join("файл.c"), "").unwrap();

        let dir = resolve_launch(Path::new("src/"), &cwd).unwrap();
        assert_eq!(dir.root, cwd.join("src"));
        assert_eq!(dir.file, None);

        let file = resolve_launch(Path::new("./src/main.rs"), &cwd).unwrap();
        assert_eq!(
            file.root, cwd,
            "a file inside cwd keeps cwd as the workspace"
        );
        assert_eq!(
            file.file,
            Some(LaunchFile::Existing(cwd.join("src/main.rs")))
        );
        let elsewhere = temp("elsewhere");
        for name in ["file with spaces.cpp", "файл.c"] {
            let launch = resolve_launch(&cwd.join(name), &elsewhere).unwrap();
            assert_eq!(launch.root, cwd, "outside cwd: the file's directory");
            assert_eq!(launch.file, Some(LaunchFile::Existing(cwd.join(name))));
        }

        let new = resolve_launch(Path::new("new.c"), &cwd).unwrap();
        assert_eq!(new.file, Some(LaunchFile::New(cwd.join("new.c"))));
        assert!(
            !cwd.join("new.c").exists(),
            "resolution never creates files"
        );

        let missing = resolve_launch(Path::new("nope/new.c"), &cwd).unwrap_err();
        assert!(missing.0.contains("parent directory"), "{}", missing.0);
        let _ = fs::remove_dir_all(&cwd);
        let _ = fs::remove_dir_all(&elsewhere);
    }

    #[test]
    fn files_open_in_the_enclosing_project_and_symlinks_resolve() {
        let cwd = temp("project");
        fs::create_dir_all(cwd.join("proj/kernel")).unwrap();
        fs::write(cwd.join("proj").join(MANIFEST_FILE), "schema = 1\n").unwrap();
        fs::write(cwd.join("proj/kernel/main.c"), "").unwrap();
        let launch = resolve_launch(&cwd.join("proj/kernel/main.c"), &cwd).unwrap();
        assert_eq!(launch.root, cwd.join("proj"));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(cwd.join("proj/kernel/main.c"), cwd.join("link.c")).unwrap();
            let link = resolve_launch(Path::new("link.c"), &cwd).unwrap();
            assert_eq!(
                link.file,
                Some(LaunchFile::Existing(cwd.join("proj/kernel/main.c")))
            );
            std::os::unix::fs::symlink(cwd.join("gone.c"), cwd.join("dangling.c")).unwrap();
            assert!(resolve_launch(Path::new("dangling.c"), &cwd).is_err());
        }
        let _ = fs::remove_dir_all(&cwd);
    }
}
