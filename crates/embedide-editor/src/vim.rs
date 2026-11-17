//! Modal input parser independent of windows, terminal escapes and document I/O.
//!
//! The host applies emitted actions to its document/session. `WriteQuit` is a
//! request: the host must save successfully before closing, and `Quit` must
//! check unsaved changes. Parsing a command never writes a file or runs a tool.
//! Feed committed IME/paste text as `Key::Text`, not as a sequence of commands.

use std::{collections::BTreeMap, fmt, sync::Arc};

/// Maximum repeat count, including multiplication of operator and motion counts.
pub const MAX_COUNT: u32 = 1_000_000;
/// Maximum UTF-8 byte length of a command or search prompt.
pub const MAX_COMMAND_BYTES: usize = 4096;

/// Logical input after platform key decoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    /// A layout-aware key character for modal commands or direct typing.
    Char(char),
    /// Committed text from IME or paste; never interpreted as Normal commands.
    Text(String),
    /// Escape or a frontend-normalized equivalent.
    Escape,
    /// Enter/Return.
    Enter,
    /// Tab.
    Tab,
    /// Shift+Tab.
    BackTab,
    /// Backspace.
    Backspace,
    /// Delete.
    Delete,
    /// Control chord; use a lowercase character, e.g. `Ctrl('r')`.
    Ctrl(char),
    /// Left arrow.
    Left,
    /// Down arrow.
    Down,
    /// Up arrow.
    Up,
    /// Right arrow.
    Right,
    /// Home.
    Home,
    /// End.
    End,
}

/// User-visible editor mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Motions and editing commands.
    Normal,
    /// Literal text input.
    Insert,
    /// Motions extend a selection.
    Visual,
    /// Ex command or search prompt; inspect `command_prompt` for its kind.
    Command,
}

/// Selection geometry, independent of a renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionKind {
    /// Unicode character selection.
    Character,
    /// Whole-line selection.
    Line,
}

/// Document motion. Columns and offsets are resolved by the document layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    /// Previous character on this line.
    Left,
    /// Next line, retaining the desired column.
    Down,
    /// Previous line, retaining the desired column.
    Up,
    /// Next character on this line.
    Right,
    /// Start of the next word.
    WordForward,
    /// Start of the previous word.
    WordBackward,
    /// End of the current or next word.
    WordEnd,
    /// First column of this line.
    LineStart,
    /// First non-whitespace character of this line.
    FirstNonBlank,
    /// End of this line.
    LineEnd,
    /// One-based line number, clamped by the document layer.
    Line(u32),
    /// Last line of the document.
    LastLine,
}

/// An operator awaiting a motion, text object or second operator key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    /// Delete text into the unnamed register.
    Delete,
    /// Copy text into the unnamed register.
    Yank,
    /// Delete text and enter Insert mode as one edit transaction.
    Change,
}

/// Location at which an Insert session starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertPosition {
    /// Before the cursor (`i`).
    Before,
    /// After the cursor (`a`).
    After,
    /// First non-whitespace character of the line (`I`).
    LineStart,
    /// End of the line (`A`).
    LineEnd,
    /// New line below the cursor (`o`).
    LineBelow,
    /// New line above the cursor (`O`).
    LineAbove,
}

/// IDE-level actions. The parser only emits requests, never starts processes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdeAction {
    /// Focus the project explorer.
    ProjectExplorer,
    /// Request a build.
    Build,
    /// Request a debug session.
    Debug,
    /// Focus the terminal panel.
    Terminal,
    /// Focus the serial monitor.
    SerialMonitor,
    /// Open file search.
    FindFile,
    /// Focus (or leave) the explorer, like `Alt+1`.
    FocusExplorer,
    /// Focus (or leave) the AI panel, like `Alt+2`.
    FocusAssistant,
    /// Hide the explorer, like holding `Alt+1` and pressing Delete.
    HideExplorer,
    /// Hide the AI panel, like holding `Alt+2` and pressing Delete.
    HideAssistant,
    /// Hide the terminal panel, like holding `Alt+3` and pressing Delete.
    HideTerminal,
}

/// Supported Ex commands; arguments and shell escapes are deliberately rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExCommand {
    /// Persist the active document.
    Write,
    /// Request close, checking unsaved changes.
    Quit,
    /// Explicitly request close while discarding unsaved edits.
    ForceQuit,
    /// Persist successfully, then close.
    WriteQuit,
    /// Any other command line, forwarded to the host IDE when the keymap
    /// allows host commands (e.g. `:build`, `:e src/main.c`).
    Host(String),
}

/// Typed operation to be applied by a document or IDE session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Move the cursor, or extend the active Visual selection.
    Move {
        /// Motion to resolve.
        motion: Motion,
        /// Number of repetitions.
        count: u32,
    },
    /// Begin a grouped Insert session; retain its repeat for Escape handling.
    BeginInsert {
        /// Placement relative to the cursor.
        position: InsertPosition,
        /// Number of times to repeat the inserted text on completion.
        repeat: u32,
    },
    /// Finish the Insert group and return the cursor to Normal positioning.
    FinishInsert,
    /// Insert committed literal text without key-command interpretation.
    InsertText(String),
    /// Insert a line break using the document's line-ending and indent policy.
    InsertNewline,
    /// Tab in Insert mode: indent the line one level when the cursor is in
    /// its leading whitespace, otherwise insert a tab or spaces to the next stop.
    InsertTab,
    /// Shift+Tab in Insert mode: remove one indentation level from the line.
    Dedent,
    /// A typed closing delimiter (`}`, `]`, `)`); on an otherwise blank line
    /// it re-aligns with its opener before being inserted.
    TypeCloser(char),
    /// Tab / Shift+Tab in Normal mode: shift `count` lines by one level.
    ShiftLines {
        /// Remove a level instead of adding one.
        dedent: bool,
        /// Number of lines.
        count: u32,
    },
    /// Tab / Shift+Tab in Visual mode: shift the selected lines by one level.
    ShiftSelection {
        /// Remove a level instead of adding one.
        dedent: bool,
    },
    /// Delete before the insertion point.
    Backspace,
    /// Delete characters after the cursor or insertion point.
    DeleteForward(u32),
    /// Apply an operator to complete lines (`dd`, `yy`, `cc`).
    Lines {
        /// Operation to perform.
        operator: Operator,
        /// Number of lines.
        count: u32,
    },
    /// Apply an operator to a motion range, for example `dw`.
    Operate {
        /// Operation to perform.
        operator: Operator,
        /// Range-ending motion.
        motion: Motion,
        /// Product of operator and motion repeats.
        count: u32,
    },
    /// Operate on an inner-word text object (`ciw`, `diw`, `yiw`).
    InnerWord {
        /// Operation to perform.
        operator: Operator,
        /// Number of words.
        count: u32,
    },
    /// Begin a selection or change its geometry while retaining its anchor.
    BeginVisual(SelectionKind),
    /// Clear the selection.
    EndVisual,
    /// Apply an operator to the active selection.
    VisualOperator(Operator),
    /// Paste the unnamed register.
    Paste {
        /// Insert before rather than after the cursor.
        before: bool,
        /// Number of copies.
        count: u32,
    },
    /// Undo this many transactions.
    Undo(u32),
    /// Redo this many transactions.
    Redo(u32),
    /// Search literally; an empty query requests the host's previous search.
    Search {
        /// Committed search text.
        query: String,
        /// Search towards the start of the file.
        backward: bool,
    },
    /// Repeat the host's last search.
    RepeatSearch {
        /// Reverse the last search direction (`N`).
        opposite: bool,
        /// Number of matches to advance.
        count: u32,
    },
    /// Request a file/session command.
    Ex(ExCommand),
    /// Request an IDE action from a configured leader chord.
    Ide(IdeAction),
}

/// Result of input routing. Only `Unhandled` may reach frontend shortcuts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dispatch {
    /// A completed typed action.
    Action(Action),
    /// Consumed prefix, prompt update or unsupported modal character.
    Consumed,
    /// Unclaimed system/control key; no text should be inserted implicitly.
    Unhandled,
}

/// Recoverable input error; no document operation has been performed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputError {
    /// A repeat or its product exceeds `MAX_COUNT`.
    CountTooLarge,
    /// The command/search prompt exceeds `MAX_COMMAND_BYTES`.
    CommandTooLong,
    /// Unsupported Ex text, retained for an actionable frontend diagnostic.
    UnknownCommand(String),
}

impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CountTooLarge => write!(f, "repeat count exceeds {MAX_COUNT}"),
            Self::CommandTooLong => write!(f, "command exceeds {MAX_COMMAND_BYTES} UTF-8 bytes"),
            Self::UnknownCommand(command) => write!(f, "unsupported command: :{command}"),
        }
    }
}

impl std::error::Error for InputError {}

/// Configurable Normal/Visual character aliases and Normal-only leader chords.
/// Aliases apply once, including operator prefixes, never to Insert or prompts.
/// This is a small keymap, not Vim's recursive mapping language.
#[derive(Debug, Clone)]
pub struct Keymap {
    leader: char,
    leader_bindings: BTreeMap<char, IdeAction>,
    aliases: BTreeMap<char, char>,
    host_commands: bool,
}

impl Default for Keymap {
    fn default() -> Self {
        Self {
            leader: ' ',
            leader_bindings: [
                ('e', IdeAction::ProjectExplorer),
                ('b', IdeAction::Build),
                ('d', IdeAction::Debug),
                ('t', IdeAction::Terminal),
                ('s', IdeAction::SerialMonitor),
                ('f', IdeAction::FindFile),
                // Panels without Alt: the FreeBSD console, for one, sends Alt+1 as a plain "1".
                ('1', IdeAction::FocusExplorer),
                ('2', IdeAction::FocusAssistant),
                ('3', IdeAction::Terminal),
                ('a', IdeAction::FocusAssistant),
                ('E', IdeAction::HideExplorer),
                ('A', IdeAction::HideAssistant),
                ('T', IdeAction::HideTerminal),
            ]
            .into_iter()
            .collect(),
            aliases: BTreeMap::new(),
            host_commands: false,
        }
    }
}

impl Keymap {
    /// Forward unknown `:` commands to the host instead of rejecting them.
    /// A standalone editor keeps this off; an IDE workspace turns it on.
    pub fn set_host_commands(&mut self, enabled: bool) {
        self.host_commands = enabled;
    }

    /// Set the Normal-only leader character.
    pub fn set_leader(&mut self, leader: char) {
        self.leader = leader;
    }

    /// Add or replace an IDE chord following the leader.
    pub fn bind_leader(&mut self, key: char, action: IdeAction) {
        self.leader_bindings.insert(key, action);
    }

    /// Remove a leader chord; return its previous action if present.
    pub fn unbind_leader(&mut self, key: char) -> Option<IdeAction> {
        self.leader_bindings.remove(&key)
    }

    /// Map a Normal/Visual character to another logical character, once.
    /// For example, mapping `H` to `h` adds an alternative Left binding.
    pub fn remap(&mut self, from: char, to: char) {
        self.aliases.insert(from, to);
    }
}

#[derive(Debug, Clone, Copy)]
enum Pending {
    None,
    Goto,
    Leader,
    Operator(Operator, u32),
    InnerWord(Operator, u32),
}

/// Stateful modal parser, with bounded repeats and Unicode-aware prompt editing.
/// A frontend should retain one instance per editor session.
#[derive(Debug, Clone)]
pub struct VimInput {
    mode: Mode,
    visual_kind: SelectionKind,
    keymap: Arc<Keymap>,
    pending: Pending,
    count: Option<u32>,
    prompt: char,
    command: String,
    return_mode: Mode,
}

impl Default for VimInput {
    fn default() -> Self {
        Self::with_keymap(Keymap::default())
    }
}

impl VimInput {
    /// Start in Normal mode with the supplied keymap.
    pub fn with_keymap(keymap: Keymap) -> Self {
        Self {
            mode: Mode::Normal,
            visual_kind: SelectionKind::Character,
            keymap: Arc::new(keymap),
            pending: Pending::None,
            count: None,
            prompt: ':',
            command: String::new(),
            return_mode: Mode::Normal,
        }
    }

    /// Current mode for focus handling and the status bar.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Active command/search prompt character, if a prompt is open.
    pub fn command_prompt(&self) -> Option<char> {
        (self.mode == Mode::Command).then_some(self.prompt)
    }

    /// Current prompt text without the leading `:`, `/` or `?`.
    pub fn command_text(&self) -> Option<&str> {
        (self.mode == Mode::Command).then_some(self.command.as_str())
    }

    /// Replace configuration and cancel incomplete prefixes without changing mode.
    pub fn set_keymap(&mut self, keymap: Keymap) {
        self.keymap = Arc::new(keymap);
        self.clear_prefix();
    }

    /// Return to Normal after a failed host operation or document replacement.
    /// The host must separately finish any open document edit group.
    pub fn reset(&mut self) {
        self.mode = Mode::Normal;
        self.command.clear();
        self.clear_prefix();
    }

    fn clear_prefix(&mut self) {
        self.pending = Pending::None;
        self.count = None;
    }

    pub(crate) fn cancel_prefix(&mut self) {
        self.clear_prefix();
    }

    fn take_count(&mut self) -> u32 {
        self.count.take().unwrap_or(1)
    }

    fn motion(key: &Key) -> Option<Motion> {
        match key {
            Key::Char('h') | Key::Left | Key::Backspace => Some(Motion::Left),
            Key::Char('j') | Key::Down => Some(Motion::Down),
            Key::Char('k') | Key::Up => Some(Motion::Up),
            Key::Char('l') | Key::Right => Some(Motion::Right),
            Key::Char('w') => Some(Motion::WordForward),
            Key::Char('b') => Some(Motion::WordBackward),
            Key::Char('e') => Some(Motion::WordEnd),
            Key::Char('0') | Key::Home => Some(Motion::LineStart),
            Key::Char('^') => Some(Motion::FirstNonBlank),
            Key::Char('$') | Key::End => Some(Motion::LineEnd),
            _ => None,
        }
    }

    fn emit(action: Action) -> Result<Dispatch, InputError> {
        Ok(Dispatch::Action(action))
    }

    /// Feed one decoded key. Errors consume the key and never request text edits.
    pub fn feed(&mut self, mut key: Key) -> Result<Dispatch, InputError> {
        match self.mode {
            Mode::Insert => return self.insert(key),
            Mode::Command => return self.command(key),
            Mode::Normal | Mode::Visual => {}
        }
        if let Key::Char(character) = &mut key {
            // Leader suffixes are literal configuration keys, not motion aliases.
            if !matches!(self.pending, Pending::Leader) {
                *character = self
                    .keymap
                    .aliases
                    .get(character)
                    .copied()
                    .unwrap_or(*character);
            }
        }
        if key == Key::Escape {
            self.clear_prefix();
            if self.mode == Mode::Visual {
                self.mode = Mode::Normal;
                return Self::emit(Action::EndVisual);
            }
            return Ok(Dispatch::Consumed);
        }
        if let Key::Ctrl(character) = key {
            let count = self.take_count();
            let idle = matches!(self.pending, Pending::None);
            self.clear_prefix();
            if character == 'r' && idle && self.mode == Mode::Normal {
                return Self::emit(Action::Redo(count));
            }
            return Ok(Dispatch::Unhandled);
        }
        if matches!(self.pending, Pending::None | Pending::Operator(..)) {
            if let Key::Char(character @ '0'..='9') = key {
                if character != '0' || self.count.is_some() {
                    let digit = u32::from(character) - u32::from('0');
                    let count = self.count.unwrap_or(0) * 10 + digit;
                    if count > MAX_COUNT {
                        self.clear_prefix();
                        return Err(InputError::CountTooLarge);
                    }
                    self.count = Some(count);
                    return Ok(Dispatch::Consumed);
                }
            }
        }
        match std::mem::replace(&mut self.pending, Pending::None) {
            Pending::Leader => {
                self.count = None;
                if let Key::Char(character) = key {
                    if let Some(action) = self.keymap.leader_bindings.get(&character) {
                        return Self::emit(Action::Ide(*action));
                    }
                }
                Ok(Dispatch::Consumed)
            }
            Pending::Goto => {
                let line = self.take_count();
                if key == Key::Char('g') {
                    return Self::emit(Action::Move {
                        motion: Motion::Line(line),
                        count: 1,
                    });
                }
                Ok(Dispatch::Consumed)
            }
            Pending::InnerWord(operator, count) => {
                self.count = None;
                if key == Key::Char('w') {
                    self.after_operator(operator);
                    return Self::emit(Action::InnerWord { operator, count });
                }
                Ok(Dispatch::Consumed)
            }
            Pending::Operator(operator, prefix) => self.operator(key, operator, prefix),
            Pending::None => self.normal(key),
        }
    }

    fn after_operator(&mut self, operator: Operator) {
        self.mode = if operator == Operator::Change {
            Mode::Insert
        } else {
            Mode::Normal
        };
    }

    fn operator(
        &mut self,
        key: Key,
        operator: Operator,
        prefix: u32,
    ) -> Result<Dispatch, InputError> {
        let count = prefix
            .checked_mul(self.take_count())
            .filter(|count| *count <= MAX_COUNT)
            .ok_or(InputError::CountTooLarge)?;
        let repeated = matches!(
            (operator, &key),
            (Operator::Delete, Key::Char('d'))
                | (Operator::Yank, Key::Char('y'))
                | (Operator::Change, Key::Char('c'))
        );
        if repeated {
            self.after_operator(operator);
            return Self::emit(Action::Lines { operator, count });
        }
        if key == Key::Char('i') {
            self.pending = Pending::InnerWord(operator, count);
            return Ok(Dispatch::Consumed);
        }
        if let Some(motion) = Self::motion(&key) {
            self.after_operator(operator);
            return Self::emit(Action::Operate {
                operator,
                motion,
                count,
            });
        }
        Ok(Dispatch::Consumed)
    }

    fn normal(&mut self, key: Key) -> Result<Dispatch, InputError> {
        if self.mode == Mode::Normal && key == Key::Char(self.keymap.leader) {
            self.count = None;
            self.pending = Pending::Leader;
            return Ok(Dispatch::Consumed);
        }
        if key == Key::Char('g') {
            self.pending = Pending::Goto;
            return Ok(Dispatch::Consumed);
        }
        if key == Key::Char('G') {
            let motion = self
                .count
                .take()
                .map(Motion::Line)
                .unwrap_or(Motion::LastLine);
            return Self::emit(Action::Move { motion, count: 1 });
        }
        let count = self.take_count();
        if let Some(motion) = Self::motion(&key) {
            return Self::emit(Action::Move { motion, count });
        }
        if matches!(key, Key::Tab | Key::BackTab) {
            let dedent = key == Key::BackTab;
            if self.mode == Mode::Visual {
                self.mode = Mode::Normal;
                return Self::emit(Action::ShiftSelection { dedent });
            }
            return Self::emit(Action::ShiftLines { dedent, count });
        }
        let character = match key {
            Key::Char(character) => character,
            Key::Delete => 'x',
            Key::Text(_) => return Ok(Dispatch::Consumed),
            _ => return Ok(Dispatch::Unhandled),
        };
        if matches!(character, 'v' | 'V') {
            let kind = if character == 'v' {
                SelectionKind::Character
            } else {
                SelectionKind::Line
            };
            if self.mode == Mode::Visual && self.visual_kind == kind {
                self.mode = Mode::Normal;
                return Self::emit(Action::EndVisual);
            }
            self.mode = Mode::Visual;
            self.visual_kind = kind;
            return Self::emit(Action::BeginVisual(kind));
        }
        if character == '/' || character == '?' || (character == ':' && self.mode == Mode::Normal) {
            self.return_mode = self.mode;
            self.mode = Mode::Command;
            self.prompt = character;
            self.command.clear();
            return Ok(Dispatch::Consumed);
        }
        if character == 'n' || character == 'N' {
            return Self::emit(Action::RepeatSearch {
                opposite: character == 'N',
                count,
            });
        }
        let operator = match character {
            'd' | 'x' if self.mode == Mode::Visual => Some(Operator::Delete),
            'd' => Some(Operator::Delete),
            'y' => Some(Operator::Yank),
            'c' => Some(Operator::Change),
            _ => None,
        };
        if let Some(operator) = operator {
            if self.mode == Mode::Visual {
                self.after_operator(operator);
                return Self::emit(Action::VisualOperator(operator));
            }
            self.pending = Pending::Operator(operator, count);
            return Ok(Dispatch::Consumed);
        }
        if self.mode == Mode::Visual {
            return Ok(Dispatch::Consumed);
        }
        let position = match character {
            'i' => Some(InsertPosition::Before),
            'a' => Some(InsertPosition::After),
            'I' => Some(InsertPosition::LineStart),
            'A' => Some(InsertPosition::LineEnd),
            'o' => Some(InsertPosition::LineBelow),
            'O' => Some(InsertPosition::LineAbove),
            _ => None,
        };
        if let Some(position) = position {
            self.mode = Mode::Insert;
            return Self::emit(Action::BeginInsert {
                position,
                repeat: count,
            });
        }
        match character {
            'u' => Self::emit(Action::Undo(count)),
            'p' | 'P' => Self::emit(Action::Paste {
                before: character == 'P',
                count,
            }),
            'x' => Self::emit(Action::DeleteForward(count)),
            _ => Ok(Dispatch::Consumed),
        }
    }

    fn insert(&mut self, key: Key) -> Result<Dispatch, InputError> {
        match key {
            Key::Escape => {
                self.mode = Mode::Normal;
                self.clear_prefix();
                Self::emit(Action::FinishInsert)
            }
            Key::Text(text) => Self::emit(Action::InsertText(text)),
            Key::Char(closer @ ('}' | ']' | ')')) => Self::emit(Action::TypeCloser(closer)),
            Key::Char(character) if !character.is_control() => {
                Self::emit(Action::InsertText(character.to_string()))
            }
            Key::Enter => Self::emit(Action::InsertNewline),
            Key::Tab => Self::emit(Action::InsertTab),
            Key::BackTab => Self::emit(Action::Dedent),
            Key::Backspace => Self::emit(Action::Backspace),
            Key::Delete => Self::emit(Action::DeleteForward(1)),
            Key::Left | Key::Down | Key::Up | Key::Right | Key::Home | Key::End => {
                if let Some(motion) = Self::motion(&key) {
                    return Self::emit(Action::Move { motion, count: 1 });
                }
                Ok(Dispatch::Unhandled)
            }
            _ => Ok(Dispatch::Unhandled),
        }
    }

    fn append_command(&mut self, text: &str) -> Result<Dispatch, InputError> {
        if text.len() > MAX_COMMAND_BYTES - self.command.len() {
            return Err(InputError::CommandTooLong);
        }
        self.command.push_str(text);
        Ok(Dispatch::Consumed)
    }

    fn command(&mut self, key: Key) -> Result<Dispatch, InputError> {
        match key {
            Key::Escape => {
                self.mode = self.return_mode;
                self.command.clear();
                Ok(Dispatch::Consumed)
            }
            Key::Backspace => {
                self.command.pop();
                Ok(Dispatch::Consumed)
            }
            Key::Text(text) => self.append_command(&text),
            Key::Char(character) if !character.is_control() => {
                self.append_command(&character.to_string())
            }
            Key::Enter => {
                self.mode = self.return_mode;
                let text = std::mem::take(&mut self.command);
                if self.prompt != ':' {
                    return Self::emit(Action::Search {
                        query: text,
                        backward: self.prompt == '?',
                    });
                }
                let command = match text.trim() {
                    "w" => ExCommand::Write,
                    "q" => ExCommand::Quit,
                    "q!" => ExCommand::ForceQuit,
                    "wq" => ExCommand::WriteQuit,
                    other if self.keymap.host_commands && !other.is_empty() => {
                        ExCommand::Host(other.to_string())
                    }
                    _ => return Err(InputError::UnknownCommand(text)),
                };
                Self::emit(Action::Ex(command))
            }
            _ => Ok(Dispatch::Unhandled),
        }
    }
}
