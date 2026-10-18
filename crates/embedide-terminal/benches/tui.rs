//! Event -> engine -> layout -> row-diff workload, excluding actual terminal I/O.
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use embedide_editor::Editor;
use embedide_terminal::tui::{screen::Screen, TuiEditor};
use std::{
    hint::black_box,
    io::{self, Write},
    time::Instant,
};

#[derive(Default)]
struct Sink {
    bytes: usize,
}
impl Write for Sink {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.bytes += data.len();
        Ok(data.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn event(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}
fn draw(app: &mut TuiEditor, screen: &mut Screen, sink: &mut Sink) {
    screen.draw(&black_box(app.frame().unwrap()), sink).unwrap();
}
fn main() {
    println!("bytes,typing_p50_ns,typing_p95_ns,move_1000_us,undo_redo_100_us,output_bytes");
    for size in [1024 * 1024, 10 * 1024 * 1024] {
        let source = "0123456789abcdef\n".repeat(size / 17);
        let len = source.len();
        let mut editor = Editor::new(source);
        editor.set_cursor(len / 2).unwrap();
        let mut app = TuiEditor::new(editor, 80, 24);
        let mut screen = Screen::default();
        let mut sink = Sink::default();
        draw(&mut app, &mut screen, &mut sink);
        app.handle(event(KeyCode::Char('i')));
        let mut samples = Vec::with_capacity(1000);
        for _ in 0..1000 {
            let start = Instant::now();
            app.handle(event(KeyCode::Char(black_box('x'))));
            draw(&mut app, &mut screen, &mut sink);
            samples.push(start.elapsed().as_nanos());
        }
        app.handle(event(KeyCode::Esc));
        let start = Instant::now();
        for index in 0..1000 {
            app.handle(event(if index % 2 == 0 {
                KeyCode::Left
            } else {
                KeyCode::Right
            }));
            draw(&mut app, &mut screen, &mut sink);
        }
        let movement = start.elapsed().as_micros();
        let start = Instant::now();
        for _ in 0..100 {
            app.handle(event(KeyCode::Char('u')));
            draw(&mut app, &mut screen, &mut sink);
            app.handle(Event::Key(KeyEvent::new(
                KeyCode::Char('r'),
                KeyModifiers::CONTROL,
            )));
            draw(&mut app, &mut screen, &mut sink);
        }
        let history = start.elapsed().as_micros();
        samples.sort_unstable();
        assert_eq!(app.editor().state().buffer().byte_len(), len + 1000);
        println!(
            "{len},{},{},{movement},{history},{}",
            samples[500], samples[950], sink.bytes
        );
    }
}
