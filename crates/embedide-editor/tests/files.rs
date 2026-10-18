//! Disposable file fixtures exercise save, reload, conflicts and failure paths.
#[cfg(unix)]
use embedide_editor::file::FileBinding;
use embedide_editor::{vim::Key, Editor, EditorError, Effect};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "embedide-file-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.path(name);
        fs::write(&path, text).unwrap();
        path
    }
    fn no_temporaries(&self) {
        assert!(fs::read_dir(&self.0).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".embedide-")));
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn command(editor: &mut Editor, text: &str) -> Result<Effect, EditorError> {
    editor.feed(Key::Char(':'))?;
    editor.feed(Key::Text(text.into()))?;
    editor.feed(Key::Enter)
}
fn append(editor: &mut Editor, text: &str) {
    editor.feed(Key::Char('A')).unwrap();
    editor.feed(Key::Text(text.into())).unwrap();
    editor.feed(Key::Escape).unwrap();
}

#[test]
fn utf8_bom_crlf_save_reload_and_savepoint_history() {
    let f = Fixture::new();
    let path = f.write("space ; Unicode-я.c", "\u{feff}one\r\ntwo\r\n");
    let mut editor = Editor::open(&path).unwrap();
    assert!(!editor.state().is_modified());
    append(&mut editor, "猫");
    assert!(editor.state().is_modified());
    assert!(matches!(
        command(&mut editor, "w").unwrap(),
        Effect::Saved(_)
    ));
    let expected = "\u{feff}one猫\r\ntwo\r\n";
    assert_eq!(fs::read(&path).unwrap(), expected.as_bytes());
    assert!(!editor.state().is_modified());
    let reloaded = Editor::open(&path).unwrap();
    assert_eq!(reloaded.state().buffer().contents(), expected);
    editor.feed(Key::Char('u')).unwrap();
    assert!(editor.state().is_modified());
    editor.feed(Key::Ctrl('r')).unwrap();
    assert!(!editor.state().is_modified());
    f.no_temporaries();
}

#[test]
fn new_file_is_created_only_on_save_and_then_can_be_saved_again() {
    let f = Fixture::new();
    let path = f.path("new.rs");
    let mut editor = Editor::create(&path).unwrap();
    assert!(!path.exists());
    append(&mut editor, "first");
    command(&mut editor, "w").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "first");
    editor.feed(Key::Char('A')).unwrap();
    editor.feed(Key::Text(" second".into())).unwrap();
    editor.save().unwrap();
    editor.feed(Key::Escape).unwrap();
    assert!(!editor.state().is_modified());
    assert_eq!(fs::read_to_string(&path).unwrap(), "first second");
    assert!(Editor::create(&path).is_err());
    f.no_temporaries();
}

#[test]
fn dirty_quit_is_refused_forcequit_is_explicit_and_writequit_saves() {
    let f = Fixture::new();
    let path = f.write("main.c", "old");
    let mut editor = Editor::open(&path).unwrap();
    append(&mut editor, "!");
    assert!(matches!(
        command(&mut editor, "q"),
        Err(EditorError::UnsavedChanges)
    ));
    assert!(!editor.state().is_closed());
    editor.feed(Key::Escape).unwrap(); // Failed Ex action retained its prompt.
    assert_eq!(command(&mut editor, "q!").unwrap(), Effect::Closed);
    assert_eq!(fs::read_to_string(&path).unwrap(), "old");
    assert!(matches!(
        editor.feed(Key::Char('i')),
        Err(EditorError::Closed)
    ));
    let mut editor = Editor::open(&path).unwrap();
    append(&mut editor, "new");
    assert_eq!(command(&mut editor, "wq").unwrap(), Effect::Closed);
    assert_eq!(fs::read_to_string(&path).unwrap(), "oldnew");
}

#[test]
fn external_change_deletion_and_new_file_collision_are_not_overwritten() {
    let f = Fixture::new();
    let path = f.write("main.c", "original");
    let mut editor = Editor::open(&path).unwrap();
    append(&mut editor, " local");
    fs::write(&path, "external").unwrap(); // Same byte length as the baseline.
    assert!(matches!(
        command(&mut editor, "wq"),
        Err(EditorError::FileChanged)
    ));
    assert_eq!(fs::read_to_string(&path).unwrap(), "external");
    assert!(editor.state().is_modified());
    assert!(!editor.state().is_closed());
    editor.feed(Key::Escape).unwrap();
    fs::remove_file(&path).unwrap();
    assert!(matches!(
        command(&mut editor, "w"),
        Err(EditorError::FileChanged)
    ));
    assert!(!path.exists());
    let path = f.path("new.c");
    let mut editor = Editor::create(&path).unwrap();
    append(&mut editor, "mine");
    fs::write(&path, "theirs").unwrap();
    assert!(matches!(
        command(&mut editor, "w"),
        Err(EditorError::FileChanged)
    ));
    assert_eq!(fs::read_to_string(&path).unwrap(), "theirs");
    f.no_temporaries();
}

#[test]
fn invalid_encoding_missing_directory_and_untitled_save_are_errors() {
    let f = Fixture::new();
    let path = f.path("bad.bin");
    fs::write(&path, [0xff, 0xfe]).unwrap();
    assert!(
        matches!(Editor::open(&path), Err(EditorError::Io(ref e)) if e.kind() == std::io::ErrorKind::InvalidData)
    );
    assert!(Editor::create(&f.path("missing/new.c")).is_err());
    assert!(Editor::open(&f.path("absent.c")).is_err());
    assert!(Editor::open(&f.0).is_err());
    let mut editor = Editor::new("unsaved".into());
    assert!(matches!(
        command(&mut editor, "w"),
        Err(EditorError::NoPath)
    ));
    assert!(editor.state().is_modified());
}

#[test]
fn invalid_filename_and_removed_parent_keep_unsaved_text_in_memory() {
    let f = Fixture::new();
    assert!(Editor::create(&f.path("invalid\0name")).is_err());
    let directory = f.path("removed");
    fs::create_dir(&directory).unwrap();
    let path = directory.join("new.c");
    let mut editor = Editor::create(&path).unwrap();
    append(&mut editor, "not lost");
    fs::remove_dir(&directory).unwrap();
    assert!(editor.save().is_err());
    assert!(editor.state().is_modified());
    assert_eq!(editor.state().buffer().contents(), "not lost");
    assert!(!path.exists());
    fs::create_dir(&directory).unwrap();
    editor.save().unwrap();
    assert_eq!(
        Editor::open(&path).unwrap().state().buffer().contents(),
        "not lost"
    );
    f.no_temporaries();
}

#[cfg(unix)]
#[test]
fn unwritable_parent_refuses_staging_and_allows_retry_without_data_loss() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let path = f.write("main.c", "original");
    let mut editor = Editor::open(&path).unwrap();
    append(&mut editor, " changed");
    fs::set_permissions(&f.0, fs::Permissions::from_mode(0o500)).unwrap();
    let result = editor.save();
    // Restore before assertions so fixture cleanup also works after a failure.
    fs::set_permissions(&f.0, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(
        matches!(result, Err(EditorError::Io(ref e)) if e.kind() == std::io::ErrorKind::PermissionDenied)
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "original");
    assert_eq!(editor.state().buffer().contents(), "original changed");
    assert!(editor.state().is_modified());
    f.no_temporaries();
    editor.save().unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "original changed");
    assert!(!editor.state().is_modified());
}

#[cfg(unix)]
#[test]
fn replacement_with_identical_bytes_and_mtime_is_detected_by_file_identity() {
    let f = Fixture::new();
    let path = f.write("main.c", "same bytes");
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    let mut editor = Editor::open(&path).unwrap();
    append(&mut editor, " local");
    let replacement = f.write("replacement.c", "same bytes");
    fs::File::options()
        .write(true)
        .open(&replacement)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(modified))
        .unwrap();
    fs::rename(replacement, &path).unwrap();
    assert!(matches!(editor.save(), Err(EditorError::FileChanged)));
    assert_eq!(fs::read_to_string(&path).unwrap(), "same bytes");
    assert!(editor.state().is_modified());
    f.no_temporaries();
}

#[cfg(unix)]
#[test]
fn symlink_substitution_after_open_never_writes_through_to_its_target() {
    let f = Fixture::new();
    let path = f.write("main.c", "original");
    let target = f.write("other.c", "untouched");
    let mut editor = Editor::open(&path).unwrap();
    append(&mut editor, " local");
    fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(&target, &path).unwrap();
    assert!(matches!(editor.save(), Err(EditorError::SymbolicLink)));
    assert_eq!(fs::read_to_string(target).unwrap(), "untouched");
    assert_eq!(editor.state().buffer().contents(), "original local");
    assert!(editor.state().is_modified());
    f.no_temporaries();
}

#[cfg(unix)]
#[test]
fn final_symlinks_and_multiple_hard_links_are_refused_for_save() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    let target = f.write("target.c", "original");
    let link = f.path("link.c");
    symlink(&target, &link).unwrap();
    assert!(matches!(
        FileBinding::open(&link),
        Err(EditorError::SymbolicLink)
    ));
    let mut editor = Editor::open(&target).unwrap();
    append(&mut editor, " local");
    let hard = f.path("hard.c");
    fs::hard_link(&target, &hard).unwrap();
    assert!(matches!(
        command(&mut editor, "w"),
        Err(EditorError::Unsupported(_))
    ));
    assert_eq!(fs::read_to_string(&hard).unwrap(), "original");
    fs::remove_file(&hard).unwrap();
    editor.feed(Key::Escape).unwrap();
    fs::remove_file(&target).unwrap();
    symlink(&link, &target).unwrap();
    assert!(matches!(
        command(&mut editor, "w"),
        Err(EditorError::SymbolicLink)
    ));
}

#[cfg(unix)]
#[test]
fn permission_bits_survive_replace_and_readonly_failure_keeps_data() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let path = f.write("main.c", "source");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    let mut editor = Editor::open(&path).unwrap();
    append(&mut editor, "!");
    command(&mut editor, "w").unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o640
    );
    fs::set_permissions(&path, fs::Permissions::from_mode(0o440)).unwrap();
    append(&mut editor, "?");
    assert!(
        matches!(command(&mut editor, "w"), Err(EditorError::Io(ref e)) if e.kind() == std::io::ErrorKind::PermissionDenied)
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "source!");
    assert!(editor.state().is_modified());
    f.no_temporaries();
}
