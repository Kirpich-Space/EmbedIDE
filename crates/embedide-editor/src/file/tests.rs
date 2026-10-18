//! Save staging invariants, including failures before the commit point.
use super::*;
use std::io::Write;

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "embedide-staging-{}-{stamp}-{}",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn staging_error_cleans_up_without_touching_the_source() {
    let directory = Directory::new();
    let source = directory.0.join("source.c");
    fs::write(&source, "original").unwrap();
    let fail = || -> io::Result<()> {
        let (_temporary, mut file) = Temporary::create(&directory.0)?;
        file.write_all(b"partial replacement")?;
        Err(io::Error::new(
            io::ErrorKind::WriteZero,
            "injected write failure",
        ))
    };
    assert_eq!(fail().unwrap_err().kind(), io::ErrorKind::WriteZero);
    assert_eq!(fs::read_to_string(source).unwrap(), "original");
    assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 1);
}

#[test]
fn reserving_a_temporary_never_truncates_an_existing_entry() {
    let directory = Directory::new();
    let path = directory.0.join("occupied.tmp");
    fs::write(&path, "another process").unwrap();
    assert_eq!(
        platform::create_temporary(&path).unwrap_err().kind(),
        io::ErrorKind::AlreadyExists
    );
    assert_eq!(fs::read_to_string(path).unwrap(), "another process");
}

#[cfg(unix)]
#[test]
fn temporary_contents_are_private_before_source_permissions_are_applied() {
    use std::os::unix::fs::PermissionsExt;
    let directory = Directory::new();
    let (temporary, mut file) = Temporary::create(&directory.0).unwrap();
    file.write_all(b"private source").unwrap();
    assert_eq!(file.metadata().unwrap().permissions().mode() & 0o077, 0);
    drop(file);
    drop(temporary);
    assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn protection_checks_detect_metadata_changes_even_without_content_changes() {
    use std::os::unix::fs::PermissionsExt;
    let directory = Directory::new();
    let path = directory.0.join("source.c");
    fs::write(&path, "same bytes").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let before = fs::metadata(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    assert!(!platform::same_protection(
        &before,
        &fs::metadata(&path).unwrap()
    ));
}
