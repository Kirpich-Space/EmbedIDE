//! Public parser/state integration. No frontend-specific event loop is needed.
use crate::{
    executor,
    file::{FileBinding, SaveReport},
    vim::{Dispatch, IdeAction, Key, Keymap, Mode, VimInput},
    EditorError, EditorState,
};
use std::{ops::Range, path::Path};

/// Observable session result; IDE requests are handled by the owning frontend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Input was consumed and/or document state changed.
    Applied,
    /// The frontend may route this system shortcut elsewhere.
    Unhandled,
    /// File save committed; inspect the platform synchronization report.
    Saved(SaveReport),
    /// A quit request passed dirty-state checks.
    Closed,
    /// Focus/task request; the editor itself has not started a process.
    Ide(IdeAction),
}

/// One editable document and its modal input state. Mode is owned only by input.
#[derive(Debug)]
pub struct Editor {
    input: VimInput,
    state: EditorState,
}

impl Editor {
    /// Create an untitled UTF-8 document. Nonempty initial text is unsaved.
    pub fn new(text: String) -> Self {
        Self {
            input: VimInput::default(),
            state: EditorState::new(text, None, false),
        }
    }
    /// Open an existing UTF-8 file without changing its bytes.
    pub fn open(path: &Path) -> Result<Self, EditorError> {
        let (binding, text) = FileBinding::open(path)?;
        Ok(Self {
            input: VimInput::default(),
            state: EditorState::new(text, Some(binding), true),
        })
    }
    /// Bind a new empty file; disk creation happens on successful save only.
    pub fn create(path: &Path) -> Result<Self, EditorError> {
        let binding = FileBinding::create(path)?;
        Ok(Self {
            input: VimInput::default(),
            state: EditorState::new(String::new(), Some(binding), false),
        })
    }
    /// Read-only document, register, selection and cursor state.
    pub fn state(&self) -> &EditorState {
        &self.state
    }
    /// Current mode, command prompt and unfinished parser state.
    pub fn input(&self) -> &VimInput {
        &self.input
    }
    /// Change bindings without exposing mutable modal state.
    pub fn set_keymap(&mut self, keymap: Keymap) {
        self.input.set_keymap(keymap);
    }
    /// Move the caret from a mouse/API byte coordinate, validating UTF-8 first.
    pub fn set_cursor(&mut self, offset: usize) -> Result<(), EditorError> {
        if self.state.closed {
            return Err(EditorError::Closed);
        }
        let cursor = self.state.lines.clamp(
            offset,
            self.input.mode() == Mode::Insert,
            &self.state.buffer,
        )?;
        self.state.finish_group();
        self.input.cancel_prefix();
        self.state.cursor = cursor;
        self.state.preferred = None;
        if let Some(selection) = &mut self.state.selection {
            selection.head = cursor;
        }
        Ok(())
    }
    /// Save through the file binding, including a Save shortcut during Insert.
    /// The current undo group ends, while the parser's editing mode is retained.
    pub fn save(&mut self) -> Result<SaveReport, EditorError> {
        if self.state.closed {
            return Err(EditorError::Closed);
        }
        self.state.save()
    }
    /// Replace a validated range through the same history portal used by Vim.
    pub fn replace(&mut self, range: Range<usize>, text: &str) -> Result<(), EditorError> {
        if self.state.closed {
            return Err(EditorError::Closed);
        }
        let cursor = range
            .start
            .checked_add(text.len())
            .ok_or(EditorError::EditTooLarge)?;
        let insert = self.input.mode() == Mode::Insert;
        let _ = self.state.buffer.chunks_in(range.clone())?;
        if insert {
            self.state.begin_group();
        }
        if let Err(error) = self.state.replace(range, text, cursor, insert) {
            self.state.discard_empty_group();
            return Err(error);
        }
        if !insert {
            self.input.reset();
            self.state.selection = None;
        }
        Ok(())
    }
    /// Parse and execute a single logical key. No shell or GUI is involved.
    /// A failed action restores mode/prompt, cancelling its pending repeat/operator.
    pub fn feed(&mut self, key: Key) -> Result<Effect, EditorError> {
        if self.state.closed {
            return Err(EditorError::Closed);
        }
        let previous = self.input.clone();
        let was_insert = previous.mode() == Mode::Insert;
        match self.input.feed(key)? {
            Dispatch::Consumed => Ok(Effect::Applied),
            Dispatch::Unhandled => Ok(Effect::Unhandled),
            Dispatch::Action(action) => {
                match executor::execute(&mut self.state, action, was_insert) {
                    Ok(effect) => Ok(effect),
                    Err(error) => {
                        self.state.discard_empty_group();
                        self.input = previous;
                        self.input.cancel_prefix();
                        Err(error)
                    }
                }
            }
        }
    }
}
