//! Both frontends can reach the same editor through the headless core facade.
use embedide_core::{editor::vim::Key, Editor};

#[test]
fn shared_core_executes_a_modal_edit_and_undo() {
    let mut editor = Editor::new("old value".into());
    for key in "ciw".chars() {
        editor.feed(Key::Char(key)).unwrap();
    }
    editor.feed(Key::Text("new".into())).unwrap();
    editor.feed(Key::Escape).unwrap();
    assert_eq!(editor.state().buffer().contents(), "new value");
    editor.feed(Key::Char('u')).unwrap();
    assert_eq!(editor.state().buffer().contents(), "old value");
}
