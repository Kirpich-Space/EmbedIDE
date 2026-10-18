//! Deterministic document workload. No timing thresholds or speed claims.
use embedide_editor::{vim::Key, Editor};
use std::{fs, hint::black_box, path::PathBuf, time::Instant};

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn main() {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let fixture = Fixture(std::env::temp_dir().join(format!(
        "embedide-engine-bench-{}-{stamp}",
        std::process::id()
    )));
    fs::create_dir(&fixture.0).unwrap();
    println!("bytes,open_us,insert_1000_us,delete_us,move_1000_us,undo_redo_100_us,typing_1000_us");
    for size in [1024 * 1024, 10 * 1024 * 1024] {
        let source = "0123456789abcdef\n".repeat(size / 17);
        let path = fixture.0.join("source.c");
        fs::write(&path, &source).unwrap();
        let start = Instant::now();
        let mut editor = Editor::open(&path).unwrap();
        let open = start.elapsed().as_micros();
        let start = Instant::now();
        for _ in 0..1000 {
            let offset = editor.state().buffer().byte_len() / 2;
            editor.replace(offset..offset, black_box("x")).unwrap();
        }
        let insert = start.elapsed().as_micros();
        let start = Instant::now();
        editor.replace(100..10000, "").unwrap();
        let delete = start.elapsed().as_micros();
        let start = Instant::now();
        for index in 0..1000 {
            editor
                .feed(Key::Char(if index % 2 == 0 { 'j' } else { 'k' }))
                .unwrap();
        }
        let movement = start.elapsed().as_micros();
        let start = Instant::now();
        for _ in 0..100 {
            editor.feed(Key::Char('u')).unwrap();
            editor.feed(Key::Ctrl('r')).unwrap();
        }
        let history = start.elapsed().as_micros();
        assert_eq!(
            editor.state().buffer().byte_len(),
            source.len() + 1000 - 9900
        );
        // A separate, fresh document measures real contiguous typing and its
        // single Insert transaction, independently of fragmented replacements.
        let mut typing = Editor::open(&path).unwrap();
        typing.set_cursor(source.len() / 2).unwrap();
        let start = Instant::now();
        typing.feed(Key::Char('i')).unwrap();
        for _ in 0..1000 {
            typing.feed(Key::Char(black_box('x'))).unwrap();
        }
        typing.feed(Key::Escape).unwrap();
        let typing_time = start.elapsed().as_micros();
        assert_eq!(typing.state().buffer().byte_len(), source.len() + 1000);
        typing.feed(Key::Char('u')).unwrap();
        assert_eq!(typing.state().buffer().contents(), source);
        println!(
            "{},{open},{insert},{delete},{movement},{history},{typing_time}",
            source.len()
        );
    }
}
