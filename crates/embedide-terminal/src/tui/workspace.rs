//! Project workspace adapter. Services and document mutations remain outside widgets.
use super::binview;
use super::popup::{self, Popup};
use super::{
    assistant::{Assistant, EditorContext},
    guard,
    layout::{self, Frame, Row},
    screen::{Screen, Theme},
    term::{self, TerminalPane},
    TuiEditor,
};
use crate::FrontendError;
use crossterm::{
    event::{
        self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
        MouseEventKind,
    },
    terminal,
};
use embedide_core::lsp;
use embedide_core::{
    self as core,
    config::NativeConfig,
    debug::{DebugCommand, DebugEvent, DebugSession},
    diagnostics::Diagnostic,
    explorer::{self, Entry},
    hardware::{self, SerialEvent, SerialMonitor},
    process::{ProcessEvent, ProcessTask},
    BuildPlan, BuildSystem, ProcessSpec,
};
use embedide_editor::completion::{self, Candidate};
use embedide_editor::indent::IndentStyle;
use embedide_editor::{
    vim::{IdeAction, Keymap, Mode, SelectionKind},
    Editor,
};
use embedide_workspace::{Manifest, UserConfig};
use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    io::{self, IsTerminal},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};
use unicode_width::UnicodeWidthStr;

const OUTPUT_LINES: usize = 2000;
const COMMANDS: &[(&str, &str)] = &[
    ("focus_explorer", "Explorer Panel (Alt+1)"),
    ("focus_assistant", "AI Assistant Panel (Alt+2)"),
    ("focus_terminal", "Terminal Panel (Alt+3)"),
    ("hide_explorer", "Hide Explorer Panel (Alt+1+Delete)"),
    ("hide_assistant", "Hide AI Assistant Panel (Alt+2+Delete)"),
    (
        "hide_terminal",
        "Hide Terminal Panel, shell keeps running (Alt+3+Delete)",
    ),
    ("assistant", "Show / Hide AI Panel"),
    ("terminal_restart", "Restart Terminal Shell"),
    (
        "complete",
        "AI: Suggest Completion at Cursor (Tab accepts, Esc rejects)",
    ),
    ("explain", "AI: Explain Current File or Selection"),
    (
        "suggest",
        "Suggest Completions at the Cursor (Ctrl+Space, Insert mode)",
    ),
    (
        "ai_suggest",
        "AI Inline Suggestion at the Cursor (Alt+\\, Insert mode; Tab accepts)",
    ),
    ("lsp_restart", "Restart Language Servers (completion)"),
    ("labels", "Go to Label (assembly: labels of this file)"),
    ("hex", "Open in HEX View (current file; :hex <path>)"),
    (
        "elf",
        "Inspect ELF Headers, Sections, Symbols (:elf <path>)",
    ),
    (
        "disasm",
        "Disassemble Binary (objdump / llvm-objdump; :disasm <path>)",
    ),
    ("close_binary", "Close Binary View"),
    ("build", "Build Project"),
    ("check", "Check Project (Cargo)"),
    ("clean", "Clean Project"),
    ("cancel", "Cancel Current Task"),
    ("find", "Filter Loaded Explorer Entries by Name"),
    ("open", "Open File (relative or absolute path)"),
    ("search", "Search Project (case sensitive)"),
    ("search_ignore_case", "Search Project (ignore case)"),
    ("problems", "Show Problems / Search Results"),
    ("explorer", "Toggle Explorer"),
    ("toggle_panel", "Toggle Output Panel"),
    ("reveal", "Reveal Current File"),
    ("refresh", "Refresh Explorer"),
    ("new_file", "Create File"),
    ("new_directory", "Create Directory"),
    ("rename", "Rename Selected File / Directory"),
    ("delete", "Delete Selected File / Empty Directory"),
    ("target", "Select Target"),
    ("profile", "Select Build Profile"),
    ("tools", "Discover Toolchains (PATH inventory)"),
    ("probes", "Discover Probes (probe-rs list)"),
    ("flash", "Flash Firmware (preview and confirmation)"),
    ("ports", "Discover Serial Ports"),
    ("serial_port", "Select Serial Port / Baud"),
    ("serial", "Connect Serial Monitor"),
    ("serial_disconnect", "Disconnect Serial Monitor"),
    ("serial_send", "Send Serial Text"),
    ("serial_pause", "Pause / Resume Serial Display"),
    ("serial_timestamps", "Toggle Serial Timestamps"),
    ("serial_clear", "Clear Serial View"),
    ("serial_export", "Export Serial Output (new file)"),
    ("debug", "Start Debugging (GDB/MI, optional OpenOCD)"),
    ("debug_stop", "Stop Debugging"),
    ("continue", "Debug Continue"),
    ("interrupt", "Debug Interrupt"),
    ("step", "Debug Step"),
    ("next", "Debug Next"),
    ("breakpoint", "Debug Breakpoint at Current Line"),
    ("break_delete", "Debug Remove Breakpoint (ID)"),
    ("backtrace", "Debug Backtrace"),
    ("registers", "Debug Registers"),
    ("settings", "Show Settings"),
    ("compilers", "Show Compilers Selected for This Project"),
    ("edit_project_config", "Edit Project Config (embedide.toml)"),
    ("edit_user_config", "Edit User Config (config.toml)"),
    ("reload", "Reload Settings"),
    ("theme", "Cycle Theme"),
    (
        "mouse",
        "Toggle Mouse (click focus/cursor, wheel scroll; Shift+drag selects text)",
    ),
    ("help", "Keyboard Help"),
    ("close", "Close Document"),
    ("quit", "Quit Workspace"),
];
/// A language server process, or why it is not available this session.
enum LspSlot {
    Running(lsp::Client),
    Failed,
}

/// Language-server language id of a document (`rust`, `c`, `cpp`, `zig`).
fn lsp_language(path: Option<&Path>) -> Option<&'static str> {
    match language_of(path) {
        "rust" => Some("rust"),
        "c" => Some("c"),
        "cpp" => Some("cpp"),
        "zig" => Some("zig"),
        _ => None,
    }
}

/// Screen rectangle of a panel in the last frame, for mouse hit tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Area {
    x: u16,
    y: u16,
    width: u16,
    height: u16,
}
impl Area {
    fn new(x: usize, y: usize, width: usize, height: usize) -> Option<Self> {
        let cell = |v: usize| u16::try_from(v).ok();
        (width > 0 && height > 0).then_some(Self {
            x: cell(x)?,
            y: cell(y)?,
            width: cell(width)?,
            height: cell(height)?,
        })
    }
    fn contains(self, column: u16, row: u16) -> bool {
        (self.x..self.x.saturating_add(self.width)).contains(&column)
            && (self.y..self.y.saturating_add(self.height)).contains(&row)
    }
}
/// Where the explorer, editor text, AI panel and bottom panel were drawn.
#[derive(Debug, Clone, Copy, Default)]
struct Hits {
    tree: Option<Area>,
    editor: Option<Area>,
    assistant: Option<Area>,
    bottom: Option<Area>,
}

/// Current keyboard destination. Insert-mode text is never interpreted as a command name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    /// Shared Vim engine.
    Editor,
    /// Lazy directory tree.
    Explorer,
    /// Output/problem navigation.
    Panel,
    /// AI assistant input line.
    Assistant,
    /// Interactive shell; every key except the panel keys goes to it.
    Terminal,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Panel {
    Problems,
    Build,
    Tasks,
    Serial,
    Debug,
    Tools,
    Settings,
    Help,
    Terminal,
}
impl Panel {
    fn name(self) -> &'static str {
        match self {
            Self::Problems => "PROBLEMS",
            Self::Build => "BUILD OUTPUT",
            Self::Tasks => "TASK CONSOLE",
            Self::Serial => "SERIAL",
            Self::Debug => "DEBUG",
            Self::Tools => "TOOLS",
            Self::Settings => "SETTINGS",
            Self::Help => "HELP",
            Self::Terminal => "TERMINAL",
        }
    }
    fn next(self) -> Self {
        match self {
            Self::Problems => Self::Build,
            Self::Build => Self::Tasks,
            Self::Tasks => Self::Serial,
            Self::Serial => Self::Debug,
            Self::Debug => Self::Tools,
            Self::Tools => Self::Settings,
            Self::Settings => Self::Help,
            Self::Help | Self::Terminal => Self::Problems,
        }
    }
}
struct TreeRow {
    entry: Entry,
    depth: usize,
    expanded: bool,
}
#[derive(Default)]
struct Log {
    lines: VecDeque<String>,
    bytes: usize,
}
impl Log {
    fn push(&mut self, text: &str) {
        if self.lines.back().is_some_and(|line| !line.is_empty()) {
            self.chunk("\n");
        }
        self.chunk(text);
        if !text.ends_with('\n') {
            self.chunk("\n");
        }
    }
    fn clear(&mut self) {
        self.lines.clear();
        self.bytes = 0;
    }
    fn chunk(&mut self, text: &str) {
        for (i, piece) in text.split('\n').enumerate() {
            if i > 0 || self.lines.is_empty() {
                self.lines.push_back(String::new());
            }
            let last = self.lines.back_mut().expect("log line");
            for ch in piece.chars() {
                if last.len() + ch.len_utf8() > 8192 {
                    break;
                }
                last.push(ch);
                self.bytes += ch.len_utf8();
            }
            while self.lines.len() > OUTPUT_LINES || self.bytes > 2 * 1024 * 1024 {
                if let Some(line) = self.lines.pop_front() {
                    self.bytes -= line.len();
                }
            }
        }
    }
}
enum InputKind {
    Open,
    NewFile,
    NewDirectory,
    Rename(PathBuf),
    Search(bool),
    SerialSend,
    SerialPort,
    SerialExport,
    BreakDelete,
    OutputFilter,
}
enum Confirm {
    Delete(PathBuf),
    Close,
    SaveBinary,
    CloseBinary,
    Quit,
    Flash(BuildPlan),
    Debug,
}
enum Dialog {
    Palette {
        query: String,
        selected: usize,
    },
    Input {
        label: String,
        text: String,
        kind: InputKind,
    },
    Confirm {
        text: String,
        action: Confirm,
    },
}
#[derive(Clone, Copy, PartialEq)]
enum Operation {
    Build,
    Task,
    Search,
    Flash,
    Probes,
}
struct Running {
    task: ProcessTask,
    kind: Operation,
    partial: [String; 2],
}

/// Full project session: lazy tree, independent document histories and optional services.
pub struct Workspace {
    root: PathBuf,
    config: NativeConfig,
    documents: Vec<TuiEditor>,
    active: usize,
    tree: Vec<TreeRow>,
    selected: usize,
    tree_top: usize,
    explorer_visible: bool,
    focus: Focus,
    panel: Panel,
    panel_visible: bool,
    panel_scroll: usize,
    problem: usize,
    size: (u16, u16),
    dialog: Option<Dialog>,
    notification: String,
    closed: bool,
    build_status: String,
    profile: String,
    target: Option<usize>,
    operation: Option<Running>,
    problems: Vec<Diagnostic>,
    build_log: Log,
    task_log: Log,
    serial_log: Log,
    debug_log: Log,
    tools_log: Log,
    output_filter: String,
    serial: Option<SerialMonitor>,
    debug: Option<DebugSession>,
    serial_paused: bool,
    serial_snapshot: Vec<String>,
    serial_timestamps: bool,
    serial_started: Instant,
    inventory: Option<Receiver<Vec<String>>>,
    debug_status: String,
    debug_location: Option<(PathBuf, usize)>,
    debug_breakpoints: String,
    debug_stack: String,
    debug_registers: String,
    assistant: Assistant,
    assistant_visible: bool,
    terminal: Option<TerminalPane>,
    keyboard_enhanced: bool,
    held_panel: Option<String>,
    /// Mouse capture requested (config `mouse` or `:mouse`).
    pub mouse: bool,
    /// Open completion popup.
    popup: Option<Popup>,
    /// Binary file shown in the editor area (HEX / ELF / disassembly).
    binary: Option<binview::BinaryView>,
    /// Language servers by command line (clangd serves C and C++).
    lsp: BTreeMap<String, LspSlot>,
    hits: Hits,
    user: UserConfig,
    manifest: Option<Manifest>,
    config_notes: Vec<String>,
    compilers: Vec<embedide_build::Selection>,
    prepared: Option<embedide_build::Prepared>,
    edit_mark: (usize, u64, usize),
}
fn service_error(error: impl std::fmt::Display) -> FrontendError {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string()).into()
}
impl Workspace {
    /// Open a directory without running project scripts, tool discovery or hardware scans.
    pub fn open(root: &Path, width: u16, height: u16) -> Result<Self, FrontendError> {
        let root = root.canonicalize()?;
        if !root.is_dir() {
            return Err(service_error("Workspace must be a directory"));
        }
        let config = NativeConfig::load(&root).map_err(service_error)?;
        let mouse = config.mouse;
        let tree = explorer::read_directory(&root, &root)
            .map_err(service_error)?
            .into_iter()
            .map(|entry| TreeRow {
                entry,
                depth: 0,
                expanded: false,
            })
            .collect();
        let profile = if config.profiles.contains_key("debug") {
            "debug".into()
        } else {
            config
                .profiles
                .keys()
                .next()
                .expect("validated profile")
                .clone()
        };
        let target = (!config.targets.is_empty()).then_some(0);
        let (user, manifest, mut config_notes) = load_configs(&root);
        config_notes.extend(unknown_key_commands(&config));
        let profile = match &manifest {
            Some(m) if m.profiles.contains_key("debug") || m.profiles.is_empty() => "debug".into(),
            Some(m) => m
                .profiles
                .keys()
                .next()
                .cloned()
                .unwrap_or_else(|| "debug".into()),
            None => profile,
        };
        let assistant = Assistant::new(user.ai.clone());
        let mut workspace = Self {
            root,
            config,
            documents: vec![TuiEditor::new(Editor::new(String::new()), width, height)],
            active: 0,
            tree,
            selected: 0,
            tree_top: 0,
            explorer_visible: true,
            focus: Focus::Explorer,
            panel: Panel::Build,
            panel_visible: true,
            panel_scroll: 0,
            problem: 0,
            size: (width, height),
            dialog: None,
            notification: "F1 help | Ctrl+K commands | F5 build | F6 focus | Ctrl+Q quit".into(),
            closed: false,
            build_status: "Ready".into(),
            profile,
            target,
            operation: None,
            problems: Vec::new(),
            build_log: Log::default(),
            task_log: Log::default(),
            serial_log: Log::default(),
            debug_log: Log::default(),
            tools_log: Log::default(),
            output_filter: String::new(),
            serial: None,
            debug: None,
            serial_paused: false,
            serial_snapshot: Vec::new(),
            serial_timestamps: false,
            serial_started: Instant::now(),
            inventory: None,
            debug_status: "Disconnected".into(),
            debug_location: None,
            debug_breakpoints: String::new(),
            debug_stack: String::new(),
            debug_registers: String::new(),
            assistant,
            assistant_visible: true,
            terminal: None,
            keyboard_enhanced: false,
            held_panel: None,
            mouse,
            popup: None,
            binary: None,
            lsp: BTreeMap::new(),
            hits: Hits::default(),
            user,
            manifest,
            config_notes,
            compilers: Vec::new(),
            prepared: None,
            edit_mark: (0, 0, 0),
        };
        workspace.documents[0] = workspace.new_document(Editor::new(String::new()));
        workspace.select_compilers();
        if let Some(note) = workspace.config_notes.first() {
            workspace.notification = format!("Error: {note} (see :settings)");
        }
        Ok(workspace)
    }
    fn new_document(&self, mut editor: Editor) -> TuiEditor {
        let mut keymap = Keymap::default();
        keymap.set_host_commands(true);
        editor.set_keymap(keymap);
        editor.set_indent(indent_style(&self.config, editor.state()));
        let mut doc = TuiEditor::new(editor, self.size.0, self.size.1);
        doc.viewport.set_tab_width(self.config.tab_width);
        doc.viewport
            .set_dialect(embedide_editor::asm::Dialect::parse(
                &self.config.asm.syntax,
            ));
        doc
    }
    /// Toolchains `embedide.toml` and the user configuration select for this
    /// project (no version probes; nothing is installed).
    fn select_compilers(&mut self) {
        self.compilers.clear();
        let Some(m) = &self.manifest else { return };
        let tools = embedide_build::registry(&self.user, false);
        let req = embedide_build::Request {
            profile: self.profile.clone(),
            ..Default::default()
        };
        match embedide_build::prepare(&self.root, Some(m), &self.user, &tools, &req) {
            Ok(p) => self.compilers = p.toolchains,
            Err(e) => self
                .config_notes
                .push(format!("Toolchain selection: {}", e.0)),
        }
    }
    /// Compiler chosen for the active document's language, for the status bar.
    fn compiler_label(&self, language: &str) -> String {
        if self.manifest.is_none() {
            return "no embedide.toml".into();
        }
        self.compilers
            .iter()
            .find(|s| s.language.id() == language)
            .map(|s| s.id.clone())
            .unwrap_or_else(|| "-".into())
    }
    fn editor_context(&self) -> EditorContext {
        let state = self.editor().state();
        let text = state.buffer().contents();
        let offset = state.cursor();
        let (line, column) = state
            .position()
            .map_or((1, 1), |p| (p.line + 1, p.scalar_column + 1));
        let selection = state.selection().and_then(|sel| {
            let (lo, hi) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
            let range = match sel.kind {
                SelectionKind::Character => {
                    lo..state
                        .buffer()
                        .next_grapheme(hi)
                        .unwrap_or(hi)
                        .min(text.len())
                }
                SelectionKind::Line => {
                    let start = text[..lo].rfind('\n').map_or(0, |i| i + 1);
                    let end = text[hi.min(text.len())..]
                        .find('\n')
                        .map_or(text.len(), |i| hi + i + 1);
                    start..end
                }
            };
            text.get(range).map(str::to_string)
        });
        EditorContext {
            root: self.root.clone(),
            path: state.path().map(Path::to_path_buf),
            line,
            column,
            offset,
            selection,
            language: language_of(state.path()).to_string(),
            exclude: self
                .manifest
                .as_ref()
                .map(|m| m.ai.exclude.clone())
                .unwrap_or_default(),
            indent_tabs: state.indent().tabs,
            indent_width: state.indent().width,
            project: self
                .manifest
                .as_ref()
                .map(|m| format!("{} ({})", m.project.name, m.project.language)),
            text,
        }
    }
    /// Start the shell on first use (or after it exited).
    fn ensure_terminal(&mut self) -> Result<(), FrontendError> {
        if self.terminal.as_ref().is_some_and(|t| t.exited().is_none()) {
            return Ok(());
        }
        let mut env: BTreeMap<String, String> = self.config.environment.clone();
        if let Some(m) = &self.manifest {
            env.extend(m.env.clone());
        }
        let mut dirs: Vec<PathBuf> = Vec::new();
        for s in &self.compilers {
            if let Some(d) = s.program.parent() {
                if !d.as_os_str().is_empty() && !dirs.iter().any(|x| x == d) {
                    dirs.push(d.to_path_buf());
                }
            }
        }
        let (cols, rows) = (self.size.0.saturating_sub(1).max(20), 10);
        self.terminal = Some(TerminalPane::spawn(
            &term::default_shell(),
            &self.root,
            &env,
            &dirs,
            rows,
            cols,
        )?);
        Ok(())
    }
    fn toggle_focus(&mut self, id: &str) -> Result<(), FrontendError> {
        match id {
            "focus_explorer" => {
                if self.focus == Focus::Explorer {
                    self.focus = Focus::Editor;
                } else {
                    self.explorer_visible = true;
                    self.focus = Focus::Explorer;
                }
            }
            "focus_assistant" => {
                if self.focus == Focus::Assistant {
                    self.focus = Focus::Editor;
                } else {
                    self.assistant_visible = true;
                    self.focus = Focus::Assistant;
                }
            }
            _ => {
                if self.focus == Focus::Terminal {
                    self.focus = Focus::Editor;
                } else {
                    self.ensure_terminal()?;
                    self.panel = Panel::Terminal;
                    self.panel_visible = true;
                    self.focus = Focus::Terminal;
                }
            }
        }
        Ok(())
    }
    /// Remove a panel from the layout. Its state (tree, conversation and unsent
    /// input, the running shell) is kept and returns when the panel is reopened.
    fn hide_panel(&mut self, id: &str) {
        let (focus, name, digit) = match id {
            "hide_explorer" => {
                self.explorer_visible = false;
                (Focus::Explorer, "Explorer hidden", '1')
            }
            "hide_assistant" => {
                self.assistant_visible = false;
                (Focus::Assistant, "AI panel hidden, chat kept", '2')
            }
            _ => {
                if self.panel == Panel::Terminal {
                    self.panel_visible = false;
                }
                (Focus::Terminal, "Terminal hidden, shell running", '3')
            }
        };
        if self.focus == focus {
            self.focus = Focus::Editor;
        }
        self.notification = format!("{name} ({} reopens)", self.panel_key(digit));
    }
    /// Tell the workspace whether the terminal reports kitty keyboard events
    /// (press/repeat/release). Without them a held key cannot be observed.
    pub fn set_keyboard_enhanced(&mut self, enhanced: bool) {
        self.keyboard_enhanced = enhanced;
        self.held_panel = None;
    }
    /// Alt+1/2/3 (or Ctrl+Alt+1/2/3) open a panel (again: back to the editor). With the digit
    /// still held, Delete hides that panel. "Held" is known only from kitty
    /// keyboard release events, so the hide chord is never inferred from a
    /// sequence of separate presses. Returns None when the key is not a chord.
    fn panel_chord(&mut self, key: KeyEvent) -> Option<Result<(), FrontendError>> {
        let chord = key.modifiers == KeyModifiers::ALT
            || key.modifiers == KeyModifiers::CONTROL | KeyModifiers::ALT;
        if chord && matches!(key.code, KeyCode::Char('0'..='9')) {
            let id = self.shortcut(key).filter(|id| id.starts_with("focus_"));
            match key.kind {
                KeyEventKind::Release => {
                    if self.held_panel.is_some() && self.held_panel == id {
                        self.held_panel = None;
                    }
                    return Some(Ok(()));
                }
                KeyEventKind::Repeat if id.is_some() => return Some(Ok(())),
                _ => {}
            }
            if let Some(id) = id {
                if self.dialog.is_some() {
                    return None;
                }
                self.held_panel = self.keyboard_enhanced.then(|| id.clone());
                return Some(self.command(&id));
            }
        }
        if key.kind == KeyEventKind::Release {
            return None;
        }
        if chord && key.code == KeyCode::Delete && self.dialog.is_none() {
            if let Some(held) = self.held_panel.clone() {
                self.hide_panel(&held.replacen("focus_", "hide_", 1));
                return Some(Ok(()));
            }
        }
        self.held_panel = None;
        None
    }
    fn apply_proposal(&mut self) -> Result<(), FrontendError> {
        let Some((path, proposal)) = self.assistant.take_proposal_if(true) else {
            return Ok(());
        };
        let index = self
            .documents
            .iter()
            .position(|d| d.editor().state().path() == Some(path.as_path()))
            .ok_or_else(|| service_error("The proposed file is no longer open"))?;
        let doc = &mut self.documents[index];
        let len = doc.editor().state().buffer().byte_len();
        if doc.editor().state().buffer().contents() != proposal.original {
            self.assistant
                .error("The buffer changed after the proposal; ask again with /edit");
            return Ok(());
        }
        doc.session.editor.replace(0..len, &proposal.proposed)?;
        self.active = index;
        self.assistant
            .info("Applied to the editor buffer (undo with u; nothing is written until :w)");
        self.notification = "AI edit applied to the buffer; u undoes, :w saves".into();
        Ok(())
    }
    /// Host side of `:` commands typed in the editor.
    fn host_command(&mut self, line: &str) -> Result<(), FrontendError> {
        let (name, arg) = line
            .split_once(char::is_whitespace)
            .map_or((line, ""), |(n, a)| (n, a.trim()));
        match name {
            "e" | "edit" | "open" if !arg.is_empty() => self.open_file(Path::new(arg)),
            "w" | "write" | "saveas" if !arg.is_empty() => self.save_as(Path::new(arg)),
            "label" | "l" => self.goto_label(arg),
            "hex" | "elf" | "disasm" if !arg.is_empty() => {
                self.open_binary(Path::new(arg))?;
                self.binary_mode(name);
                Ok(())
            }
            line if !line.is_empty()
                && line.bytes().all(|b| b.is_ascii_digit())
                && arg.is_empty() =>
            {
                self.goto_line(line.parse().unwrap_or(usize::MAX))
            }
            "e" | "edit" | "open" => self.command("open"),
            "ai" if !arg.is_empty() => {
                self.assistant_visible = true;
                self.assistant.input = arg.to_string();
                let ctx = self.editor_context();
                self.assistant.submit(&ctx);
                Ok(())
            }
            id if COMMANDS.iter().any(|(c, _)| *c == id) && arg.is_empty() => self.command(id),
            _ => Err(service_error(format!(
                "Unknown command :{line} (Ctrl+K lists commands; :help shows keys)"
            ))),
        }
    }
    /// `:w <file>` / `:saveas <file>`: write the active document to a new
    /// file (relative to the project) and keep editing it there. An existing
    /// file is never overwritten.
    fn save_as(&mut self, path: &Path) -> Result<(), FrontendError> {
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        };
        self.documents[self.active].session.editor.save_as(&path)?;
        self.notification = format!("Saved as {}", path.display());
        let _ = self.refresh();
        Ok(())
    }
    /// Show a file in the binary view (read-only until `i`).
    fn open_binary(&mut self, path: &Path) -> Result<(), FrontendError> {
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        };
        if self.binary.as_ref().is_some_and(|b| b.file.is_modified()) {
            return Err(service_error(
                "The open binary has unsaved changes: w saves, q closes (asks first)",
            ));
        }
        let b = &self.config.binary;
        let settings = binview::Settings {
            bytes_per_row: b.bytes_per_row,
            allow_edit: b.allow_edit,
            intel: b.disasm_syntax == "intel",
            objdump: b.objdump.clone(),
            llvm_objdump: b.llvm_objdump.clone(),
        };
        let view = binview::BinaryView::open(&path, settings).map_err(service_error)?;
        self.notification = format!(
            "Opened {} as binary ({} bytes, read-only): Tab/e/d ELF and disassembly, i edits, q closes",
            path.display(),
            view.file.len()
        );
        self.binary = Some(view);
        self.popup = None;
        self.focus = Focus::Editor;
        Ok(())
    }
    /// Switch the open binary view to `hex`, `elf` or `disasm`.
    fn binary_mode(&mut self, id: &str) {
        if let Some(view) = &mut self.binary {
            view.set_mode(match id {
                "elf" => binview::Mode::Elf,
                "disasm" => binview::Mode::Disasm,
                _ => binview::Mode::Hex,
            });
            self.focus = Focus::Editor;
        }
    }
    /// Close the binary view, asking first when it has unsaved edits.
    fn close_binary(&mut self) {
        match &self.binary {
            Some(view) if view.file.is_modified() => {
                self.dialog = Some(Dialog::Confirm {
                    text: format!(
                        "Discard {} changed bytes in {}? y discards; Esc cancels (w saves)",
                        view.file.changed(),
                        view.path().display()
                    ),
                    action: Confirm::CloseBinary,
                })
            }
            _ => self.binary = None,
        }
    }
    /// Keys for the binary view (after global shortcuts).
    fn binary_key(&mut self, key: KeyEvent) -> Option<Result<(), FrontendError>> {
        let view = self.binary.as_mut()?;
        match view.key(key) {
            binview::Outcome::Done => {}
            binview::Outcome::Ignored => return None,
            binview::Outcome::Close => self.close_binary(),
            binview::Outcome::Save => {
                if view.file.is_modified() {
                    self.dialog = Some(Dialog::Confirm {
                        text: format!(
                            "Write {} changed bytes to {}? Same length; untouched bytes are kept. y writes; Esc cancels",
                            view.file.changed(),
                            view.path().display()
                        ),
                        action: Confirm::SaveBinary,
                    });
                } else {
                    view.message = "No changes to write".into();
                }
            }
        }
        Some(Ok(()))
    }
    /// Assembly dialect of the active document, if it is assembly.
    fn asm_dialect(&self) -> Option<embedide_editor::asm::Dialect> {
        let doc = &self.documents[self.active];
        let state = doc.editor().state();
        if language_of(state.path()) != "asm" {
            return None;
        }
        doc.viewport.dialect().or_else(|| {
            let sample = state
                .buffer()
                .slice(0..state.buffer().byte_len().min(16 * 1024))
                .ok()?;
            Some(embedide_editor::asm::Dialect::detect(state.path(), &sample))
        })
    }
    /// Labels of the active assembly document (empty for other languages).
    fn asm_labels(&self) -> Vec<embedide_editor::asm::Label> {
        let Some(dialect) = self.asm_dialect() else {
            return Vec::new();
        };
        let text = self.editor().state().buffer().contents();
        embedide_editor::asm::labels(&text, dialect)
    }
    /// `:label [name]`: jump to a label of this assembly file (the word under
    /// the cursor without a name).
    fn goto_label(&mut self, name: &str) -> Result<(), FrontendError> {
        let name = if name.is_empty() {
            let state = self.editor().state();
            let cursor = state.cursor();
            let line = state
                .line_range(state.position()?.line)
                .unwrap_or(cursor..cursor);
            let text = state.buffer().slice(line.clone())?;
            let at = cursor - line.start;
            let is_label = |c: char| c.is_alphanumeric() || matches!(c, '_' | '.' | '$');
            let start = text[..at].rfind(|c| !is_label(c)).map_or(0, |i| i + 1);
            let end = text[at..]
                .find(|c| !is_label(c))
                .map_or(text.len(), |i| at + i);
            text[start..end].to_string()
        } else {
            name.to_string()
        };
        if self.asm_dialect().is_none() {
            return Err(service_error(
                ":label works in assembly files (Ctrl+K: Go to Label)",
            ));
        }
        match self.asm_labels().into_iter().find(|l| l.name == name) {
            Some(label) => {
                self.goto_line(label.line + 1)?;
                self.notification = format!("{name}: line {}", label.line + 1);
                Ok(())
            }
            None => Err(service_error(format!("No label {name:?} in this file"))),
        }
    }
    /// `:<line>`: move to a 1-based line (clamped to the last line), first column.
    fn goto_line(&mut self, line: usize) -> Result<(), FrontendError> {
        let editor = &mut self.documents[self.active].session.editor;
        let count = editor.state().line_count().max(1);
        let index = line.clamp(1, count) - 1;
        let start = editor.state().line_range(index).map_or(0, |r| r.start);
        editor.set_cursor(start)?;
        self.focus = Focus::Editor;
        Ok(())
    }
    /// Active engine, shared with all other frontends.
    pub fn editor(&self) -> &Editor {
        self.documents[self.active].editor()
    }
    /// Number of independently owned documents.
    pub fn document_count(&self) -> usize {
        self.documents.len()
    }
    /// Current focus for headless frontend tests.
    pub fn focus(&self) -> Focus {
        self.focus
    }
    /// Accepted workspace quit, after dirty-buffer protection.
    pub fn is_closed(&self) -> bool {
        self.closed
    }
    /// Current status/error text.
    pub fn notification(&self) -> &str {
        &self.notification
    }
    /// Open a file while retaining the previous editor and its undo history.
    pub fn open_file(&mut self, path: &Path) -> Result<(), FrontendError> {
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        };
        if fs::symlink_metadata(&path)?.file_type().is_symlink() {
            return Err(service_error("Symlink files are not opened"));
        }
        let path = path.canonicalize()?;
        // Binary files never go through the text editor (no UTF-8 decoding,
        // no normalisation): they open read-only in the binary view.
        if embedide_core::binary::looks_binary(&path) {
            return self.open_binary(&path);
        }
        if let Some(index) = self
            .documents
            .iter()
            .position(|doc| doc.editor().state().path() == Some(path.as_path()))
        {
            self.active = index;
        } else {
            let doc = self.new_document(crate::open(&path)?);
            if self.documents.len() == 1
                && self.editor().state().path().is_none()
                && !self.editor().state().is_modified()
            {
                self.documents[0] = doc;
            } else {
                self.documents.push(doc);
                self.active = self.documents.len() - 1;
            }
        }
        self.focus = Focus::Editor;
        self.notification = format!("Opened {}", path.display());
        Ok(())
    }
    /// Open the document named on the command line: an existing file, or a new
    /// unsaved buffer bound to a missing path (created only on `:w`, never
    /// replacing a file that appears meanwhile).
    pub fn open_launch_file(
        &mut self,
        file: &embedide_workspace::LaunchFile,
    ) -> Result<(), FrontendError> {
        match file {
            embedide_workspace::LaunchFile::Existing(path) => self.open_file(path),
            embedide_workspace::LaunchFile::New(path) => {
                let doc = self.new_document(Editor::create(path)?);
                if self.documents.len() == 1
                    && self.editor().state().path().is_none()
                    && !self.editor().state().is_modified()
                {
                    self.documents[0] = doc;
                } else {
                    self.documents.push(doc);
                    self.active = self.documents.len() - 1;
                }
                self.focus = Focus::Editor;
                self.notification = format!("New file {} (written on :w)", path.display());
                Ok(())
            }
        }
    }
    /// Re-read the tree, keeping expanded directories open and the selected
    /// path selected. Directories that disappeared or became unreadable stay
    /// collapsed; only a failure to read the workspace root is an error.
    fn refresh(&mut self) -> core::Result<()> {
        fn fill(
            root: &Path,
            dir: &Path,
            depth: usize,
            expanded: &std::collections::HashSet<PathBuf>,
            tree: &mut Vec<TreeRow>,
        ) -> core::Result<()> {
            for entry in explorer::read_directory(root, dir)? {
                let open = entry.directory && !entry.symlink && expanded.contains(&entry.path);
                let path = entry.path.clone();
                tree.push(TreeRow {
                    entry,
                    depth,
                    expanded: false,
                });
                if open && tree.len() < 20000 {
                    let at = tree.len() - 1;
                    if fill(root, &path, depth + 1, expanded, tree).is_ok() {
                        tree[at].expanded = true;
                    } else {
                        tree.truncate(at + 1);
                    }
                }
            }
            Ok(())
        }
        let expanded = self
            .tree
            .iter()
            .filter(|row| row.expanded)
            .map(|row| row.entry.path.clone())
            .collect();
        let selected = self
            .tree
            .get(self.selected)
            .map(|row| row.entry.path.clone());
        let mut tree = Vec::new();
        fill(&self.root, &self.root, 0, &expanded, &mut tree)?;
        self.tree = tree;
        self.selected = selected
            .and_then(|path| self.tree.iter().position(|row| row.entry.path == path))
            .unwrap_or(self.selected)
            .min(self.tree.len().saturating_sub(1));
        self.tree_top = self.tree_top.min(self.selected);
        Ok(())
    }
    fn toggle_entry(&mut self) -> Result<(), FrontendError> {
        let Some(row) = self.tree.get(self.selected) else {
            return Ok(());
        };
        if row.entry.symlink {
            return Err(service_error("Symlink traversal is disabled"));
        }
        if !row.entry.directory {
            return self.open_file(&row.entry.path.clone());
        }
        let depth = row.depth;
        if row.expanded {
            let end = self.tree[self.selected + 1..]
                .iter()
                .position(|r| r.depth <= depth)
                .map_or(self.tree.len(), |i| self.selected + 1 + i);
            self.tree.drain(self.selected + 1..end);
            self.tree[self.selected].expanded = false;
        } else {
            let entries =
                explorer::read_directory(&self.root, &row.entry.path).map_err(service_error)?;
            if self.tree.len() + entries.len() > 20000 {
                return Err(service_error(
                    "Explorer has 20000 visible rows; collapse directories first",
                ));
            }
            self.tree[self.selected].expanded = true;
            self.tree.splice(
                self.selected + 1..self.selected + 1,
                entries.into_iter().map(|entry| TreeRow {
                    entry,
                    depth: depth + 1,
                    expanded: false,
                }),
            );
        }
        Ok(())
    }
    fn input(&mut self, label: &str, kind: InputKind) {
        self.dialog = Some(Dialog::Input {
            label: label.into(),
            text: String::new(),
            kind,
        });
    }
    fn show_panel(&mut self, panel: Panel) {
        self.panel = panel;
        self.panel_visible = true;
        self.focus = Focus::Panel;
        self.panel_scroll = 0;
    }
    fn start_plan(&mut self, plan: BuildPlan, kind: Operation) -> core::Result<()> {
        let env = self.config.environment.clone();
        self.start_plan_env(plan, kind, env)
    }
    fn start_plan_env(
        &mut self,
        plan: BuildPlan,
        kind: Operation,
        env: BTreeMap<String, String>,
    ) -> core::Result<()> {
        if self.operation.is_some() {
            return Err(core::Error(
                "Another task is running; cancel or wait first".into(),
            ));
        }
        let task = ProcessTask::start(plan, env)?;
        self.operation = Some(Running {
            task,
            kind,
            partial: [String::new(), String::new()],
        });
        if kind == Operation::Build || kind == Operation::Flash {
            let previous_focus = self.focus;
            self.show_panel(Panel::Build);
            self.focus = previous_focus;
            self.build_status = "Running".into();
        } else {
            self.show_panel(if kind == Operation::Search {
                Panel::Problems
            } else {
                Panel::Tasks
            });
        }
        Ok(())
    }
    fn plan(&self, check: bool) -> core::Result<BuildPlan> {
        let project = core::inspect(&self.root)?;
        if check && project.build_system(None)? != BuildSystem::Cargo {
            return Err(core::Error(
                "Check is currently implemented for Cargo only".into(),
            ));
        }
        self.config
            .build_plan(&project, &self.profile, self.target, check)
    }
    /// Build an `embedide.toml` project with the compilers it selects.
    fn build_manifest(&mut self, check: bool) -> Result<(), FrontendError> {
        if check {
            return Err(service_error(
                "Check applies to legacy Cargo projects; F5 builds embedide.toml projects",
            ));
        }
        if self.operation.is_some() {
            return Err(service_error(
                "Another task is running; cancel or wait first",
            ));
        }
        let manifest = self.manifest.as_ref().expect("manifest");
        let tools = embedide_build::registry(&self.user, true);
        let req = embedide_build::Request {
            profile: self.profile.clone(),
            ..Default::default()
        };
        let prepared =
            embedide_build::prepare(&self.root, Some(manifest), &self.user, &tools, &req)
                .map_err(|e| service_error(e.0))?;
        prepared.begin().map_err(|e| service_error(e.0))?;
        self.problems.clear();
        self.build_log.clear();
        self.problem = 0;
        for s in &prepared.toolchains {
            self.build_log.push(&format!(
                "[{}] {} → {} {}",
                s.language.id(),
                s.id,
                s.program.display(),
                s.version.as_deref().unwrap_or("")
            ));
        }
        self.compilers = prepared.toolchains.clone();
        if prepared.steps.is_empty() {
            self.build_log.push("Up to date");
            self.build_status = "Succeeded".into();
            self.notification = "Build up to date".into();
            let previous = self.focus;
            self.show_panel(Panel::Build);
            self.focus = previous;
            return Ok(());
        }
        let mut env = self.config.environment.clone();
        env.extend(prepared.env.clone());
        self.start_plan_env(prepared.plan(), Operation::Build, env)
            .map_err(service_error)?;
        self.prepared = Some(prepared);
        Ok(())
    }
    fn debug_command(&self, command: DebugCommand) -> core::Result<()> {
        self.debug
            .as_ref()
            .ok_or_else(|| core::Error("Start a debugger session first".into()))?
            .command(command)
    }
    fn selected_path(&self) -> core::Result<PathBuf> {
        self.tree
            .get(self.selected)
            .map(|row| row.entry.path.clone())
            .ok_or_else(|| core::Error("Select an explorer entry first".into()))
    }
    fn file_is_open(&self, path: &Path) -> bool {
        self.documents.iter().any(|doc| {
            doc.editor()
                .state()
                .path()
                .is_some_and(|p| p.starts_with(path))
        })
    }
    fn close_document(&mut self) {
        self.popup = None;
        if let Some(path) = self.editor().state().path().map(Path::to_path_buf) {
            for slot in self.lsp.values_mut() {
                if let LspSlot::Running(client) = slot {
                    client.close(&path);
                }
            }
        }
        if self.documents.len() == 1 {
            self.closed = true;
        } else {
            self.documents.remove(self.active);
            self.active = self.active.saturating_sub(1);
        }
    }
    /// Execute a named command. Flash/delete/discard/debug commands create explicit dialogs.
    pub fn command(&mut self, id: &str) -> Result<(), FrontendError> {
        let result = self.command_inner(id);
        if let Err(error) = &result {
            self.notification = format!("Error: {error}");
        }
        result
    }
    fn command_inner(&mut self, id: &str) -> Result<(), FrontendError> {
        match id {
            "build" | "check" => {
                if self
                    .documents
                    .iter()
                    .any(|doc| doc.editor().state().is_modified())
                {
                    return Err(service_error(
                        "Save modified documents before building; builds use files on disk",
                    ));
                }
                if self.manifest.is_some() {
                    return self.build_manifest(id == "check");
                }
                let plan = self.plan(id == "check").map_err(service_error)?;
                self.start_plan(plan, Operation::Build)
                    .map_err(service_error)?;
                self.problems.clear();
                self.build_log.clear();
                self.problem = 0;
            }
            "clean" if self.manifest.is_some() => {
                if self.operation.is_some() {
                    return Err(service_error("Wait for the active task before cleaning"));
                }
                match embedide_build::plan_clean(&self.root, self.manifest.as_ref())
                    .map_err(|e| service_error(e.0))?
                {
                    embedide_build::CleanAction::Remove(dir) => {
                        fs::remove_dir_all(&dir)?;
                        self.notification = format!("Removed {}", dir.display());
                        self.refresh().map_err(service_error)?;
                    }
                    embedide_build::CleanAction::Run(spec) => self
                        .start_plan(
                            BuildPlan {
                                directory: self.root.clone(),
                                system: BuildSystem::Make,
                                steps: vec![spec],
                            },
                            Operation::Task,
                        )
                        .map_err(service_error)?,
                    embedide_build::CleanAction::Nothing(why) => {
                        self.notification = format!("Nothing to clean: {why}")
                    }
                }
            }
            "clean" => {
                let plan = self.plan(false).map_err(service_error)?;
                let args = match plan.system {
                    BuildSystem::Cargo => vec!["clean".into(), "--offline".into()],
                    BuildSystem::Cmake => vec![
                        "--build".into(),
                        self.config.build_directory.clone(),
                        "--target".into(),
                        "clean".into(),
                    ],
                    BuildSystem::Make => vec!["clean".into()],
                    BuildSystem::Zig => {
                        return Err(service_error(
                            "Zig clean is project-defined; configure a task",
                        ))
                    }
                };
                let program = plan.steps.last().expect("adapter plan").program.clone();
                self.start_plan(
                    BuildPlan {
                        steps: vec![ProcessSpec { program, args }],
                        ..plan
                    },
                    Operation::Task,
                )
                .map_err(service_error)?;
            }
            "cancel" => {
                if let Some(running) = &self.operation {
                    running.task.cancel();
                    self.notification = "Cancellation requested".into();
                } else if self.assistant.busy() {
                    self.assistant.cancel();
                    self.notification = "AI request cancelled".into();
                }
            }
            "focus_explorer" | "focus_assistant" | "focus_terminal" => self.toggle_focus(id)?,
            "suggest" => self.suggest(true, None)?,
            "ai_suggest" => {
                if self.binary.is_some() {
                    self.notification = "AI suggestions work in text files".into();
                } else if self.editor().input().mode() != Mode::Insert {
                    self.notification =
                        "AI suggestions work in Insert mode (press i, then Alt+\\)".into();
                } else {
                    self.focus = Focus::Editor;
                    let ctx = self.editor_context();
                    self.notification = self.assistant.suggest_now(&ctx);
                }
            }
            "hex" | "elf" | "disasm" => {
                if self.binary.is_none() {
                    let path = self
                        .editor()
                        .state()
                        .path()
                        .map(Path::to_path_buf)
                        .ok_or_else(|| service_error("Open a file first, or :hex <path>"))?;
                    self.open_binary(&path)?;
                }
                self.binary_mode(id);
            }
            "close_binary" => self.close_binary(),
            "labels" => {
                self.dialog = Some(Dialog::Palette {
                    query: "label: ".into(),
                    selected: 0,
                })
            }
            id if id.starts_with("line:") => {
                let line: usize = id[5..].parse().map_err(service_error)?;
                self.goto_line(line)?;
            }
            "lsp_restart" => {
                self.lsp.clear();
                if let Some(popup) = &mut self.popup {
                    popup.request = None;
                }
                self.notification =
                    "Language servers stopped; they start again on the next completion".into();
            }
            "mouse" => {
                self.mouse = !self.mouse;
                self.notification = if self.mouse {
                    "Mouse on: click focuses and places the cursor, wheel scrolls; Shift+drag selects text"
                } else {
                    "Mouse off: the terminal selects and copies text as usual"
                }
                .into();
            }
            "hide_explorer" | "hide_assistant" | "hide_terminal" => self.hide_panel(id),
            "assistant" => {
                self.assistant_visible = !self.assistant_visible;
                if !self.assistant_visible && self.focus == Focus::Assistant {
                    self.focus = Focus::Editor;
                }
            }
            "terminal_restart" => {
                self.terminal = None;
                self.focus = Focus::Editor;
                self.toggle_focus("focus_terminal")?;
            }
            "complete" => {
                self.assistant_visible = true;
                let ctx = self.editor_context();
                self.assistant.start_completion(&ctx);
            }
            "explain" => {
                self.assistant_visible = true;
                self.assistant.input = "/explain".into();
                let ctx = self.editor_context();
                self.assistant.submit(&ctx);
            }
            "compilers" => {
                self.tools_log.clear();
                match &self.manifest {
                    None => self.tools_log.push("No embedide.toml: builds use the legacy project adapters (Make/CMake/Cargo/Zig)."),
                    Some(_) => {
                        self.select_compilers();
                        if self.compilers.is_empty() {
                            self.tools_log.push("No toolchain selected; see the notes below and `embed toolchain list`.");
                        }
                        for s in &self.compilers {
                            self.tools_log.push(&format!(
                                "[{}] {} → {} ({})",
                                s.language.id(),
                                s.id,
                                s.program.display(),
                                s.reason
                            ));
                        }
                    }
                }
                for note in &self.config_notes {
                    self.tools_log.push(note);
                }
                self.tools_log.push("Choose compilers in embedide.toml [toolchain] / [profiles.<name>.toolchain] or user config.toml [toolchains.defaults]. Nothing is installed automatically.");
                self.show_panel(Panel::Tools);
            }
            "edit_project_config" => {
                let path = self.root.join("embedide.toml");
                if path.exists() {
                    self.open_file(&path)?;
                } else {
                    return Err(service_error(
                        "This project has no embedide.toml; create one with `embed project init`",
                    ));
                }
            }
            "edit_user_config" => {
                let path = UserConfig::path().map_err(|e| service_error(e.0))?;
                if path.exists() {
                    self.open_file(&path)?;
                } else {
                    // A new buffer bound to the path; nothing is written before :w.
                    let doc = self.new_document(crate::open(&path)?);
                    self.documents.push(doc);
                    self.active = self.documents.len() - 1;
                    self.focus = Focus::Editor;
                    self.notification = format!("New file {} (written only on :w)", path.display());
                }
            }
            "find" => {
                self.dialog = Some(Dialog::Palette {
                    query: "file: ".into(),
                    selected: 0,
                })
            }
            "open" => self.input(
                "Open file: path (Enter opens; Esc cancels)",
                InputKind::Open,
            ),
            "search" | "search_ignore_case" => self.input(
                "Project search: literal text (requires rg; Esc cancels)",
                InputKind::Search(id.ends_with("ignore_case")),
            ),
            "problems" => self.show_panel(Panel::Problems),
            "toggle_panel" => {
                self.panel_visible = !self.panel_visible;
                if !self.panel_visible {
                    self.focus = Focus::Editor;
                }
            }
            "explorer" => {
                self.explorer_visible = !self.explorer_visible;
                self.focus = if self.explorer_visible {
                    Focus::Explorer
                } else {
                    Focus::Editor
                };
            }
            "refresh" => self.refresh().map_err(service_error)?,
            "reveal" => {
                let path = self
                    .editor()
                    .state()
                    .path()
                    .map(Path::to_path_buf)
                    .ok_or_else(|| service_error("Untitled document"))?;
                self.refresh().map_err(service_error)?;
                let relative = path.strip_prefix(&self.root).map_err(service_error)?;
                let mut current = self.root.clone();
                for component in relative.components() {
                    current.push(component);
                    if let Some(index) = self.tree.iter().position(|row| row.entry.path == current)
                    {
                        self.selected = index;
                        if self.tree[index].entry.directory && !self.tree[index].expanded {
                            self.toggle_entry()?;
                        }
                    }
                }
                self.focus = Focus::Explorer;
                self.explorer_visible = true;
            }
            "new_file" => self.input("Create file: workspace-relative path", InputKind::NewFile),
            "new_directory" => self.input(
                "Create directory: workspace-relative path",
                InputKind::NewDirectory,
            ),
            "rename" | "delete" => {
                let path = self.selected_path().map_err(service_error)?;
                if self.file_is_open(&path) {
                    return Err(service_error(
                        "Close documents under this path before renaming/deleting",
                    ));
                }
                if id == "rename" {
                    self.input(
                        "Rename to: workspace-relative path",
                        InputKind::Rename(path),
                    );
                } else {
                    self.dialog = Some(Dialog::Confirm {
                        text: format!(
                            "Delete {}? Empty directories only. Press y to confirm; Esc cancels.",
                            path.display()
                        ),
                        action: Confirm::Delete(path),
                    });
                }
            }
            "target" => {
                self.dialog = Some(Dialog::Palette {
                    query: "target: ".into(),
                    selected: 0,
                });
            }
            "profile" => {
                self.dialog = Some(Dialog::Palette {
                    query: "profile: ".into(),
                    selected: 0,
                });
            }
            "tools" => {
                self.tools_log.clear();
                for tool in core::toolchains() {
                    self.tools_log.push(&format!(
                        "{}: {}",
                        tool.name,
                        tool.path
                            .map_or("MISSING".into(), |p| p.display().to_string())
                    ));
                }
                self.tools_log.push("PATH inventory only; versions and hardware compatibility not verified. Configure executable overrides in native.tools.");
                self.show_panel(Panel::Tools);
            }
            "probes" => {
                let program = self
                    .config
                    .tools
                    .get("probe-rs")
                    .cloned()
                    .unwrap_or("probe-rs".into());
                self.start_plan(
                    BuildPlan {
                        directory: self.root.clone(),
                        system: BuildSystem::Cargo,
                        steps: vec![ProcessSpec {
                            program,
                            args: vec!["list".into()],
                        }],
                    },
                    Operation::Probes,
                )
                .map_err(service_error)?;
            }
            "flash" => {
                if self.operation.is_some() {
                    return Err(service_error("Wait for the active task before flashing"));
                }
                if self.serial.is_some() || self.debug.is_some() {
                    return Err(service_error(
                        "Disconnect serial and debugger before flashing",
                    ));
                }
                if self
                    .documents
                    .iter()
                    .any(|d| d.editor().state().is_modified())
                {
                    return Err(service_error(
                        "Save documents and build the desired firmware first",
                    ));
                }
                let target = self
                    .target
                    .and_then(|i| self.config.targets.get(i))
                    .ok_or_else(|| {
                        service_error("Configure and select a target with chip, probe and artifact")
                    })?;
                let program = self
                    .config
                    .tools
                    .get("probe-rs")
                    .map(String::as_str)
                    .unwrap_or("probe-rs");
                let plan =
                    hardware::flash_plan(&self.root, target, program).map_err(service_error)?;
                self.dialog = Some(Dialog::Confirm {
                    text: format!(
                        "FLASH {}: {} {:?}. Overwrites device firmware. y confirms; Esc cancels.",
                        target.name, plan.steps[0].program, plan.steps[0].args
                    ),
                    action: Confirm::Flash(plan),
                });
            }
            "ports" => {
                if self.inventory.is_none() {
                    let (tx, rx) = mpsc::channel();
                    self.inventory = Some(rx);
                    thread::spawn(move || {
                        let lines =
                            hardware::serial_ports().unwrap_or_else(|e| vec![e.to_string()]);
                        let _ = tx.send(lines);
                    });
                }
                self.show_panel(Panel::Tools);
                self.notification = "Enumerating actual serial devices...".into();
            }
            "serial_port" => self.input(
                "Serial device and baud, separated by a space (e.g. /dev/ttyACM0 115200)",
                InputKind::SerialPort,
            ),
            "serial" => {
                if self.serial.is_some() {
                    return Err(service_error("Serial monitor is already connected"));
                }
                self.serial =
                    Some(SerialMonitor::connect(&self.config.serial).map_err(service_error)?);
                self.serial_started = Instant::now();
                self.serial_log.push(&format!(
                    "Connected {} at {} baud",
                    self.config.serial.port, self.config.serial.baud
                ));
                self.show_panel(Panel::Serial);
            }
            "serial_disconnect" => {
                self.serial = None;
                self.serial_log.push("Disconnected");
            }
            "serial_send" => self.input(
                "Serial outgoing text (framing/receive_only from settings)",
                InputKind::SerialSend,
            ),
            "serial_pause" => {
                self.serial_paused = !self.serial_paused;
                if self.serial_paused {
                    self.serial_snapshot = self.serial_log.lines.iter().cloned().collect();
                }
                self.show_panel(Panel::Serial);
            }
            "serial_timestamps" => self.serial_timestamps = !self.serial_timestamps,
            "serial_clear" => {
                self.serial_log.clear();
                self.serial_snapshot.clear();
            }
            "serial_export" => self.input(
                "Export serial log: NEW workspace-relative file",
                InputKind::SerialExport,
            ),
            "debug" => {
                if self.debug.is_some() {
                    return Err(service_error("Debugger is already active"));
                }
                if self.operation.is_some() || self.serial.is_some() {
                    return Err(service_error(
                        "Stop tasks/serial before starting hardware debugging",
                    ));
                }
                self.dialog = Some(Dialog::Confirm { text: format!("Start GDB {:?}, ELF {:?}, remote {:?}, OpenOCD {:?}, configs {:?}? Project configs execute code. y confirms; Esc cancels.", self.config.debug.gdb, self.config.debug.elf, self.config.debug.remote, self.config.debug.openocd, self.config.debug.configs), action: Confirm::Debug });
            }
            "debug_stop" => {
                self.debug = None;
                self.debug_status = "Disconnected".into();
                self.debug_log.push("Session stopped");
            }
            "continue" => self
                .debug_command(DebugCommand::Continue)
                .map_err(service_error)?,
            "interrupt" => self
                .debug_command(DebugCommand::Interrupt)
                .map_err(service_error)?,
            "step" => self
                .debug_command(DebugCommand::Step)
                .map_err(service_error)?,
            "next" => self
                .debug_command(DebugCommand::Next)
                .map_err(service_error)?,
            "backtrace" => {
                self.debug_command(DebugCommand::Backtrace)
                    .map_err(service_error)?;
                self.show_panel(Panel::Debug);
            }
            "registers" => {
                self.debug_command(DebugCommand::Registers)
                    .map_err(service_error)?;
                self.show_panel(Panel::Debug);
            }
            "breakpoint" => {
                let path = self
                    .editor()
                    .state()
                    .path()
                    .ok_or_else(|| service_error("Save file before setting a breakpoint"))?
                    .to_string_lossy()
                    .into_owned();
                self.debug_command(DebugCommand::Break(
                    path,
                    self.editor().state().position()?.line + 1,
                ))
                .map_err(service_error)?;
                self.show_panel(Panel::Debug);
            }
            "break_delete" => self.input("Remove breakpoint: numeric ID", InputKind::BreakDelete),
            "settings" => self.show_panel(Panel::Settings),
            "reload" => {
                self.config = NativeConfig::load(&self.root).map_err(service_error)?;
                self.mouse = self.config.mouse;
                let (user, manifest, notes) = load_configs(&self.root);
                self.assistant = Assistant::new(user.ai.clone());
                self.user = user;
                self.manifest = manifest;
                self.config_notes = notes;
                self.config_notes.extend(unknown_key_commands(&self.config));
                self.select_compilers();
                self.target = self.target.filter(|i| *i < self.config.targets.len());
                if !self.config.profiles.contains_key(&self.profile) {
                    self.profile = self
                        .config
                        .profiles
                        .keys()
                        .next()
                        .expect("validated profile")
                        .clone();
                }
                for doc in &mut self.documents {
                    doc.viewport.set_tab_width(self.config.tab_width);
                }
                self.notification = "Settings reloaded; active hardware sessions retain their original configuration".into();
            }
            "theme" => {
                self.config.theme = match self.config.theme.as_str() {
                    "dark" => "light",
                    "light" => "mono",
                    _ => "dark",
                }
                .into()
            }
            "help" => self.show_panel(Panel::Help),
            "close" if self.binary.is_some() => self.close_binary(),
            "close" => {
                if self.editor().state().is_modified() {
                    self.dialog = Some(Dialog::Confirm {
                        text: "Discard unsaved changes in this document? y confirms; Esc cancels."
                            .into(),
                        action: Confirm::Close,
                    });
                } else {
                    self.close_document();
                }
            }
            "quit" => {
                if self
                    .documents
                    .iter()
                    .any(|doc| doc.editor().state().is_modified())
                    || self.binary.as_ref().is_some_and(|b| b.file.is_modified())
                {
                    self.dialog = Some(Dialog::Confirm {
                        text: "Discard ALL unsaved documents and quit? y confirms; Esc cancels."
                            .into(),
                        action: Confirm::Quit,
                    });
                } else {
                    self.closed = true;
                }
            }
            id if id.starts_with("file:") => self.open_file(Path::new(&id[5..]))?,
            id if id.starts_with("tree:") => {
                self.selected = id[5..].parse().map_err(service_error)?;
                self.toggle_entry()?;
                self.dialog = Some(Dialog::Palette {
                    query: "file: ".into(),
                    selected: 0,
                });
            }
            id if id.starts_with("target:") => {
                let i = id[7..].parse::<usize>().map_err(service_error)?;
                if i >= self.config.targets.len() {
                    return Err(service_error("Unknown target"));
                }
                self.target = Some(i);
                self.notification = format!("Selected {}", self.config.targets[i].name);
            }
            id if id.starts_with("profile:") => {
                let name = &id[8..];
                let known = match &self.manifest {
                    Some(m) => {
                        m.profiles.contains_key(name) || (m.profiles.is_empty() && name == "debug")
                    }
                    None => self.config.profiles.contains_key(name),
                };
                if !known {
                    return Err(service_error("Unknown profile"));
                }
                self.profile = name.into();
                self.select_compilers();
            }
            id if id.starts_with("task:") => {
                let task = self
                    .config
                    .tasks
                    .get(&id[5..])
                    .ok_or_else(|| service_error("Unknown task"))?;
                let plan = BuildPlan {
                    directory: self.root.clone(),
                    system: BuildSystem::Make,
                    steps: vec![ProcessSpec {
                        program: task.program.clone(),
                        args: task.args.clone(),
                    }],
                };
                self.start_plan(plan, Operation::Task)
                    .map_err(service_error)?;
            }
            _ => return Err(service_error(format!("Unknown command: {id}"))),
        }
        Ok(())
    }
    fn choices(&self, query: &str) -> Vec<(String, String)> {
        let mut choices: Vec<_> = COMMANDS
            .iter()
            .map(|(id, label)| (id.to_string(), label.to_string()))
            .collect();
        choices.extend(self.asm_labels().into_iter().map(|l| {
            (
                format!("line:{}", l.line + 1),
                format!("label: {} (line {})", l.name, l.line + 1),
            )
        }));
        choices.extend(self.config.targets.iter().enumerate().map(|(i, target)| {
            (
                format!("target:{i}"),
                format!(
                    "target: {} | chip={} | triple={}",
                    target.name,
                    target.chip.as_deref().unwrap_or("unset"),
                    target.compiler_target.as_deref().unwrap_or("host")
                ),
            )
        }));
        let profiles: Vec<String> = match &self.manifest {
            Some(m) => m.profiles.keys().cloned().collect(),
            None => self.config.profiles.keys().cloned().collect(),
        };
        choices.extend(
            profiles
                .iter()
                .map(|name| (format!("profile:{name}"), format!("profile: {name}"))),
        );
        choices.extend(
            self.config
                .tasks
                .keys()
                .map(|name| (format!("task:{name}"), format!("task: {name}"))),
        );
        choices.extend(
            self.tree
                .iter()
                .enumerate()
                .filter(|(_, row)| !row.entry.symlink)
                .map(|(i, row)| {
                    let relative = row
                        .entry
                        .path
                        .strip_prefix(&self.root)
                        .unwrap_or(&row.entry.path)
                        .display();
                    (
                        if row.entry.directory {
                            format!("tree:{i}")
                        } else {
                            format!("file:{}", row.entry.path.display())
                        },
                        format!(
                            "file: {relative}{}",
                            if row.entry.directory {
                                "/ [expand/collapse]"
                            } else {
                                ""
                            }
                        ),
                    )
                }),
        );
        let query = query.to_lowercase();
        choices.retain(|(_, name)| name.to_lowercase().contains(&query));
        choices
    }
    fn submit_input(&mut self, kind: InputKind, text: String) -> Result<(), FrontendError> {
        match kind {
            InputKind::Open => self.open_file(Path::new(&text))?,
            InputKind::NewFile | InputKind::NewDirectory => {
                let directory = matches!(kind, InputKind::NewDirectory);
                let path = explorer::create(&self.root, &text, directory).map_err(service_error)?;
                self.refresh().map_err(service_error)?;
                if let Some(index) = self.tree.iter().position(|row| row.entry.path == path) {
                    self.selected = index;
                }
                if !directory {
                    self.open_file(&path)?;
                }
            }
            InputKind::Rename(source) => {
                explorer::rename(&self.root, &source, &text).map_err(service_error)?;
                self.refresh().map_err(service_error)?;
            }
            InputKind::Search(ignore_case) => {
                if text.is_empty() {
                    return Err(service_error("Search text must not be empty"));
                }
                let mut args = vec![
                    "--json".into(),
                    "--fixed-strings".into(),
                    "--max-count".into(),
                    "100".into(),
                    "--max-columns".into(),
                    "4096".into(),
                    "--glob".into(),
                    "!target/**".into(),
                    "--glob".into(),
                    "!.embedide-build/**".into(),
                ];
                if ignore_case {
                    args.push("--ignore-case".into());
                }
                args.extend(["--".into(), text, ".".into()]);
                let program = self.config.tools.get("rg").cloned().unwrap_or("rg".into());
                if program == "rg"
                    && !core::toolchains()
                        .iter()
                        .any(|tool| tool.name == "rg" && tool.path.is_some())
                {
                    return Err(service_error(
                        "Project search requires ripgrep (rg); install it or set native.tools.rg",
                    ));
                }
                self.start_plan(
                    BuildPlan {
                        directory: self.root.clone(),
                        system: BuildSystem::Make,
                        steps: vec![ProcessSpec { program, args }],
                    },
                    Operation::Search,
                )
                .map_err(service_error)?;
                self.problems.clear();
                self.problem = 0;
            }
            InputKind::SerialSend => self
                .serial
                .as_ref()
                .ok_or_else(|| service_error("Serial monitor is disconnected"))?
                .send(&text)
                .map_err(service_error)?,
            InputKind::SerialPort => {
                if self.serial.is_some() {
                    return Err(service_error(
                        "Disconnect serial before changing its configuration",
                    ));
                }
                let (port, baud) = text
                    .rsplit_once(' ')
                    .ok_or_else(|| service_error("Enter device and baud separated by a space"))?;
                let mut config = self.config.serial.clone();
                config.port = port.into();
                config.baud = baud.parse().map_err(service_error)?;
                config.validate().map_err(service_error)?;
                self.config.serial = config;
                self.notification = "Serial settings selected for this session".into();
            }
            InputKind::SerialExport => {
                let path = explorer::destination(&self.root, &text).map_err(service_error)?;
                use std::io::Write;
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)?;
                for line in &self.serial_log.lines {
                    writeln!(file, "{line}")?;
                }
                file.sync_all()?;
                self.notification = format!("Serial log exported to {}", path.display());
            }
            InputKind::BreakDelete => self
                .debug_command(DebugCommand::Delete(text.parse().map_err(service_error)?))
                .map_err(service_error)?,
            InputKind::OutputFilter => {
                self.output_filter = text;
                self.panel_scroll = 0;
            }
        }
        Ok(())
    }
    fn confirm(&mut self, action: Confirm) -> Result<(), FrontendError> {
        match action {
            Confirm::Delete(path) => {
                explorer::delete(&self.root, &path).map_err(service_error)?;
                self.refresh().map_err(service_error)?;
            }
            Confirm::Close => self.close_document(),
            Confirm::SaveBinary => {
                if let Some(view) = &mut self.binary {
                    let n = view.file.changed();
                    view.file.save().map_err(service_error)?;
                    view.editing = false;
                    view.message =
                        format!("Wrote {n} changed bytes; untouched bytes were copied unchanged");
                    self.notification = format!("Saved {}", view.path().display());
                }
            }
            Confirm::CloseBinary => self.binary = None,
            Confirm::Quit => self.closed = true,
            Confirm::Flash(plan) => {
                // Revalidate the ELF and exact target at the moment of confirmation.
                let target = self
                    .target
                    .and_then(|i| self.config.targets.get(i))
                    .ok_or_else(|| service_error("Target is no longer selected"))?;
                let validated = hardware::flash_plan(&self.root, target, &plan.steps[0].program)
                    .map_err(service_error)?;
                if validated.steps[0].args != plan.steps[0].args {
                    return Err(service_error("Flash plan changed; preview again"));
                }
                self.start_plan(validated, Operation::Flash)
                    .map_err(service_error)?;
            }
            Confirm::Debug => {
                self.debug = Some(
                    DebugSession::start(&self.root, &self.config.debug).map_err(service_error)?,
                );
                self.debug_status = "Connecting".into();
                self.debug_location = None;
                self.show_panel(Panel::Debug);
            }
        }
        Ok(())
    }
    fn handle_dialog(&mut self, key: KeyEvent) -> Result<(), FrontendError> {
        let dialog = self.dialog.take().expect("dialog");
        if key.code == KeyCode::Esc {
            self.notification = "Cancelled".into();
            return Ok(());
        }
        match dialog {
            Dialog::Confirm { text, action } => {
                if key.code == KeyCode::Char('y') && key.modifiers.is_empty() {
                    self.confirm(action)?;
                } else if key.code == KeyCode::Char('n') {
                    self.notification = "Cancelled".into();
                } else {
                    self.dialog = Some(Dialog::Confirm { text, action });
                }
            }
            Dialog::Input {
                label,
                mut text,
                kind,
            } => {
                match key.code {
                    KeyCode::Enter => return self.submit_input(kind, text),
                    KeyCode::Backspace => {
                        text.pop();
                    }
                    KeyCode::Char(ch)
                        if !key
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                            && !ch.is_control()
                            && text.len() < 4096 =>
                    {
                        text.push(ch)
                    }
                    _ => {}
                }
                self.dialog = Some(Dialog::Input { label, text, kind });
            }
            Dialog::Palette {
                mut query,
                mut selected,
            } => {
                let choices = self.choices(&query);
                match key.code {
                    KeyCode::Enter => {
                        if let Some((id, _)) = choices.get(selected) {
                            return self.command(id);
                        }
                    }
                    KeyCode::Down => selected = (selected + 1).min(choices.len().saturating_sub(1)),
                    KeyCode::Up => selected = selected.saturating_sub(1),
                    KeyCode::Backspace => {
                        query.pop();
                        selected = 0;
                    }
                    KeyCode::Char(ch)
                        if !key
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                            && !ch.is_control()
                            && query.len() < 256 =>
                    {
                        query.push(ch);
                        selected = 0;
                    }
                    _ => {}
                }
                self.dialog = Some(Dialog::Palette { query, selected });
            }
        }
        Ok(())
    }
    fn jump_problem(&mut self) -> Result<(), FrontendError> {
        let diagnostic = self
            .problems
            .get(self.problem)
            .ok_or_else(|| service_error("No diagnostic selected"))?
            .clone();
        let path = diagnostic
            .file
            .ok_or_else(|| service_error("This diagnostic has no source location"))?;
        self.open_file(&path)?;
        let state = self.editor().state();
        let row = diagnostic
            .line
            .saturating_sub(1)
            .min(state.line_count().saturating_sub(1));
        let range = state
            .line_range(row)
            .ok_or_else(|| service_error("Invalid diagnostic line"))?;
        let text = state.buffer().slice(range.clone())?;
        let offset = text
            .char_indices()
            .nth(diagnostic.column.saturating_sub(1))
            .map_or(range.end, |(i, _)| range.start + i);
        self.documents[self.active]
            .session
            .editor
            .set_cursor(offset)?;
        Ok(())
    }
    fn shortcut(&self, key: KeyEvent) -> Option<String> {
        let digit = matches!(key.code, KeyCode::Char(ch) if ch.is_ascii_digit());
        let binding = if key.modifiers == KeyModifiers::CONTROL | KeyModifiers::ALT && digit {
            if let KeyCode::Char(ch) = key.code {
                format!("ctrl+alt+{ch}")
            } else {
                return None;
            }
        } else if key.modifiers.contains(KeyModifiers::CONTROL) {
            // Ctrl+Alt+<letter> stays Ctrl+<letter>: a legacy terminal turns Esc
            // followed quickly by Ctrl+S (one read, e.g. over SSH) into ESC ^S.
            match key.code {
                // Terminals send NUL for Ctrl+Space (kitty: CSI 32;5u).
                KeyCode::Char(' ') => "ctrl+space".into(),
                KeyCode::Char(ch) => format!("ctrl+{}", ch.to_ascii_lowercase()),
                _ => return None,
            }
        } else if key.modifiers == KeyModifiers::ALT {
            if let KeyCode::Char(ch) = key.code {
                format!("alt+{}", ch.to_ascii_lowercase())
            } else {
                return None;
            }
        } else if key.modifiers.is_empty() {
            if let KeyCode::F(number) = key.code {
                format!("f{number}")
            } else {
                return None;
            }
        } else {
            return None;
        };
        // Alt+<digit> and Ctrl+Alt+<digit> are the same panel keys: every
        // terminal can send Alt+<digit> (ESC <digit>), while Ctrl+Alt+<digit>
        // needs the kitty keyboard protocol. A ctrl+alt+N binding answers to
        // Alt+N as well.
        let legacy = binding
            .strip_prefix("alt+")
            .filter(|d| d.len() == 1 && d.as_bytes()[0].is_ascii_digit())
            .map(|d| format!("ctrl+alt+{d}"));
        let candidates = [Some(binding.as_str()), legacy.as_deref()];
        let mut candidates = candidates.iter().flatten().copied();
        for binding in candidates.clone() {
            if let Some((id, _)) = self
                .config
                .keys
                .iter()
                .find(|(_, chord)| chord.to_lowercase() == binding)
            {
                return Some(id.clone());
            }
        }
        let default = candidates.find_map(|binding| {
            Some(match binding {
                "ctrl+alt+1" => "focus_explorer",
                "ctrl+alt+2" => "focus_assistant",
                "ctrl+alt+3" => "focus_terminal",
                "ctrl+p" => "find",
                "ctrl+space" => "suggest",
                "alt+\\" => "ai_suggest",
                "ctrl+k" => "palette",
                "ctrl+s" => "save",
                "ctrl+w" => "close",
                "ctrl+q" => "quit",
                "f1" => "help",
                "f2" => "explorer",
                "f3" => "problems",
                "f4" => "panel",
                "f5" => "build",
                "f6" => "focus",
                "f7" => "check",
                "f8" => "cancel",
                "f9" => "target",
                "f10" => "next_document",
                _ => return None,
            })
        })?;
        if self.config.keys.contains_key(default) {
            None
        } else {
            Some(default.into())
        }
    }
    /// Mouse (only while `:mouse` is on): a left click focuses the panel under
    /// it, places the editor cursor or selects an explorer row (a second click
    /// on the selected row opens/expands it); the wheel scrolls the editor,
    /// explorer, AI chat, output panels and terminal history by 3 lines.
    fn mouse_event(&mut self, mouse: MouseEvent) -> Result<(), FrontendError> {
        let (x, y) = (mouse.column, mouse.row);
        let hits = self.hits;
        let inside = |area: Option<Area>| area.filter(|a| a.contains(x, y));
        let wheel = match mouse.kind {
            MouseEventKind::ScrollUp => -3isize,
            MouseEventKind::ScrollDown => 3,
            MouseEventKind::Down(MouseButton::Left) => 0,
            _ => return Ok(()),
        };
        if let Some(area) = inside(hits.tree) {
            // The first row is the EXPLORER header.
            let index = self.tree_top + usize::from(y - area.y).saturating_sub(1);
            if wheel != 0 {
                self.selected = self
                    .selected
                    .saturating_add_signed(wheel)
                    .min(self.tree.len().saturating_sub(1));
            } else if y > area.y && index < self.tree.len() {
                let again = self.focus == Focus::Explorer && index == self.selected;
                self.focus = Focus::Explorer;
                self.selected = index;
                if again {
                    self.toggle_entry()?;
                }
            } else {
                self.focus = Focus::Explorer;
            }
        } else if let Some(area) = inside(hits.editor) {
            let doc = &mut self.documents[self.active];
            let state = doc.session.editor.state();
            if wheel != 0 {
                let line = state.position()?.line.saturating_add_signed(wheel);
                self.goto_line(line + 1)?;
            } else {
                let digits = state.line_count().to_string().len();
                let width = usize::from(area.width);
                let gutter = if width > digits + 3 { digits + 2 } else { 0 };
                let column = usize::from(x - area.x).saturating_sub(gutter);
                let byte = doc
                    .viewport
                    .byte_at_cell(state, usize::from(y - area.y), column)?;
                doc.session.editor.set_cursor(byte)?;
                self.focus = Focus::Editor;
            }
        } else if let Some(_area) = inside(hits.assistant) {
            if wheel != 0 {
                self.assistant.scroll(-wheel);
            } else {
                self.assistant_visible = true;
                self.focus = Focus::Assistant;
            }
        } else if let Some(_area) = inside(hits.bottom) {
            if self.panel == Panel::Terminal {
                match &mut self.terminal {
                    Some(t) if wheel != 0 => {
                        if !t.alternate_screen() {
                            t.scroll(-wheel);
                        }
                    }
                    _ if wheel == 0 => {
                        self.ensure_terminal()?;
                        self.focus = Focus::Terminal;
                    }
                    _ => {}
                }
            } else if wheel != 0 {
                self.panel_scroll = self.panel_scroll.saturating_add_signed(-wheel);
            } else {
                self.focus = Focus::Panel;
            }
        }
        Ok(())
    }
    /// `/provider codex-cli` typed in the editor is a Vim search, not an AI
    /// command: say where AI commands go instead of only "not found".
    fn hint_ai_command_typed_in_editor(&mut self) {
        let key = self.panel_key('2');
        let message = &self.documents[self.active].session.message;
        let Some(pattern) = message
            .strip_prefix("Error: literal search not found: ")
            .map(str::to_string)
        else {
            return;
        };
        const AI: [&str; 16] = [
            "provider",
            "providers",
            "status",
            "file",
            "sel",
            "selection",
            "explain",
            "edit",
            "diff",
            "apply",
            "reject",
            "history",
            "cancel",
            "clear",
            "model",
            "auth",
        ];
        if AI.contains(&pattern.split_whitespace().next().unwrap_or("")) {
            self.documents[self.active].session.message = format!(
                "\"/{pattern}\" was a text search here. AI commands go in the AI panel: press {key}, then type /{pattern}"
            );
            self.notification = format!("AI panel: {key}; then type /{pattern} and Enter");
        }
    }
    /// Keys a visible completion popup takes: Down/Ctrl+N and Up/Ctrl+P
    /// select, Tab/Enter accept the selected item. Esc closes the popup and,
    /// as in Vim, still leaves Insert mode (so `Esc :w` always saves).
    /// Every other key (and Tab/Enter with nothing selected) goes on.
    fn popup_key(&mut self, key: KeyEvent) -> Option<Result<(), FrontendError>> {
        if key.code == KeyCode::Esc {
            self.popup = None;
            return None;
        }
        let popup = self.popup.as_mut().filter(|p| !p.is_empty())?;
        let ctrl = key.modifiers == KeyModifiers::CONTROL;
        let plain = key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT;
        match key.code {
            KeyCode::Down if plain => popup.next(),
            KeyCode::Char('n') if ctrl => popup.next(),
            KeyCode::Up if plain => popup.previous(),
            KeyCode::Char('p') if ctrl => popup.previous(),
            KeyCode::Tab | KeyCode::Enter
                if key.modifiers.is_empty() && popup.selected.is_some() =>
            {
                return Some(self.accept_completion())
            }
            _ => return None,
        }
        Some(Ok(()))
    }
    /// Replace the typed word (or the range the language server named)
    /// with the selected candidate; part of the current Insert undo step.
    fn accept_completion(&mut self) -> Result<(), FrontendError> {
        let Some(popup) = self.popup.take() else {
            return Ok(());
        };
        let Some(candidate) = popup.chosen().cloned() else {
            return Ok(());
        };
        let editor = &mut self.documents[popup.doc].session.editor;
        let state = editor.state();
        let cursor = state.cursor();
        let line = state
            .line_range(state.position()?.line)
            .unwrap_or(cursor..cursor);
        let range = candidate
            .range
            .filter(|r| line.start <= r.start && r.start <= cursor && r.end <= line.end)
            .map_or(popup.start..cursor, |r| r.start..r.end.max(cursor));
        editor.replace(range, &candidate.insert)?;
        self.after_editor_input();
        Ok(())
    }
    /// After an Insert-mode key: identifier characters open or update the
    /// popup (from `completion.min_chars` characters), the server's trigger
    /// characters (`.`, `::`, `->`) ask it for members, other keys close it.
    fn after_completion_key(&mut self, key: KeyEvent) -> Result<(), FrontendError> {
        if self.editor().input().mode() != Mode::Insert {
            self.popup = None;
            return Ok(());
        }
        let auto = self.config.completion.auto;
        let typed = !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        match key.code {
            KeyCode::Char(ch) if typed && completion::is_word_char(ch) => {
                if auto || self.popup.is_some() {
                    self.suggest(false, None)?;
                }
            }
            KeyCode::Char(ch) if typed && auto && self.is_trigger(ch) => {
                self.popup = None;
                self.suggest(false, Some(ch))?;
            }
            KeyCode::Backspace if self.popup.is_some() => self.suggest(false, None)?,
            KeyCode::Left | KeyCode::Right | KeyCode::Home | KeyCode::End | KeyCode::Char(_) => {
                self.popup = None
            }
            _ => self.validate_popup(),
        }
        Ok(())
    }
    /// Whether `ch` asks the document's language server for members.
    fn is_trigger(&self, ch: char) -> bool {
        let state = self.editor().state();
        let Some(language) = lsp_language(state.path()) else {
            return false;
        };
        let Some(server) = lsp::server_for(language, &self.config.completion) else {
            return false;
        };
        match self.lsp.get(&server_key(&server)) {
            Some(LspSlot::Running(client)) if client.is_ready() => {
                client.trigger_characters().iter().any(|t| t.ends_with(ch))
            }
            Some(_) => false,
            None => matches!(ch, '.' | ':' | '>'),
        }
    }
    /// Close the popup when its document, focus, mode or word changed.
    fn validate_popup(&mut self) {
        let Some(popup) = &self.popup else { return };
        let doc = &self.documents[self.active];
        let valid = popup.doc == self.active
            && self.focus == Focus::Editor
            && self.dialog.is_none()
            && doc.editor().input().mode() == Mode::Insert
            && completion::word_before_cursor(doc.editor().state())
                .is_ok_and(|w| w.start == popup.start);
        if !valid {
            self.popup = None;
        }
    }
    /// Open or update completion at the cursor. `manual` (Ctrl+Space) also
    /// works below `min_chars` and preselects the first item; `trigger` is
    /// a member-access character just typed.
    fn suggest(&mut self, manual: bool, trigger: Option<char>) -> Result<(), FrontendError> {
        let doc = self.active;
        let editor = self.documents[doc].editor();
        if editor.input().mode() != Mode::Insert {
            self.popup = None;
            if manual {
                self.notification = "Completion works in Insert mode (press i first)".into();
            }
            return Ok(());
        }
        let state = editor.state();
        let word = completion::word_before_cursor(state)?;
        let query = state.buffer().slice(word.clone())?;
        let same = self
            .popup
            .as_ref()
            .is_some_and(|p| p.doc == doc && p.start == word.start);
        let enough = query.chars().count() >= self.config.completion.min_chars;
        if !manual && trigger.is_none() && !enough && !same {
            self.popup = None;
            return Ok(());
        }
        let mut ask = manual;
        if same {
            let popup = self.popup.as_mut().expect("open popup");
            popup.refilter(&query);
            if manual && popup.selected.is_none() && !popup.is_empty() {
                popup.selected = Some(0);
            }
            ask |= popup.semantic && popup.incomplete && popup.request.is_none();
        } else {
            // Member completion has no word yet: only the server can answer.
            let words = if trigger.is_some() {
                Vec::new()
            } else {
                completion::document_words(state)
                    .iter()
                    .map(|w| Candidate::word(w))
                    .collect()
            };
            let (items, source) = match self.asm_dialect().filter(|_| trigger.is_none()) {
                Some(dialect) => (asm_candidates(dialect, self.asm_labels(), words), "asm"),
                None => (words, "words"),
            };
            self.popup = Some(Popup::new(doc, word.start, items, source, &query, manual));
            ask = true;
        }
        if ask {
            self.request_completion(trigger);
        }
        let popup = self.popup.as_ref().expect("open popup");
        if popup.is_empty() && popup.request.is_none() && !self.server_starting(popup) {
            self.popup = None;
            if manual {
                self.notification = "No suggestions".into();
            }
        }
        Ok(())
    }
    /// The popup's language server is still initializing (it is asked as
    /// soon as it is ready).
    fn server_starting(&self, popup: &Popup) -> bool {
        popup.server.as_ref().is_some_and(|key| {
            matches!(self.lsp.get(key), Some(LspSlot::Running(c)) if !c.is_ready() && !c.is_stopped())
        })
    }
    /// Ask the document's language server (starting it on first use).
    fn request_completion(&mut self, trigger: Option<char>) {
        let Some(popup) = self.popup.as_mut() else {
            return;
        };
        let state = self.documents[popup.doc].session.editor.state();
        let (Some(path), Some(language)) = (state.path(), lsp_language(state.path())) else {
            return;
        };
        let Some(server) = lsp::server_for(language, &self.config.completion) else {
            return;
        };
        let key = server_key(&server);
        if !self.lsp.contains_key(&key) {
            match lsp::Client::start(language, &server, &self.root) {
                Ok(client) => {
                    self.notification = format!(
                        "Starting {} for {language} completion; words from this file until it is ready",
                        client.name()
                    );
                    self.lsp.insert(key.clone(), LspSlot::Running(client));
                }
                Err(e) => {
                    self.notification = format!("{e}; completion uses words from this file");
                    self.lsp.insert(key.clone(), LspSlot::Failed);
                    return;
                }
            }
        }
        popup.server = Some(key.clone());
        if let Some(LspSlot::Running(client)) = self.lsp.get_mut(&key) {
            popup.request = client.complete(path, language, state, trigger);
        }
    }
    /// Language-server events: readiness, answers, crashes. Never blocks.
    fn poll_lsp(&mut self) -> bool {
        let mut events = Vec::new();
        for (key, slot) in &mut self.lsp {
            if let LspSlot::Running(client) = slot {
                for event in client.poll() {
                    events.push((key.clone(), client.name().to_string(), client.utf8(), event));
                }
            }
        }
        let mut changed = false;
        for (key, name, utf8, event) in events {
            let ours = self
                .popup
                .as_ref()
                .is_some_and(|p| p.server.as_deref() == Some(&key));
            match event {
                lsp::Event::Ready => {
                    if ours && self.popup.as_ref().is_some_and(|p| p.request.is_none()) {
                        self.request_completion(None);
                        changed = true;
                    }
                }
                lsp::Event::Completion {
                    request,
                    items,
                    incomplete,
                } => {
                    let Some(popup) = self.popup.as_mut() else {
                        continue;
                    };
                    if !ours || popup.request != Some(request) {
                        continue; // superseded or for a closed popup
                    }
                    popup.request = None;
                    let state = self.documents[popup.doc].session.editor.state();
                    let candidates: Vec<Candidate> = items
                        .into_iter()
                        .map(|item| Candidate {
                            range: item.range.and_then(|((sl, sc), (el, ec))| {
                                Some(
                                    lsp::offset(state, sl, sc, utf8)?
                                        ..lsp::offset(state, el, ec, utf8)?,
                                )
                            }),
                            label: item.label,
                            insert: item.insert,
                            kind: item.kind,
                            detail: item.detail,
                            filter: item.filter,
                            sort: item.sort,
                        })
                        .collect();
                    if !candidates.is_empty() {
                        popup.replace(candidates, &name, true);
                        popup.incomplete = incomplete;
                    }
                    if popup.is_empty() {
                        self.popup = None;
                    }
                    changed = true;
                }
                lsp::Event::Stopped(reason) => {
                    self.notification = format!(
                        "{reason}; completion uses words from this file (:lsp_restart retries)"
                    );
                    self.lsp.insert(key.clone(), LspSlot::Failed);
                    if let Some(popup) = self.popup.as_mut().filter(|_| ours) {
                        popup.request = None;
                        if popup.is_empty() {
                            self.popup = None;
                        }
                    }
                    changed = true;
                }
            }
        }
        changed
    }
    /// How the focus chord for panel `digit` is typed in this terminal.
    fn panel_key(&self, digit: char) -> String {
        format!("Alt+{digit}")
    }
    /// Route terminal input with dialogs before global keys, then focused content.
    pub fn handle(&mut self, event: Event) -> bool {
        let result = self.handle_inner(event);
        if let Err(error) = result {
            self.notification = format!("Error: {error}");
        }
        true
    }
    fn handle_inner(&mut self, event: Event) -> Result<(), FrontendError> {
        match event {
            Event::Resize(width, height) => self.size = (width, height),
            Event::Key(key) => {
                if let Some(result) = self.panel_chord(key) {
                    return result;
                }
                if key.kind == KeyEventKind::Release {
                    return Ok(());
                }
                if self.dialog.is_some() {
                    return self.handle_dialog(key);
                }
                if self.focus == Focus::Editor && self.popup.is_some() {
                    if let Some(result) = self.popup_key(key) {
                        return result;
                    }
                }
                let shortcut = self.shortcut(key);
                if self.focus == Focus::Terminal
                    && !shortcut
                        .as_deref()
                        .is_some_and(|id| id.starts_with("focus_") || id.starts_with("hide_"))
                {
                    // The shell owns the keyboard: Ctrl+C, Ctrl+D, Tab, Esc, F-keys.
                    if let Some(t) = &mut self.terminal {
                        if !t.alternate_screen() && scrollback_key(t, key) {
                            return Ok(());
                        }
                    }
                    match &mut self.terminal {
                        Some(t) if t.exited().is_none() => {
                            // Typing returns to the live screen, like a terminal emulator.
                            t.scroll_to_live();
                            t.key(key);
                        }
                        _ => {
                            if matches!(key.code, KeyCode::Enter) {
                                self.terminal = None;
                                self.ensure_terminal()?;
                            } else if key.code == KeyCode::Esc {
                                self.focus = Focus::Editor;
                            }
                        }
                    }
                    return Ok(());
                }
                if let Some(id) = shortcut {
                    match id.as_str() {
                        "palette" => {
                            self.dialog = Some(Dialog::Palette {
                                query: String::new(),
                                selected: 0,
                            })
                        }
                        "save" => {
                            self.documents[self.active].session.editor.save()?;
                            self.notification = "Saved".into();
                        }
                        "focus" => {
                            self.focus = match self.focus {
                                Focus::Editor => {
                                    self.explorer_visible = true;
                                    Focus::Explorer
                                }
                                Focus::Explorer => {
                                    self.panel_visible = true;
                                    Focus::Panel
                                }
                                Focus::Panel | Focus::Terminal => {
                                    if self.assistant_visible {
                                        Focus::Assistant
                                    } else {
                                        Focus::Editor
                                    }
                                }
                                Focus::Assistant => Focus::Editor,
                            }
                        }
                        "panel" => {
                            self.panel = self.panel.next();
                            self.panel_visible = true;
                            self.focus = Focus::Panel;
                            self.panel_scroll = 0;
                        }
                        "next_document" => {
                            self.active = (self.active + 1) % self.documents.len();
                            self.focus = Focus::Editor;
                        }
                        _ => self.command(&id)?,
                    }
                    return Ok(());
                }
                match self.focus {
                    Focus::Terminal => {}
                    Focus::Assistant => match key.code {
                        KeyCode::Esc => self.focus = Focus::Editor,
                        KeyCode::Enter => match self.assistant.input.trim() {
                            "/apply" => {
                                self.assistant.input.clear();
                                self.apply_proposal()?;
                            }
                            "/reject" => {
                                self.assistant.input.clear();
                                self.assistant.take_proposal_if(false);
                            }
                            _ => {
                                let ctx = self.editor_context();
                                self.assistant.submit(&ctx);
                            }
                        },
                        KeyCode::Backspace => {
                            self.assistant.input.pop();
                        }
                        KeyCode::Up => self.assistant.scroll(1),
                        KeyCode::Down => self.assistant.scroll(-1),
                        KeyCode::PageUp => self.assistant.scroll(10),
                        KeyCode::PageDown => self.assistant.scroll(-10),
                        // One key answers an agent approval: y allow, a allow similar, n deny
                        KeyCode::Char(ch @ ('y' | 'a' | 'n'))
                            if self.assistant.approval_pending()
                                && self.assistant.input.is_empty()
                                && !key
                                    .modifiers
                                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                        {
                            self.assistant.answer(match ch {
                                'y' => embedide_ai::agent::Decision::Allow,
                                'a' => embedide_ai::agent::Decision::AllowSimilar,
                                _ => embedide_ai::agent::Decision::Deny(String::new()),
                            });
                        }
                        KeyCode::Char(ch)
                            if !key
                                .modifiers
                                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                                && !ch.is_control()
                                && self.assistant.input.len() < 16384 =>
                        {
                            self.assistant.input.push(ch)
                        }
                        _ => {}
                    },
                    Focus::Editor if self.binary.is_some() => {
                        if let Some(result) = self.binary_key(key) {
                            return result;
                        }
                    }
                    Focus::Editor => {
                        let doc = &self.documents[self.active];
                        let cursor = doc.editor().state().cursor();
                        let path = doc.editor().state().path().map(Path::to_path_buf);
                        let pending = self
                            .assistant
                            .suggestion
                            .as_ref()
                            .is_some_and(|s| s.offset == cursor && s.path == path);
                        if pending && key.code == KeyCode::Tab && key.modifiers.is_empty() {
                            let text = self.assistant.suggestion.take().expect("suggestion").text;
                            let editor = &mut self.documents[self.active].session.editor;
                            editor.replace(cursor..cursor, &text)?;
                            editor.set_cursor(cursor + text.len())?;
                            self.notification = "Suggestion inserted; u undoes".into();
                            self.edit_mark = (
                                self.active,
                                self.editor().state().buffer().revision(),
                                cursor + text.len(),
                            );
                            return Ok(());
                        }
                        if pending && key.code == KeyCode::Esc {
                            self.assistant.dismiss();
                            self.notification = "Suggestion rejected".into();
                        } else if key.code == KeyCode::Esc
                            && self.assistant.inline_status().is_some()
                        {
                            self.assistant.dismiss();
                            self.notification = "AI suggestion cancelled".into();
                        }
                        self.documents[self.active].handle(Event::Key(key));
                        if let Some(command) = self.documents[self.active].take_host_command() {
                            self.host_command(&command)?;
                        }
                        self.after_editor_input();
                        self.after_completion_key(key)?;
                        self.hint_ai_command_typed_in_editor();
                        if let Some(action) = self.documents[self.active].take_ide_action() {
                            self.command(match action {
                                IdeAction::ProjectExplorer => "explorer",
                                IdeAction::Build => "build",
                                IdeAction::Debug => "debug",
                                IdeAction::Terminal => "focus_terminal",
                                IdeAction::SerialMonitor => "serial",
                                IdeAction::FindFile => "find",
                            })?;
                        }
                        if self.editor().state().is_closed() {
                            self.close_document();
                        }
                    }
                    Focus::Explorer => match key.code {
                        KeyCode::Down | KeyCode::Char('j') => {
                            self.selected =
                                (self.selected + 1).min(self.tree.len().saturating_sub(1))
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            self.selected = self.selected.saturating_sub(1)
                        }
                        KeyCode::Enter | KeyCode::Right => self.toggle_entry()?,
                        KeyCode::Left => {
                            if self.tree.get(self.selected).is_some_and(|r| r.expanded) {
                                self.toggle_entry()?;
                            } else if let Some(row) = self.tree.get(self.selected) {
                                if let Some(i) = self.tree[..self.selected]
                                    .iter()
                                    .rposition(|r| r.depth < row.depth)
                                {
                                    self.selected = i;
                                }
                            }
                        }
                        KeyCode::Char('r') => self.command("refresh")?,
                        KeyCode::Char('n') => self.command("new_file")?,
                        // Only a plain Delete deletes: a mistimed Ctrl+Alt+<digit>+Delete
                        // hide chord must never open the delete prompt.
                        KeyCode::Delete if key.modifiers.is_empty() => self.command("delete")?,
                        KeyCode::Char('/') => self.command("find")?,
                        KeyCode::Esc => self.focus = Focus::Editor,
                        _ => {}
                    },
                    Focus::Panel => match key.code {
                        KeyCode::Esc => self.focus = Focus::Editor,
                        KeyCode::Tab => {
                            self.panel = self.panel.next();
                            self.panel_scroll = 0;
                        }
                        KeyCode::Up | KeyCode::Char('k') if self.panel == Panel::Problems => {
                            self.problem = self.problem.saturating_sub(1)
                        }
                        KeyCode::Down | KeyCode::Char('j') if self.panel == Panel::Problems => {
                            self.problem =
                                (self.problem + 1).min(self.problems.len().saturating_sub(1))
                        }
                        KeyCode::Enter if self.panel == Panel::Problems => self.jump_problem()?,
                        KeyCode::Up | KeyCode::Char('k') => {
                            self.panel_scroll = self.panel_scroll.saturating_add(1)
                        }
                        KeyCode::PageUp => self.panel_scroll = self.panel_scroll.saturating_add(10),
                        KeyCode::Down | KeyCode::Char('j') => {
                            self.panel_scroll = self.panel_scroll.saturating_sub(1)
                        }
                        KeyCode::PageDown => {
                            self.panel_scroll = self.panel_scroll.saturating_sub(10)
                        }
                        KeyCode::End => self.panel_scroll = 0,
                        KeyCode::Char('/') => {
                            self.input("Filter output (empty clears)", InputKind::OutputFilter)
                        }
                        _ => {}
                    },
                }
            }
            Event::Mouse(mouse) if self.mouse && self.dialog.is_none() => {
                return self.mouse_event(mouse)
            }
            Event::Paste(text) => {
                self.popup = None;
                if let Some(Dialog::Input { text: input, .. }) = &mut self.dialog {
                    for ch in text.chars().filter(|c| !c.is_control()) {
                        if input.len() >= 4096 {
                            break;
                        }
                        input.push(ch);
                    }
                } else if self.dialog.is_none() && self.focus == Focus::Editor {
                    self.documents[self.active].handle(Event::Paste(text));
                    self.after_editor_input();
                } else if self.dialog.is_none() && self.focus == Focus::Terminal {
                    if let Some(t) = &mut self.terminal {
                        t.scroll_to_live();
                        t.paste(&text);
                    }
                } else if self.dialog.is_none() && self.focus == Focus::Assistant {
                    let room = 16384usize.saturating_sub(self.assistant.input.len());
                    self.assistant.input.extend(
                        text.chars()
                            .map(|c| if c == '\n' { ' ' } else { c })
                            .filter(|c| !c.is_control())
                            .take(room),
                    );
                }
            }
            _ => {}
        }
        Ok(())
    }
    /// Reload open buffers whose files an agent changed (or reverted). A
    /// buffer with unsaved edits is never replaced; the user is told instead.
    fn reload_changed(&mut self, files: &[PathBuf]) -> Option<String> {
        let wanted: Vec<PathBuf> = files
            .iter()
            .map(|f| f.canonicalize().unwrap_or_else(|_| f.clone()))
            .collect();
        let mut reloaded = 0;
        let mut kept = Vec::new();
        for index in 0..self.documents.len() {
            let Some(path) = self.documents[index]
                .editor()
                .state()
                .path()
                .map(Path::to_path_buf)
            else {
                continue;
            };
            if !wanted.contains(&path) && !files.contains(&path) {
                continue;
            }
            if self.documents[index].editor().state().is_modified() {
                kept.push(path.display().to_string());
                continue;
            }
            if !path.exists() {
                kept.push(format!("{} (deleted)", path.display()));
                continue;
            }
            match crate::open(&path) {
                Ok(editor) => {
                    let doc = self.new_document(editor);
                    self.documents[index] = doc;
                    reloaded += 1;
                }
                Err(e) => kept.push(format!("{} ({e})", path.display())),
            }
        }
        if !kept.is_empty() {
            Some(format!(
                "Changed on disk by the AI agent, not reloaded: {}",
                kept.join(", ")
            ))
        } else if reloaded > 0 {
            Some(format!("Reloaded {reloaded} agent-changed file(s)"))
        } else {
            None
        }
    }
    /// Keep inline suggestions consistent with the cursor and request new ones
    /// while typing (only with `ai.inline = true`).
    fn after_editor_input(&mut self) {
        let state = self.editor().state();
        let mark = (self.active, state.buffer().revision(), state.cursor());
        if mark == self.edit_mark {
            return;
        }
        let edited = mark.0 == self.edit_mark.0 && mark.1 != self.edit_mark.1;
        self.edit_mark = mark;
        if edited && self.assistant.suggestion.is_some() {
            // Typing the start of the suggestion keeps the rest on screen.
            let state = self.editor().state();
            let text = state.buffer().contents();
            let (path, cursor) = (state.path().map(Path::to_path_buf), state.cursor());
            if self.assistant.type_through(path.as_deref(), &text, cursor) {
                return;
            }
        }
        if self.assistant.suggestion.is_some() || self.assistant.inline_status().is_some() {
            self.assistant.dismiss();
        }
        if edited && self.assistant.inline_enabled() && self.editor().input().mode() == Mode::Insert
        {
            let ctx = self.editor_context();
            self.assistant.on_edit(&ctx);
        }
    }
    fn consume_line(&mut self, line: &str, kind: Operation) {
        if kind == Operation::Search {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
                if value["type"] == "match" && self.problems.len() < 1000 {
                    let data = &value["data"];
                    if let (Some(path), Some(row), Some(text)) = (
                        data["path"]["text"].as_str(),
                        data["line_number"].as_u64(),
                        data["lines"]["text"].as_str(),
                    ) {
                        self.problems.push(Diagnostic {
                            severity: "match".into(),
                            message: text.trim_end().chars().take(512).collect(),
                            file: Some(path.into()),
                            line: row as usize,
                            column: data["submatches"][0]["start"].as_u64().map_or(1, |byte| {
                                text[..(byte as usize).min(text.len())].chars().count() + 1
                            }),
                            code: None,
                            source: "rg".into(),
                        });
                    }
                }
            }
            return;
        }
        if let Some(diagnostic) = core::diagnostics::parse(line) {
            if self.problems.len() < 1000 {
                self.problems.push(diagnostic);
            }
        }
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
            if value["reason"] == "compiler-message" {
                if let Some(rendered) = value["message"]["rendered"].as_str() {
                    self.build_log.chunk(rendered);
                }
            } else if value["reason"] == "compiler-artifact" {
                if let Some(executable) = value["executable"].as_str() {
                    self.build_log.push(&format!("Artifact: {executable}"));
                }
            }
        }
    }
    /// Poll process/serial/debug queues without blocking the terminal input loop.
    /// Returns true only when presentation state changed.
    pub fn tick(&mut self) -> bool {
        let mut changed = self.assistant.tick();
        if self.focus != Focus::Editor {
            // No suggestion requests for an editor the user is not typing in.
            self.assistant.pause_requests();
        }
        let attention = self.assistant.take_attention();
        let files = self.assistant.take_changed_files();
        let reloaded = if files.is_empty() {
            None
        } else {
            changed = true;
            self.reload_changed(&files)
        };
        match (attention, reloaded) {
            (Some(message), _) if self.assistant.approval_pending() => {
                // An approval must be visible wherever the user is working.
                self.assistant_visible = true;
                self.notification = message;
                changed = true;
            }
            (Some(message), Some(r)) => {
                self.notification = format!("{r} · {message}");
                changed = true;
            }
            (Some(message), None) | (None, Some(message)) => {
                self.notification = message;
                changed = true;
            }
            (None, None) => {}
        }
        changed |= self.poll_lsp();
        if let Some(view) = &mut self.binary {
            changed |= view.tick();
        }
        if let Some(t) = &mut self.terminal {
            changed |= t.poll();
        }
        if let Some(mut running) = self.operation.take() {
            let mut finished = false;
            for event in running.task.drain() {
                changed = true;
                match event {
                    ProcessEvent::Command(command) => {
                        let log = if matches!(running.kind, Operation::Build | Operation::Flash) {
                            &mut self.build_log
                        } else {
                            &mut self.task_log
                        };
                        log.push(&format!("$ {command}"));
                    }
                    ProcessEvent::Output(text, stderr) => {
                        let log = if matches!(running.kind, Operation::Build | Operation::Flash) {
                            &mut self.build_log
                        } else {
                            &mut self.task_log
                        };
                        // Cargo JSON is rendered separately, stderr and generic output remain raw.
                        if !text.starts_with('{') || stderr {
                            log.chunk(&text);
                        }
                        let partial = &mut running.partial[usize::from(stderr)];
                        partial.push_str(&text);
                        while let Some(end) = partial.find('\n') {
                            let line: String = partial.drain(..=end).collect();
                            self.consume_line(line.trim_end(), running.kind);
                        }
                        if partial.len() > 65536 {
                            partial.clear();
                            self.notification =
                                "Output record exceeded 64 KiB; parsing skipped".into();
                        }
                    }
                    ProcessEvent::Finished(code, elapsed) => {
                        for partial in &running.partial {
                            if !partial.is_empty() {
                                self.consume_line(partial, running.kind);
                            }
                        }
                        let omitted = running.task.dropped_bytes();
                        let message = format!(
                            "Finished: exit={code:?}, {:.2}s, omitted {omitted} bytes",
                            elapsed.as_secs_f64()
                        );
                        if running.kind == Operation::Build {
                            if let Some(prepared) = self.prepared.take() {
                                if code == Some(0) {
                                    prepared.commit();
                                    if let Some(a) =
                                        prepared.artifact.as_ref().filter(|a| a.exists())
                                    {
                                        self.build_log.push(&format!("Artifact: {}", a.display()));
                                    }
                                }
                            }
                        }
                        if matches!(running.kind, Operation::Build | Operation::Flash) {
                            self.build_log.push(&message);
                            self.build_status = if code == Some(0) {
                                "Succeeded"
                            } else {
                                "Failed / cancelled"
                            }
                            .into();
                        } else {
                            self.task_log.push(&message);
                        }
                        self.notification = if running.kind == Operation::Flash && code == Some(0) {
                            "Flasher exited successfully with verification requested; firmware behavior is not verified".into()
                        } else {
                            message
                        };
                        finished = true;
                    }
                }
            }
            if !finished {
                self.operation = Some(running);
            }
        }
        if let Some(monitor) = &self.serial {
            let events = monitor.drain();
            changed |= !events.is_empty();
            for event in events {
                match event {
                    SerialEvent::Data(bytes) => {
                        if self.serial_timestamps {
                            self.serial_log.push(&format!(
                                "[{:.3}s] {}",
                                self.serial_started.elapsed().as_secs_f64(),
                                String::from_utf8_lossy(&bytes)
                            ));
                        } else {
                            self.serial_log.chunk(&String::from_utf8_lossy(&bytes));
                        }
                    }
                    SerialEvent::Error(error) => {
                        self.serial_log.push(&error);
                        self.notification = error;
                    }
                }
            }
            if monitor.is_finished() {
                self.serial = None;
                changed = true;
            }
        }
        if let Some(session) = &self.debug {
            let events = session.drain();
            let ended = session.is_finished();
            changed |= !events.is_empty() || ended;
            let mut jump = None;
            for event in events {
                match event {
                    DebugEvent::Output(text) => self.debug_log.chunk(&text),
                    DebugEvent::Connected => {
                        self.debug_status = "Connected".into();
                        self.debug_log
                            .push("GDB acknowledged symbol loading and remote connection");
                    }
                    DebugEvent::Error(error) => {
                        self.debug_status = "Error".into();
                        self.notification = error.clone();
                        self.debug_log.push(&error);
                    }
                    DebugEvent::Stopped => {
                        if self.debug_status != "Error" {
                            self.debug_status = "Disconnected".into();
                        }
                    }
                    DebugEvent::Record(record) => {
                        if record.class == "running" {
                            self.debug_status = "Running".into();
                        }
                        if record.class == "stopped" {
                            self.debug_status = format!(
                                "Stopped: {}",
                                record.data["reason"].as_str().unwrap_or("unknown")
                            );
                            let frame = &record.data["frame"];
                            if let (Some(path), Some(line)) = (
                                frame["fullname"]
                                    .as_str()
                                    .or_else(|| frame["file"].as_str()),
                                frame["line"].as_str().and_then(|s| s.parse::<usize>().ok()),
                            ) {
                                jump = Some((PathBuf::from(path), line));
                            }
                        }
                        if !record.data["bkpt"].is_null() {
                            self.debug_breakpoints = record.data["bkpt"].to_string();
                        }
                        if !record.data["stack"].is_null() {
                            self.debug_stack = record.data["stack"].to_string();
                        }
                        if !record.data["register-values"].is_null() {
                            self.debug_registers = record.data["register-values"].to_string();
                        }
                        if record.class == "error" {
                            self.notification = format!("GDB: {}", record.data["msg"]);
                        }
                    }
                }
            }
            if ended {
                self.debug = None;
                if self.debug_status != "Error" {
                    self.debug_status = "Disconnected".into();
                }
            }
            if let Some((path, line)) = jump {
                self.debug_location = Some((path.clone(), line));
                if let Err(error) = self.open_file(&path).and_then(|_| {
                    let state = self.editor().state();
                    let row = line
                        .saturating_sub(1)
                        .min(state.line_count().saturating_sub(1));
                    let offset = state.line_range(row).map_or(0, |range| range.start);
                    self.documents[self.active]
                        .session
                        .editor
                        .set_cursor(offset)
                        .map_err(FrontendError::from)
                }) {
                    self.notification = format!("Debug source unavailable: {error}");
                }
            }
        }
        if let Some(rx) = &self.inventory {
            if let Ok(lines) = rx.try_recv() {
                self.tools_log.push("SERIAL DEVICES (actual discovery)");
                if lines.is_empty() {
                    self.tools_log.push("No serial devices discovered");
                }
                for line in lines {
                    self.tools_log.push(&line);
                }
                self.inventory = None;
                self.notification = "Serial discovery complete; select a port explicitly".into();
                changed = true;
            }
        }
        changed
    }
    fn panel_lines(&self) -> Vec<String> {
        match self.panel {
            Panel::Problems => {
                if self.problems.is_empty() {
                    return vec![
                        "No diagnostics / matches. Enter jumps to selected source location.".into(),
                    ];
                }
                self.problems
                    .iter()
                    .enumerate()
                    .map(|(i, d)| {
                        format!(
                            "{} {} {}:{}:{} {}",
                            if i == self.problem { ">" } else { " " },
                            d.severity,
                            d.file
                                .as_ref()
                                .map_or(String::new(), |p| p.display().to_string()),
                            d.line,
                            d.column,
                            d.message
                        )
                    })
                    .collect()
            }
            Panel::Build => self.build_log.lines.iter().cloned().collect(),
            Panel::Tasks => self.task_log.lines.iter().cloned().collect(),
            Panel::Serial => {
                let mut lines = vec![format!(
                    "{} {} baud={} {} dropped={} bytes",
                    if self.serial.is_some() {
                        "Connected"
                    } else {
                        "Disconnected"
                    },
                    self.config.serial.port,
                    self.config.serial.baud,
                    if self.serial_paused {
                        "[PAUSED: receive buffer continues, bounded]"
                    } else {
                        ""
                    },
                    self.serial.as_ref().map_or(0, SerialMonitor::dropped_bytes)
                )];
                lines.extend(if self.serial_paused {
                    self.serial_snapshot.clone()
                } else {
                    self.serial_log.lines.iter().cloned().collect()
                });
                lines
            }
            Panel::Debug => {
                let mut lines = vec![
                    format!("{} | location={:?}", self.debug_status, self.debug_location),
                    format!("Breakpoint response: {}", self.debug_breakpoints),
                    format!("Stack: {}", self.debug_stack),
                    format!("Registers: {}", self.debug_registers),
                ];
                lines.extend(self.debug_log.lines.iter().cloned());
                lines
            }
            Panel::Tools => self.tools_log.lines.iter().cloned().collect(),
            Panel::Settings => {
                let user_path =
                    UserConfig::path().map_or_else(|e| e.0, |p| p.display().to_string());
                let project = self.root.join("embedide.toml");
                let mut lines = vec![
                    "Compilers are chosen in configuration files, never installed or downloaded automatically:".into(),
                    format!("  project: {} {}", project.display(), if self.manifest.is_some() { "([toolchain], [profiles.<name>.toolchain])" } else { "(absent: legacy embedide.json adapters are used)" }),
                    format!("  user:    {user_path} ([toolchains] search_dirs, custom, defaults; [ai])"),
                    "  legacy:  embedide.json / embedide.local.json (theme, keys, targets, serial, debug)".into(),
                    format!("Profile: {} | target: {:?} | theme: {} | tab width: {}", self.profile, self.target, self.config.theme, self.config.tab_width),
                ];
                for c in &self.compilers {
                    lines.push(format!(
                        "  [{}] {} → {}",
                        c.language.id(),
                        c.id,
                        c.program.display()
                    ));
                }
                lines.push(format!(
                    "AI: {} | inline suggestions: {} (ai.completion.mode; Alt+\\ asks)",
                    self.assistant.status(),
                    self.assistant.inline_mode()
                ));
                lines.push(format!(
                    "Serial: {} @ {} | receive_only={}",
                    self.config.serial.port,
                    self.config.serial.baud,
                    self.config.serial.receive_only
                ));
                lines.extend(self.config_notes.iter().map(|n| format!("! {n}")));
                lines.push("Commands (Ctrl+K or :name): edit_project_config, edit_user_config, reload, compilers, profile, target, theme".into());
                lines
            }
            Panel::Help => help_lines(&self.config.keys, self.keyboard_enhanced),
            Panel::Terminal => self.terminal.as_ref().map_or_else(
                || vec![format!("{} starts your shell here", self.panel_key('3'))],
                |t| t.contents().lines().map(str::to_string).collect(),
            ),
        }
    }
    fn draw_terminal(&mut self, width: usize, height: usize) -> (Vec<Row>, Option<(u16, u16)>) {
        let focused = self.focus == Focus::Terminal;
        let key = self.panel_key('3');
        let (title, mut body, cursor) = match &mut self.terminal {
            Some(t) => {
                let mut state = match t.exited() {
                    Some(why) => format!("shell ended ({why}); Enter restarts"),
                    None => t.shell().to_string(),
                };
                if t.scrolled() > 0 {
                    let history = t.history();
                    state.push_str(&format!(
                        " | HISTORY {}/{history} lines up, Shift+End live",
                        t.scrolled()
                    ));
                }
                let (rows, cursor) = t.rows(width, height.saturating_sub(1));
                (state, rows, cursor)
            }
            None => ("not started".into(), Vec::new(), None),
        };
        let mut rows = vec![layout::label(
            &format!(
                "{}TERMINAL | {title} | {}",
                if focused { "> " } else { "" },
                if focused {
                    format!("keys go to the shell; {key} back to editor")
                } else {
                    format!("{key} focus")
                }
            ),
            width,
            true,
        )];
        rows.append(&mut body);
        rows.resize_with(height, Row::default);
        (rows, cursor.filter(|_| focused).map(|(x, y)| (x, y + 1)))
    }
    fn draw_panel(&self, width: usize, height: usize) -> Vec<Row> {
        if height == 0 {
            return Vec::new();
        }
        let mut rows = vec![layout::label(
            &format!(
                "{}{} | F4/Tab switch | / filter | {}",
                if self.focus == Focus::Panel { "> " } else { "" },
                self.panel.name(),
                self.output_filter
            ),
            width,
            true,
        )];
        let mut lines = self.panel_lines();
        if !self.output_filter.is_empty() {
            lines.retain(|line| {
                line.to_lowercase()
                    .contains(&self.output_filter.to_lowercase())
            });
        }
        let count = height.saturating_sub(1);
        let start = if self.panel == Panel::Problems {
            self.problem
                .saturating_sub(count.saturating_sub(1))
                .min(lines.len().saturating_sub(count))
        } else if matches!(self.panel, Panel::Help | Panel::Settings | Panel::Tools) {
            self.panel_scroll.min(lines.len().saturating_sub(count))
        } else {
            lines
                .len()
                .saturating_sub(count)
                .saturating_sub(self.panel_scroll)
        };
        for line in lines.iter().skip(start).take(count) {
            rows.push(layout::label(line, width, false));
        }
        rows.resize_with(height, Row::default);
        rows
    }
    fn draw_tree(&mut self, width: usize, height: usize) -> Vec<Row> {
        let mut rows = Vec::with_capacity(height);
        if height == 0 {
            return rows;
        }
        rows.push(layout::label(
            if self.focus == Focus::Explorer {
                "> EXPLORER"
            } else {
                "EXPLORER"
            },
            width,
            true,
        ));
        let count = height.saturating_sub(1);
        if self.selected < self.tree_top {
            self.tree_top = self.selected;
        }
        if self.selected >= self.tree_top + count {
            self.tree_top = self.selected.saturating_sub(count.saturating_sub(1));
        }
        for (i, row) in self.tree.iter().enumerate().skip(self.tree_top).take(count) {
            let name = row
                .entry
                .path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();
            let text = format!(
                "{}{} {}{}",
                "  ".repeat(row.depth.min(32)),
                if row.entry.directory {
                    if row.expanded {
                        "v"
                    } else {
                        ">"
                    }
                } else {
                    " "
                },
                name,
                if row.entry.symlink { " [link]" } else { "" }
            );
            rows.push(layout::label(&text, width, i == self.selected));
        }
        rows.resize_with(height, Row::default);
        rows
    }
    /// Mode | file | language | compiler | position | focus | message.
    fn status_line(&self) -> String {
        if let (Some(view), Focus::Editor) = (&self.binary, self.focus) {
            return format!("{} | Ctrl+K commands", view.status());
        }
        let state = self.editor().state();
        let mode = match self.editor().input().mode() {
            Mode::Normal => "NORMAL",
            Mode::Insert => "INSERT",
            Mode::Visual => "VISUAL",
            Mode::Command => "COMMAND",
        };
        let file = state.path().map_or_else(
            || "[untitled]".to_string(),
            |p| {
                p.strip_prefix(&self.root)
                    .unwrap_or(p)
                    .display()
                    .to_string()
            },
        );
        let language = language_of(state.path());
        let position = state.position().map_or(String::new(), |p| {
            format!("Ln {}, Col {}", p.line + 1, p.scalar_column + 1)
        });
        let hint = if self.operation.is_some() {
            "F8 cancels"
        } else if self.assistant.suggestion.is_some() {
            "Tab accepts, Esc rejects"
        } else if self.assistant.inline_status().is_some() {
            "AI suggestion… Esc cancels"
        } else {
            "Ctrl+K commands"
        };
        let place = match self.focus {
            Focus::Editor => mode.to_string(),
            other => format!("{} · {mode}", format!("{other:?}").to_uppercase()),
        };
        let info = format!(
            "{file}{} | {language} | {} | {position}",
            if state.is_modified() { " [+]" } else { "" },
            self.compiler_label(language),
        );
        // Errors must stay readable on an 80-column terminal.
        if self.notification.starts_with("Error") {
            format!("{place} | {} | {info} | {hint}", self.notification)
        } else {
            format!("{place} | {info} | {hint} | {}", self.notification)
        }
    }
    /// Show a pending AI completion at the cursor without touching the buffer:
    /// the first line at the cursor, further lines as virtual rows below it
    /// (the code underneath moves down while the suggestion is shown).
    fn ghost_text(&self, frame: &mut Frame, width: usize) {
        let Some(s) = &self.assistant.suggestion else {
            return;
        };
        let state = self.editor().state();
        if s.offset != state.cursor() || s.path.as_deref() != state.path() {
            return;
        }
        let Some((x, y)) = frame.cursor else { return };
        let y = usize::from(y);
        let tab = " ".repeat(self.config.tab_width.max(1));
        let mut lines = s.text.split('\n');
        let first = lines.next().unwrap_or("");
        let rest: Vec<&str> = lines.collect();
        // Text rows end before the editor's two status rows.
        let body = frame.rows.len().saturating_sub(2);
        let Some(row) = frame.rows.get(y).filter(|_| y < body) else {
            return;
        };
        let gutter = gutter_width(row);
        let shown = rest.len().min(body.saturating_sub(y + 1)).min(16);
        let hidden = rest.len() - shown;
        let mut ghost = first.replace('\t', &tab);
        if hidden > 0 && shown == 0 {
            ghost.push_str(&format!(" … (+{hidden} lines)"));
        }
        frame.rows[y] = insert_at(&frame.rows[y], usize::from(x), &sanitize(&ghost), width);
        if shown == 0 {
            return;
        }
        let mut virtual_rows = Vec::with_capacity(shown);
        for (i, line) in rest[..shown].iter().enumerate() {
            let mut text = line.replace('\t', &tab);
            if i + 1 == shown && hidden > 0 {
                text.push_str(&format!(" … (+{hidden} lines)"));
            }
            let blank = Row::default();
            let mut row = insert_at(&blank, gutter, &sanitize(&text), width);
            if text.is_empty() {
                row = insert_at(&blank, gutter, " ", width);
            }
            virtual_rows.push(row);
        }
        let at = y + 1;
        frame.rows.splice(at..at, virtual_rows);
        // Keep the frame height: drop the text rows pushed below the view.
        frame.rows.drain(body..body + shown);
    }
    /// Compose an adaptive bounded frame, reusing the editor's cached grapheme layout.
    pub fn frame(&mut self) -> Result<Frame, FrontendError> {
        let width = usize::from(self.size.0.min(1024)).saturating_sub(1);
        let height = usize::from(self.size.1.min(256));
        self.hits = Hits::default();
        self.validate_popup();
        if height < 6 || width < 12 {
            self.documents[self.active].size = self.size;
            let mut frame = self.documents[self.active].frame()?;
            if let Some(dialog) = &self.dialog {
                if let Some(row) = frame.rows.last_mut() {
                    *row = layout::label(
                        &format!(
                            "{} (Esc cancels)",
                            match dialog {
                                Dialog::Confirm { text, .. } => text,
                                Dialog::Input { label, .. } => label,
                                Dialog::Palette { query, .. } => query,
                            }
                        ),
                        width,
                        true,
                    );
                }
                frame.cursor = None;
            }
            return Ok(frame);
        }
        let project = self.root.file_name().unwrap_or_default().to_string_lossy();
        let target = self
            .target
            .and_then(|i| self.config.targets.get(i))
            .map(|t| t.name.as_str())
            .unwrap_or("host / unconfigured");
        let mut rows = vec![layout::label(
            &format!(
                "EmbedIDE {} | {} | {project} | {target} | {}",
                env!("CARGO_PKG_VERSION"),
                self.build_status,
                self.profile
            ),
            width,
            true,
        )];
        let tabs: String = self
            .documents
            .iter()
            .enumerate()
            .cycle()
            .skip(self.active)
            .take(self.documents.len())
            .map(|(i, doc)| {
                format!(
                    "{}{}{}  ",
                    if i == self.active { ">" } else { " " },
                    doc.editor()
                        .state()
                        .path()
                        .and_then(Path::file_name)
                        .map_or("[untitled]".into(), |name| name.to_string_lossy()),
                    if doc.editor().state().is_modified() {
                        "*"
                    } else {
                        ""
                    }
                )
            })
            .collect();
        rows.push(layout::label(&tabs, width, false));
        let area = height - 3;
        let bottom_focus = matches!(self.focus, Focus::Panel | Focus::Terminal);
        let panel_height = if self.panel_visible && bottom_focus && (height < 16 || width < 60) {
            area
        } else if self.panel_visible && height >= 16 {
            if self.focus == Focus::Terminal {
                (area / 2).clamp(6, 24)
            } else {
                (area / 3).clamp(4, 10)
            }
        } else {
            0
        };
        let editor_height = area - panel_height;
        let side_width = if self.explorer_visible && width >= 80 {
            24.min(width / 3)
        } else {
            0
        };
        let ai_width = if self.assistant_visible && width >= 100 {
            (width / 3).clamp(30, 64)
        } else {
            0
        };
        let overlay_tree =
            self.explorer_visible && self.focus == Focus::Explorer && side_width == 0;
        let overlay_ai = self.focus == Focus::Assistant && ai_width == 0;
        let mut cursor = None;
        self.hits.bottom = Area::new(0, 2 + editor_height, width, panel_height);
        if editor_height > 0 {
            let left = if side_width > 0 { side_width + 1 } else { 0 };
            let right = if ai_width > 0 { ai_width + 1 } else { 0 };
            let editor_width = width.saturating_sub(left + right);
            self.hits = match (overlay_tree, overlay_ai) {
                (true, _) => Hits {
                    tree: Area::new(0, 2, width, editor_height),
                    ..self.hits
                },
                (_, true) => Hits {
                    assistant: Area::new(0, 2, width, editor_height),
                    ..self.hits
                },
                _ => Hits {
                    tree: Area::new(0, 2, side_width, editor_height),
                    editor: Area::new(left, 2, editor_width, editor_height),
                    assistant: Area::new(left + editor_width + 1, 2, ai_width, editor_height),
                    ..self.hits
                },
            };
            if overlay_tree {
                rows.extend(self.draw_tree(width, editor_height));
            } else if overlay_ai {
                let (ai, ai_cursor) = self.assistant.rows(width, editor_height, true);
                cursor = ai_cursor.map(|(x, y)| (x, y + 2));
                rows.extend(ai);
            } else {
                self.documents[self.active].size =
                    ((editor_width + 1) as u16, editor_height as u16);
                let mut frame = self.documents[self.active].frame()?;
                self.ghost_text(&mut frame, editor_width);
                if let Some(view) = &mut self.binary {
                    frame.rows = view.rows(editor_width, editor_height);
                    frame.cursor = None;
                }
                cursor = if self.focus == Focus::Editor {
                    frame.cursor.map(|(x, y)| (x + left as u16, y + 2))
                } else {
                    None
                };
                let tree = if side_width > 0 {
                    self.draw_tree(side_width, editor_height)
                } else {
                    Vec::new()
                };
                let (ai, ai_cursor) = if ai_width > 0 {
                    self.assistant
                        .rows(ai_width, editor_height, self.focus == Focus::Assistant)
                } else {
                    (Vec::new(), None)
                };
                if let Some((x, y)) = ai_cursor {
                    cursor = Some((x + (left + editor_width + 1) as u16, y + 2));
                }
                let mut tree = tree.into_iter();
                let mut ai = ai.into_iter();
                for editor_row in frame.rows.into_iter().take(editor_height) {
                    let mut row = Row::default();
                    if side_width > 0 {
                        let mut t = tree.next().unwrap_or_default();
                        pad(&mut t, side_width);
                        row.runs.extend(t.runs);
                        row.push("|", false);
                    }
                    let mut e = editor_row;
                    if ai_width > 0 {
                        pad(&mut e, editor_width);
                    }
                    row.runs.extend(e.runs);
                    if ai_width > 0 {
                        row.push("|", false);
                        row.runs.extend(ai.next().unwrap_or_default().runs);
                    }
                    rows.push(row);
                }
            }
        }
        rows.resize_with(2 + editor_height, Row::default);
        if let (Some(popup), Some(area), Some((x, y))) = (&self.popup, self.hits.editor, cursor) {
            draw_popup(popup, area, (x, y), &mut rows);
        }
        if panel_height > 0 && self.panel == Panel::Terminal {
            let (term_rows, term_cursor) = self.draw_terminal(width, panel_height);
            if let Some((x, y)) = term_cursor {
                cursor = Some((x, y + (2 + editor_height) as u16));
            }
            rows.extend(term_rows);
        } else {
            rows.extend(self.draw_panel(width, panel_height));
        }
        rows.push(layout::label(&self.status_line(), width, true));
        if let Some(dialog) = &self.dialog {
            cursor = None;
            let mut overlay = Vec::new();
            match dialog {
                Dialog::Palette { query, selected } => {
                    overlay.push(layout::label(
                        &format!("COMMANDS > {query}_  (Enter execute, Esc cancel)"),
                        width,
                        true,
                    ));
                    let choices = self.choices(query);
                    let available = height.saturating_sub(4);
                    let first = selected.saturating_sub(available.saturating_sub(1));
                    for (i, (_, label)) in choices.iter().enumerate().skip(first).take(available) {
                        overlay.push(layout::label(
                            &format!("{} {label}", if i == *selected { ">" } else { " " }),
                            width,
                            i == *selected,
                        ));
                    }
                    if choices.is_empty() {
                        overlay.push(layout::label(
                            "No matching commands or configured targets",
                            width,
                            false,
                        ));
                    }
                }
                Dialog::Input { label, text, .. } => {
                    overlay.push(layout::label(label, width, true));
                    overlay.push(layout::label(&format!("> {text}_"), width, false));
                }
                Dialog::Confirm { text, .. } => {
                    overlay.push(layout::label(
                        "CONFIRM | y accepts | n / Esc cancels",
                        width,
                        true,
                    ));
                    let mut line = String::new();
                    for word in text.split_whitespace() {
                        if UnicodeWidthStr::width(line.as_str()) + UnicodeWidthStr::width(word) + 1
                            > width
                            && !line.is_empty()
                        {
                            overlay.push(layout::label(&line, width, false));
                            line.clear();
                        }
                        line.push_str(word);
                        line.push(' ');
                    }
                    if !line.is_empty() {
                        overlay.push(layout::label(&line, width, false));
                    }
                }
            }
            for (i, row) in overlay
                .into_iter()
                .take(height.saturating_sub(2))
                .enumerate()
            {
                rows[i + 1] = row;
            }
        }
        Ok(Frame {
            rows,
            cursor,
            size: self.size,
            insert: self.editor().input().mode() == embedide_editor::vim::Mode::Insert,
        })
    }
}
/// Terminal history keys. Shift+PageUp starts scrolling back (when there is
/// history); while scrolled, Shift+PageUp/PageDown move a page, Shift+Up/Down
/// a line, Shift+Home goes to the oldest line and Shift+End back to the live
/// screen. On the live screen every other Shift key still reaches the shell.
/// Returns whether the key was consumed.
fn scrollback_key(t: &mut TerminalPane, key: KeyEvent) -> bool {
    if key.modifiers != KeyModifiers::SHIFT {
        return false;
    }
    let page = t.page().saturating_sub(1).max(1) as isize;
    let scrolled = t.scrolled() > 0;
    match key.code {
        KeyCode::PageUp if scrolled || t.history() > 0 => t.scroll(page),
        KeyCode::PageDown if scrolled => t.scroll(-page),
        KeyCode::Up if scrolled => t.scroll(1),
        KeyCode::Down if scrolled => t.scroll(-1),
        KeyCode::Home if scrolled => t.scroll(isize::MAX),
        KeyCode::End if scrolled => t.scroll_to_live(),
        _ => return false,
    }
    true
}

/// Run a project workspace. Startup performs local metadata and one directory read only.
pub fn run(root: &Path) -> Result<(), FrontendError> {
    run_with(root, None)
}
/// Run a workspace and open `file` (from `embed <file>`) in the editor first.
pub fn run_with(
    root: &Path,
    file: Option<&embedide_workspace::LaunchFile>,
) -> Result<(), FrontendError> {
    if !(io::stdin().is_terminal() && io::stdout().is_terminal()) {
        return Err(FrontendError::NeedsTerminal);
    }
    if std::env::var_os("TERM").is_some_and(|term| term == "dumb") {
        return Err(service_error("TERM=dumb cannot run workspace TUI"));
    }
    let (width, height) = terminal::size()?;
    let mut app = Workspace::open(root, width, height)?;
    if let Some(file) = file {
        app.open_launch_file(file)?;
    }
    let mut guard = guard::TerminalGuard::enter(true)?;
    app.set_keyboard_enhanced(guard.keyboard_enhanced());
    let mut screen = Screen::default();
    let mut dirty = true;
    while !app.closed {
        if guard.interrupted() {
            return Err(FrontendError::Interrupted);
        }
        if guard.take_resize() {
            let (width, height) = terminal::size()?;
            app.handle(Event::Resize(width, height));
            dirty = true;
        }
        dirty |= app.tick();
        guard.set_mouse(app.mouse)?;
        if dirty {
            screen.set_theme(match app.config.theme.as_str() {
                "dark" => Theme::Dark,
                "light" => Theme::Light,
                _ => Theme::Mono,
            });
            screen.draw(&app.frame()?, &mut io::stdout())?;
            dirty = false;
        }
        let interval = if app.operation.is_some()
            || app.debug.is_some()
            || app.serial.is_some()
            || app.inventory.is_some()
            || app.assistant.busy()
            || app.terminal.is_some()
        {
            20
        } else {
            // The Unix resize flag may be the only SIGWINCH path. Keep its
            // latency below a visibly sluggish quarter-second idle poll.
            50
        };
        if event::poll(Duration::from_millis(interval))? {
            dirty |= app.handle(event::read()?);
        }
    }
    // Drop service owners while the terminal guard is still active.
    app.lsp.clear();
    app.operation = None;
    app.serial = None;
    app.debug = None;
    app.terminal = None;
    app.assistant.shutdown();
    Ok(())
}

/// Draw the popup under the cursor (above it when there is more room),
/// starting at the column where the completed word begins and kept inside
/// the editor area. The cursor row itself is never covered.
fn draw_popup(popup: &Popup, area: Area, cursor: (u16, u16), rows: &mut [Row]) {
    let (x, y) = (usize::from(cursor.0), usize::from(cursor.1));
    let (top, bottom) = (usize::from(area.y), usize::from(area.y + area.height));
    let (left, width) = (usize::from(area.x), usize::from(area.width));
    let below = bottom.saturating_sub(y + 1);
    let above = y.saturating_sub(top);
    let wanted = popup.shown.len().min(popup::VISIBLE) + 1;
    let room = if below >= wanted || below >= above {
        below
    } else {
        above
    };
    let (lines, w) = popup.rows(room, width);
    if lines.is_empty() {
        return;
    }
    let word = UnicodeWidthStr::width(popup.query());
    let column = x.saturating_sub(word).max(left).min(left + width - w);
    let first = if room == below {
        y + 1
    } else {
        y - lines.len()
    };
    for (i, line) in lines.iter().enumerate() {
        if let Some(row) = rows.get_mut(first + i) {
            *row = popup::overlay(row, column, line, w);
        }
    }
}

/// Assembly completion: labels of the file, then the dialect's mnemonics
/// (branches marked), directives and registers, then other words.
fn asm_candidates(
    dialect: embedide_editor::asm::Dialect,
    labels: Vec<embedide_editor::asm::Label>,
    words: Vec<Candidate>,
) -> Vec<Candidate> {
    let item = |label: &str, kind: &str, detail: String, sort: &str| Candidate {
        kind: kind.into(),
        detail,
        sort: sort.into(),
        ..Candidate::word(label)
    };
    let mut items: Vec<Candidate> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for l in labels {
        if seen.insert(l.name.clone()) {
            items.push(item(&l.name, "label", format!("line {}", l.line + 1), "0"));
        }
    }
    for m in dialect.mnemonics() {
        let (kind, detail) = if dialect.is_branch(m) {
            ("branch", dialect.name().to_string())
        } else {
            ("instruction", dialect.name().to_string())
        };
        if seen.insert(m.to_string()) {
            items.push(item(m, kind, detail, "1"));
        }
    }
    for d in dialect.directives() {
        if seen.insert(d.to_string()) {
            items.push(item(d, "directive", String::new(), "2"));
        }
    }
    for r in dialect.registers() {
        if seen.insert(r.clone()) {
            items.push(item(&r, "register", String::new(), "3"));
        }
    }
    items.extend(words.into_iter().filter(|w| seen.insert(w.label.clone())));
    items
}

/// Identity of a server process: its command line.
fn server_key(server: &embedide_core::config::LspServerConfig) -> String {
    std::iter::once(server.command.as_str())
        .chain(server.args.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Indentation for a document: the configured width and auto setting; with
/// style `detect` the file's own tabs-or-spaces convention wins.
fn indent_style(config: &NativeConfig, state: &embedide_editor::EditorState) -> IndentStyle {
    let indent = &config.indent;
    let style = IndentStyle::new(indent.width, indent.style == "tabs", indent.auto);
    if indent.style != "detect" {
        return style;
    }
    let buffer = state.buffer();
    let mut end = buffer.byte_len().min(64 * 1024);
    while end > 0 && buffer.char_at(end).is_err() {
        end -= 1;
    }
    let sample = buffer.slice(0..end).unwrap_or_default();
    style.detect(state.path(), &sample)
}

/// Key bindings whose command id does not exist (a typo would otherwise
/// leave the key silently unbound).
fn unknown_key_commands(config: &NativeConfig) -> Vec<String> {
    config
        .keys
        .iter()
        .filter(|(id, _)| !COMMANDS.iter().any(|(command, _)| command == id))
        .map(|(id, chord)| {
            format!("Key binding {chord} names unknown command {id:?}; Ctrl+K lists command ids")
        })
        .collect()
}

/// User configuration, project manifest and any problems loading them.
/// Errors never stop the IDE from opening; they appear in Settings.
fn load_configs(root: &Path) -> (UserConfig, Option<Manifest>, Vec<String>) {
    let mut notes = Vec::new();
    let user = UserConfig::load().unwrap_or_else(|e| {
        notes.push(format!("User config ignored: {}", e.0));
        UserConfig::default()
    });
    let manifest = Manifest::load(root).unwrap_or_else(|e| {
        notes.push(format!("embedide.toml ignored: {}", e.0));
        None
    });
    (user, manifest, notes)
}

/// Language id from the file extension (for the status bar, AI and compiler lookup).
fn language_of(path: Option<&Path>) -> &'static str {
    let ext = path
        .and_then(Path::extension)
        .and_then(|e| e.to_str())
        .unwrap_or("");
    match ext {
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" | "ipp" => "cpp",
        "rs" => "rust",
        "zig" => "zig",
        "s" | "S" | "sx" | "asm" | "nasm" | "inc" => "asm",
        "f" | "F" | "f90" | "F90" | "f95" | "F95" | "f03" | "F03" | "f08" | "F08" | "for" => {
            "fortran"
        }
        "toml" => "toml",
        "json" => "json",
        "md" => "markdown",
        "py" => "python",
        "ld" | "lds" => "linker-script",
        "mk" => "make",
        _ if path
            .and_then(Path::file_name)
            .is_some_and(|n| n == "Makefile" || n == "makefile") =>
        {
            "make"
        }
        _ if path
            .and_then(Path::file_name)
            .is_some_and(|n| n == "CMakeLists.txt") =>
        {
            "cmake"
        }
        _ => embedide_editor::syntax::Language::detect(path, "").id(),
    }
}

fn sanitize(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

fn pad(row: &mut Row, width: usize) {
    let used: usize = row
        .runs
        .iter()
        .map(|r| UnicodeWidthStr::width(r.text.as_str()))
        .sum();
    if used < width {
        row.push(&" ".repeat(width - used), false);
    } else if used > width {
        *row = insert_at(row, width, "", width);
    }
}

/// Insert `text` (shown in inverse video) at display column `col`, truncating at `width`.
/// Cells of the line-number gutter (`" 12 "`) at the start of an editor row.
fn gutter_width(row: &Row) -> usize {
    let text: String = row.runs.iter().map(|r| r.text.as_str()).collect();
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() && b[i] == b' ' {
        i += 1;
    }
    let digits = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i > digits && b.get(i) == Some(&b' ') {
        i + 1
    } else {
        0
    }
}

fn insert_at(row: &Row, col: usize, text: &str, width: usize) -> Row {
    use unicode_width::UnicodeWidthChar;
    fn put(
        out: &mut Row,
        used: &mut usize,
        text: &str,
        selected: bool,
        style: Option<embedide_editor::syntax::Highlight>,
        cell: Option<layout::CellStyle>,
        width: usize,
    ) {
        let mut piece = String::new();
        for ch in text.chars() {
            let w = ch.width().unwrap_or(0);
            if *used + w > width {
                break;
            }
            piece.push(ch);
            *used += w;
        }
        if !piece.is_empty() {
            out.push_attrs(&piece, selected, style, cell);
        }
    }
    let mut out = Row::default();
    let mut used = 0;
    let mut inserted = false;
    'runs: for run in &row.runs {
        let mut piece = String::new();
        for ch in run.text.chars() {
            if !inserted && used >= col {
                if !piece.is_empty() {
                    out.push_attrs(
                        &std::mem::take(&mut piece),
                        run.selected,
                        run.style,
                        run.cell,
                    );
                }
                put(&mut out, &mut used, text, true, None, None, width);
                inserted = true;
            }
            let w = ch.width().unwrap_or(0);
            if used + w > width {
                if !piece.is_empty() {
                    out.push_attrs(&piece, run.selected, run.style, run.cell);
                }
                break 'runs;
            }
            piece.push(ch);
            used += w;
        }
        if !piece.is_empty() {
            out.push_attrs(&piece, run.selected, run.style, run.cell);
        }
    }
    if !inserted && !text.is_empty() {
        if used < col {
            let gap = " ".repeat(col - used);
            put(&mut out, &mut used, &gap, false, None, None, width);
        }
        put(&mut out, &mut used, text, true, None, None, width);
    }
    out
}

fn help_lines(keys: &BTreeMap<String, String>, keyboard_enhanced: bool) -> Vec<String> {
    let mode: &[&str] = if keyboard_enhanced {
        &["        Kitty keyboard protocol active: a held digit is seen, so Alt+digit+Delete hides panels"]
    } else {
        &[
            "        This terminal cannot report a held key, so the Delete chord is unavailable: use",
            "        :hide_explorer :hide_assistant :hide_terminal (or bind them in embedide.json",
            "        \"native\": {\"keys\": {\"hide_terminal\": \"alt+6\"}}) to hide a panel",
        ]
    };
    let mut lines: Vec<String> = [
        "PANELS  Alt+1 explorer | Alt+2 AI assistant | Alt+3 terminal (press again to return to the editor; Ctrl+Alt works too)",
        "        Hold Alt and the digit and press Delete (Alt+1+Delete …) to hide that panel; its state is kept",
    ]
    .iter()
    .chain(mode)
    .chain(&[
        "        F2 show/hide explorer | F4 cycle bottom panels | F6 cycle focus | Ctrl+K command palette",
        "EDITOR  Vim modes: Normal (hjkl w b e 0 $ gg G x dd yy p u Ctrl+R / ? n N v V), Insert (i a o O, Esc),",
        "        Command (:w :q :q! :wq, :<line> go to line, :w <file> / :saveas <file> save as a NEW file (never overwrites),",
        "        plus :build :clean :cancel :e <file> :help :settings :complete :ai <question>). Search: / ? n N",
        "        Leader (Space): e explorer | b build | d debug | t terminal | s serial | f find file",
        "INDENT  Enter keeps the indentation and adds a level after { [ ( (Rust, C, C++, Zig, JS/TS, JSON), : (Python),",
        "        then/do/else/{ (shell), a label (assembly); a typed } ] ) on a blank line aligns with its opener",
        "        Insert: Tab indents (in the leading blanks) or inserts spaces/tab to the next stop; Shift+Tab dedents",
        "        Normal/Visual: Tab / Shift+Tab shift the line(s). Paste is never re-indented",
        "COMPLETE While typing (2+ letters) a popup lists words or language-server items; nothing is selected:",
        "        Down/Ctrl+N, Up/Ctrl+P select, then Tab or Enter accept. Without a selection Tab and Enter work as usual",
        "        Ctrl+Space asks explicitly and selects the first item; Esc closes it and leaves Insert mode",
        "        Servers: rust-analyzer (Rust), clangd (C/C++), zls (Zig) when installed; otherwise words of this file",
        "        Ctrl+S save | Ctrl+W close document | F10 next document | Ctrl+P find file | Ctrl+Q quit",
        "BUILD   F5 build | F7 cargo check | F8 cancel | F3 problems: j/k select, Enter jumps to the source line",
        "        Compilers come from embedide.toml and config.toml (Settings panel); nothing is installed for you",
        "TERMINAL Alt+3: your shell in a real pseudo-terminal; all keys go to it except the panel keys (Alt+1/2/3)",
        "        History: Shift+PageUp scrolls back; then Shift+PageUp/PageDown page, Shift+Up/Down line,",
        "        Shift+Home oldest, Shift+End (or typing) back to live. Full-screen programs keep these keys",
        "MOUSE   Off by default; :mouse toggles it (or \"mouse\": true in embedide.json native). Click focuses a panel,",
        "        places the cursor, selects / opens explorer rows; the wheel scrolls. Shift+drag still selects text",
        "AI      Alt+2, then /provider claude-code or /provider codex-cli; type a message, Enter sends (only then)",
        "        /file /selection /explain /edit what to change /diff /apply /reject /cancel /help",
        "        /complete or :complete shows a suggestion at the cursor: Tab accepts, Esc rejects. /help in the panel",
        "        AI is optional: without a provider the IDE works fully offline. EMBEDIDE_AI_OFFLINE=1 blocks requests",
        "EXPLORER j/k move, Enter/Right open or expand, Left collapse, n new file, Delete delete, r refresh, Esc editor",
        "PANEL   j/k scroll, PgUp/PgDn, End follow, / filter, Tab next panel, Esc editor",
        "Debugging, flashing and serial run only from explicit commands (Ctrl+K: debug, flash, serial).",
    ])
    .map(|s| s.to_string())
    .collect();
    if !keys.is_empty() {
        lines.push("Custom bindings from embedide.json keys:".into());
        lines.extend(keys.iter().map(|(id, chord)| format!("  {chord} → {id}")));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(row: &Row) -> String {
        row.runs.iter().map(|r| r.text.as_str()).collect()
    }

    #[test]
    fn multiline_ghost_text_uses_virtual_rows_and_keeps_the_frame_height() {
        let dir = std::env::temp_dir().join(format!("embedide-ghost-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.c");
        std::fs::write(&file, "int add(int a, int b) {\n    \n}\nint z;\n").unwrap();
        let mut ws = Workspace::open(&dir, 80, 20).unwrap();
        ws.open_file(&file).unwrap();
        let editor = &mut ws.documents[ws.active].session.editor;
        editor.set_cursor(28).unwrap();
        let cursor = ws.editor().state().cursor();
        ws.assistant.suggestion = Some(crate::tui::assistant::Suggestion {
            path: Some(file.canonicalize().unwrap()),
            offset: cursor,
            text: "int s = a + b;\n    return s;".into(),
            after: String::new(),
        });
        ws.documents[ws.active].size = (60, 12);
        let mut frame = ws.documents[ws.active].frame().unwrap();
        let height = frame.rows.len();
        ws.ghost_text(&mut frame, 60);
        assert_eq!(frame.rows.len(), height, "the frame keeps its height");
        let rows: Vec<String> = frame.rows.iter().map(text).collect();
        assert!(rows[1].contains("    int s = a + b;"), "{rows:?}");
        assert_eq!(
            rows[2].trim_end(),
            "       return s;",
            "a virtual row after the gutter: {rows:?}"
        );
        assert!(
            rows[3].contains("3 }"),
            "the code below moved down: {rows:?}"
        );
        assert!(
            ws.editor().state().buffer().contents().len() < 40,
            "nothing was inserted"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn ghost_text_is_inserted_at_the_cursor_column_and_clipped() {
        let mut row = Row::default();
        row.push("int ", false);
        row.push("x;", true);
        let out = insert_at(&row, 4, "y = ", 80);
        assert_eq!(text(&out), "int y = x;");
        // Adjacent inverse runs merge; the ghost text starts an inverse run.
        assert!(out
            .runs
            .iter()
            .any(|r| r.selected && r.text.starts_with("y = ")));
        assert_eq!(text(&insert_at(&row, 10, "z", 80)), "int x;    z");
        assert_eq!(text(&insert_at(&row, 2, "LONGTEXT", 6)), "inLONG");
        let mut wide = Row::default();
        wide.push("猫猫", false);
        assert_eq!(text(&insert_at(&wide, 2, "!", 80)), "猫!猫");
    }

    #[test]
    fn languages_follow_file_extensions() {
        assert_eq!(language_of(Some(Path::new("a/main.c"))), "c");
        assert_eq!(language_of(Some(Path::new("x.hpp"))), "cpp");
        assert_eq!(language_of(Some(Path::new("start.S"))), "asm");
        assert_eq!(language_of(Some(Path::new("m.f90"))), "fortran");
        assert_eq!(language_of(Some(Path::new("Makefile"))), "make");
        assert_eq!(language_of(None), "text");
    }

    fn press(code: KeyCode, kind: KeyEventKind) -> Event {
        Event::Key(KeyEvent::new_with_kind(
            code,
            KeyModifiers::CONTROL | KeyModifiers::ALT,
            kind,
        ))
    }

    #[test]
    fn held_digit_and_delete_hide_a_panel_and_keep_its_state() {
        let dir = std::env::temp_dir().join(format!("embed-ws-hide-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let mut ws = Workspace::open(&dir, 120, 40).unwrap();
        ws.set_keyboard_enhanced(true);
        ws.handle(press(KeyCode::Char('2'), KeyEventKind::Press));
        assert_eq!(ws.focus(), Focus::Assistant);
        for ch in "unsent draft".chars() {
            ws.handle(Event::Key(KeyEvent::new(
                KeyCode::Char(ch),
                KeyModifiers::NONE,
            )));
        }
        // Holding 2 (press, auto-repeat) and pressing Delete hides the AI panel.
        ws.handle(press(KeyCode::Char('2'), KeyEventKind::Press));
        assert_eq!(
            ws.focus(),
            Focus::Editor,
            "second press returns to the editor"
        );
        ws.handle(press(KeyCode::Char('2'), KeyEventKind::Repeat));
        ws.handle(press(KeyCode::Delete, KeyEventKind::Press));
        assert!(!ws.assistant_visible);
        assert_eq!(ws.focus(), Focus::Editor);
        ws.handle(press(KeyCode::Delete, KeyEventKind::Release));
        ws.handle(press(KeyCode::Char('2'), KeyEventKind::Release));
        // Reopening restores the unsent input.
        ws.handle(press(KeyCode::Char('2'), KeyEventKind::Press));
        ws.handle(press(KeyCode::Char('2'), KeyEventKind::Release));
        assert!(ws.assistant_visible);
        assert_eq!(ws.focus(), Focus::Assistant);
        assert_eq!(ws.assistant.input, "unsent draft");
        // Released digit, then Ctrl+Alt+Delete: separate presses never hide.
        ws.handle(press(KeyCode::Delete, KeyEventKind::Press));
        assert!(ws.assistant_visible);
        // Explorer: same chord with 1.
        ws.handle(press(KeyCode::Char('1'), KeyEventKind::Press));
        ws.handle(press(KeyCode::Delete, KeyEventKind::Press));
        assert!(!ws.explorer_visible);
        ws.handle(press(KeyCode::Char('1'), KeyEventKind::Release));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn without_release_events_the_hide_chord_is_never_inferred() {
        let dir = std::env::temp_dir().join(format!("embed-ws-legacy-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let mut ws = Workspace::open(&dir, 120, 40).unwrap();
        ws.set_keyboard_enhanced(false);
        ws.handle(press(KeyCode::Char('2'), KeyEventKind::Press));
        ws.handle(press(KeyCode::Delete, KeyEventKind::Press));
        assert!(ws.assistant_visible, "a held key cannot be observed");
        ws.command("hide_assistant").unwrap();
        assert!(!ws.assistant_visible);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn explorer_refresh_keeps_open_directories_selection_and_survives_deletions() {
        let dir = std::env::temp_dir().join(format!("embed-ws-tree-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src/drivers")).unwrap();
        fs::create_dir_all(dir.join("gone")).unwrap();
        fs::write(dir.join("src/main.c"), "").unwrap();
        fs::write(dir.join("src/drivers/uart.c"), "").unwrap();
        fs::write(dir.join("gone/x.c"), "").unwrap();
        let mut ws = Workspace::open(&dir, 120, 40).unwrap();
        let root = ws.root.clone();
        let select = |ws: &mut Workspace, rel: &str| {
            ws.selected = ws
                .tree
                .iter()
                .position(|r| r.entry.path == root.join(rel))
                .unwrap();
        };
        for rel in ["gone", "src", "src/drivers"] {
            select(&mut ws, rel);
            ws.toggle_entry().unwrap();
        }
        select(&mut ws, "src/drivers/uart.c");
        fs::remove_dir_all(dir.join("gone")).unwrap();
        fs::write(dir.join("src/new.h"), "").unwrap();
        ws.command("refresh").unwrap();
        let paths: Vec<_> = ws
            .tree
            .iter()
            .map(|r| {
                r.entry
                    .path
                    .strip_prefix(&root)
                    .unwrap()
                    .display()
                    .to_string()
            })
            .collect();
        assert_eq!(
            paths,
            [
                "src",
                "src/drivers",
                "src/drivers/uart.c",
                "src/main.c",
                "src/new.h"
            ],
            "expanded directories stay open; the deleted one is gone"
        );
        assert_eq!(
            ws.tree[ws.selected].entry.path,
            root.join("src/drivers/uart.c")
        );
        // The selected file was deleted: the selection stays in range.
        fs::remove_dir_all(dir.join("src")).unwrap();
        ws.command("refresh").unwrap();
        assert!(ws.tree.is_empty() && ws.selected == 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn mouse_clicks_focus_panels_place_the_cursor_and_the_wheel_scrolls() {
        use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
        let dir = std::env::temp_dir().join(format!("embed-ws-mouse-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let body: String = (1..=80)
            .map(|i| format!("line {i}\tx\u{4e2d}y\n"))
            .collect();
        fs::write(dir.join("a.c"), &body).unwrap();
        fs::write(dir.join("b.c"), "").unwrap();
        let mut ws = Workspace::open(&dir, 120, 40).unwrap();
        let click = |column, row| {
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column,
                row,
                modifiers: KeyModifiers::NONE,
            })
        };
        let wheel = |down: bool, column, row| {
            Event::Mouse(MouseEvent {
                kind: if down {
                    MouseEventKind::ScrollDown
                } else {
                    MouseEventKind::ScrollUp
                },
                column,
                row,
                modifiers: KeyModifiers::NONE,
            })
        };
        ws.frame().unwrap();
        // Off by default: clicks are ignored.
        assert!(!ws.mouse);
        ws.handle(click(2, 3));
        assert_eq!(ws.selected, 0);
        ws.command("mouse").unwrap();
        // Explorer: row 2 is the header, row 3 = a.c, row 4 = b.c.
        ws.handle(click(2, 4));
        assert_eq!((ws.focus(), ws.selected), (Focus::Explorer, 1));
        ws.handle(click(2, 3));
        ws.handle(click(2, 3)); // second click on the selected row opens it
        assert_eq!(
            ws.editor().state().path(),
            Some(ws.root.join("a.c").as_path())
        );
        ws.frame().unwrap();
        let editor = ws.hits.editor.unwrap();
        // Gutter is 2 digits + 2 cells. Line 3 "line 3<tab>x中y": column 12 is
        // the 中 (tab stops at 4 → "line 3" 0..6, tab 6..8, x 8, 中 9..11, y 11).
        ws.handle(click(editor.x + 4 + 10, editor.y + 2));
        assert_eq!(ws.focus(), Focus::Editor);
        let state = ws.editor().state();
        let line = state.line_range(2).unwrap();
        assert_eq!(
            state.cursor() - line.start,
            "line 3\tx".len(),
            "click on 中 selects it"
        );
        // Wheel moves the editor three lines per notch.
        ws.handle(wheel(true, editor.x + 5, editor.y));
        assert_eq!(ws.editor().state().position().unwrap().line, 5);
        ws.handle(wheel(false, editor.x + 5, editor.y));
        assert_eq!(ws.editor().state().position().unwrap().line, 2);
        // Clicking the bottom panel area while it shows the terminal focuses the shell.
        ws.terminal =
            Some(TerminalPane::spawn("/bin/sh", &dir, &BTreeMap::new(), &[], 10, 80).unwrap());
        ws.panel = Panel::Terminal;
        ws.panel_visible = true;
        ws.frame().unwrap();
        let bottom = ws.hits.bottom.unwrap();
        ws.handle(click(bottom.x + 3, bottom.y + 2));
        assert_eq!(ws.focus(), Focus::Terminal);
        ws.command("mouse").unwrap();
        assert!(!ws.mouse);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_binding_to_a_misspelled_command_is_reported_at_startup() {
        let dir = std::env::temp_dir().join(format!("embed-ws-badkey-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("embedide.json"),
            r#"{"native": {"keys": {"biuld": "f12"}}}"#,
        )
        .unwrap();
        let ws = Workspace::open(&dir, 120, 40).unwrap();
        assert!(
            ws.notification().contains("unknown command \"biuld\""),
            "{}",
            ws.notification()
        );
        let _ = fs::remove_dir_all(&dir);
    }

    fn completion_workspace(
        name: &str,
        files: &[(&str, &str)],
        native: &str,
    ) -> (Workspace, PathBuf) {
        let dir = std::env::temp_dir().join(format!("embed-ws-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for (file, text) in files {
            fs::write(dir.join(file), text).unwrap();
        }
        fs::write(
            dir.join("embedide.json"),
            format!(r#"{{"native": {native}}}"#),
        )
        .unwrap();
        let ws = Workspace::open(&dir, 120, 40).unwrap();
        (ws, dir)
    }

    fn key(ws: &mut Workspace, code: KeyCode, modifiers: KeyModifiers) {
        ws.handle(Event::Key(KeyEvent::new(code, modifiers)));
    }

    fn buffer(ws: &Workspace) -> String {
        ws.editor().state().buffer().contents()
    }

    fn shown(ws: &Workspace) -> Vec<String> {
        ws.popup
            .as_ref()
            .map(|p| p.shown.iter().map(|&i| p.items[i].label.clone()).collect())
            .unwrap_or_default()
    }

    #[test]
    fn word_completion_opens_while_typing_and_enter_tab_need_a_selection() {
        let (mut ws, dir) = completion_workspace(
            "words",
            &[("notes.txt", "compute counter cobalt")],
            r#"{"completion": {"lsp": false}}"#,
        );
        ws.open_file(Path::new("notes.txt")).unwrap();
        type_keys(&mut ws, "Go");
        type_keys(&mut ws, "c");
        assert!(ws.popup.is_none(), "below min_chars");
        type_keys(&mut ws, "o");
        assert_eq!(shown(&ws), ["cobalt", "compute", "counter"]);
        assert_eq!(ws.popup.as_ref().unwrap().selected, None);
        ws.frame().unwrap();
        // Enter without a selection is a newline; the popup closes.
        key(&mut ws, KeyCode::Enter, KeyModifiers::NONE);
        assert!(ws.popup.is_none());
        assert_eq!(buffer(&ws), "compute counter cobalt\nco\n");
        // Typing narrows; Down selects; Tab accepts and replaces the typed prefix.
        type_keys(&mut ws, "cou");
        // Prefix matches first, then in-order matches (c-o-…-u in "compute").
        assert_eq!(shown(&ws), ["counter", "compute"]);
        key(&mut ws, KeyCode::Down, KeyModifiers::NONE);
        key(&mut ws, KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(buffer(&ws), "compute counter cobalt\nco\ncounter");
        assert!(ws.popup.is_none());
        // Ctrl+N / Ctrl+P navigate (Ctrl+P does not open the file finder), Enter accepts.
        type_keys(&mut ws, " co");
        key(&mut ws, KeyCode::Char('n'), KeyModifiers::CONTROL);
        key(&mut ws, KeyCode::Char('n'), KeyModifiers::CONTROL);
        key(&mut ws, KeyCode::Char('p'), KeyModifiers::CONTROL);
        assert!(ws.dialog.is_none());
        key(&mut ws, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(buffer(&ws), "compute counter cobalt\nco\ncounter cobalt");
        // Esc closes the popup and leaves Insert mode, so `Esc :w` saves.
        type_keys(&mut ws, " co");
        assert!(ws.popup.is_some());
        key(&mut ws, KeyCode::Esc, KeyModifiers::NONE);
        assert!(ws.popup.is_none());
        assert_eq!(ws.editor().input().mode(), Mode::Normal);
        type_keys(&mut ws, ":w\r");
        assert_eq!(
            fs::read_to_string(dir.join("notes.txt")).unwrap(),
            "compute counter cobalt\nco\ncounter cobalt co"
        );
        // The whole Insert session (from `o`), completions included, is one undo step.
        type_keys(&mut ws, "u");
        assert_eq!(buffer(&ws), "compute counter cobalt");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn manual_completion_unicode_words_tab_indent_and_normal_mode() {
        let (mut ws, dir) = completion_workspace(
            "manual",
            &[("u.txt", "数据量 значение zeta")],
            r#"{"completion": {"lsp": false, "auto": false}}"#,
        );
        ws.open_file(Path::new("u.txt")).unwrap();
        // Normal mode: letters are commands, no popup.
        type_keys(&mut ws, "jj");
        assert!(ws.popup.is_none());
        type_keys(&mut ws, "Go");
        // auto=false: typing does not open it; Ctrl+Space does, with the first item selected.
        type_keys(&mut ws, "зн");
        assert!(ws.popup.is_none());
        key(&mut ws, KeyCode::Char(' '), KeyModifiers::CONTROL);
        assert_eq!(shown(&ws), ["значение"]);
        assert_eq!(ws.popup.as_ref().unwrap().selected, Some(0));
        key(&mut ws, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(buffer(&ws), "数据量 значение zeta\nзначение");
        // Wide characters: the popup is drawn at the word start, inside the editor.
        type_keys(&mut ws, " 数");
        key(&mut ws, KeyCode::Char(' '), KeyModifiers::CONTROL);
        let frame = ws.frame().unwrap();
        let (cx, cy) = frame.cursor.unwrap();
        let below = frame.rows[usize::from(cy) + 1]
            .runs
            .iter()
            .map(|r| r.text.as_str())
            .collect::<String>();
        assert!(below.contains(">数据量"), "{below}");
        let prefix: String = below.chars().take_while(|&c| c != '>').collect();
        assert_eq!(
            UnicodeWidthStr::width(prefix.as_str()),
            usize::from(cx) - 2,
            "popup starts at the word (数 is 2 cells)"
        );
        key(&mut ws, KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(buffer(&ws), "数据量 значение zeta\nзначение 数据量");
        // No popup: Tab is indentation again (after text: spaces to the next stop).
        key(&mut ws, KeyCode::Tab, KeyModifiers::NONE);
        // "значение 数据量" is 15 cells: one space reaches column 16.
        assert_eq!(buffer(&ws), "数据量 значение zeta\nзначение 数据量 ");
        // Ctrl+Space in Normal mode explains instead of changing anything.
        key(&mut ws, KeyCode::Esc, KeyModifiers::NONE);
        key(&mut ws, KeyCode::Char(' '), KeyModifiers::CONTROL);
        assert!(
            ws.notification().contains("Insert mode"),
            "{}",
            ws.notification()
        );
        let _ = fs::remove_dir_all(&dir);
    }

    fn mock_lsp(mode: &str, log: &Path) -> String {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../embedide-core/tests/fixtures/mock_lsp.py");
        serde_json::json!({ "completion": { "servers": { "rust": {
            "command": "python3",
            "args": [script, mode, log]
        }}}})
        .to_string()
    }

    fn tick_until(ws: &mut Workspace, mut done: impl FnMut(&Workspace) -> bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !done(ws) {
            assert!(
                std::time::Instant::now() < deadline,
                "timed out; {}",
                ws.notification()
            );
            ws.tick();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn language_server_items_replace_their_range_and_a_crash_falls_back_to_words() {
        let log =
            std::env::temp_dir().join(format!("embed-ws-lsp-log-{}.jsonl", std::process::id()));
        let _ = fs::remove_file(&log);
        let (mut ws, dir) = completion_workspace(
            "lsp",
            &[("m.rs", "fn main() {\n    let primary = 1;\n}\n")],
            &mock_lsp("normal", &log),
        );
        ws.open_file(Path::new("m.rs")).unwrap();
        type_keys(&mut ws, "jo");
        type_keys(&mut ws, "pr");
        // Words first (primary), then the server's items when it is ready.
        assert!(
            ws.notification().contains("Starting python3"),
            "{}",
            ws.notification()
        );
        tick_until(&mut ws, |ws| ws.popup.as_ref().is_some_and(|p| p.semantic));
        assert_eq!(shown(&ws), ["println!", "print_ж"]);
        assert_eq!(ws.popup.as_ref().unwrap().source, "python3");
        // Typing more re-filters the server's list without a new popup.
        type_keys(&mut ws, "i");
        tick_until(&mut ws, |ws| {
            ws.popup.as_ref().is_some_and(|p| p.request.is_none())
        });
        key(&mut ws, KeyCode::Down, KeyModifiers::NONE);
        key(&mut ws, KeyCode::Enter, KeyModifiers::NONE);
        // The server's range covered "pr"; the typed "i" after it is replaced too.
        assert_eq!(
            buffer(&ws),
            "fn main() {\n    let primary = 1;\n    println!()\n}\n"
        );
        // Closing the document tells the server.
        ws.command("close").unwrap();
        ws.dialog = None;
        ws.documents[ws.active].session.editor.save().unwrap();
        ws.close_document();
        drop(ws);
        let methods: Vec<String> = fs::read_to_string(&log)
            .unwrap()
            .lines()
            .filter_map(|l| {
                serde_json::from_str::<serde_json::Value>(l).ok()?["method"]
                    .as_str()
                    .map(String::from)
            })
            .collect();
        assert!(
            methods.iter().any(|m| m == "textDocument/didClose"),
            "{methods:?}"
        );
        let _ = fs::remove_dir_all(&dir);
        // A crashing server: a clear message, and word completion keeps working.
        let (mut ws, dir) = completion_workspace(
            "lsp-crash",
            &[("m.rs", "fn main() {\n    let primary = 1;\n}\n")],
            &mock_lsp("crash", &log),
        );
        ws.open_file(Path::new("m.rs")).unwrap();
        type_keys(&mut ws, "jo");
        type_keys(&mut ws, "pr");
        tick_until(&mut ws, |ws| {
            ws.notification().contains("completion uses words")
        });
        key(&mut ws, KeyCode::Esc, KeyModifiers::NONE);
        type_keys(&mut ws, "a pri");
        assert_eq!(shown(&ws), ["primary"]);
        assert!(!ws.popup.as_ref().unwrap().semantic);
        let _ = fs::remove_file(&log);
        let _ = fs::remove_dir_all(&dir);
    }

    fn gcc_program(dir: &Path) -> Option<PathBuf> {
        let src = dir.join("prog.c");
        fs::write(
            &src,
            "int twice(int x) { return 2 * x; }\nint main(void) { return twice(0); }\n",
        )
        .ok()?;
        let out = dir.join("prog");
        std::process::Command::new("gcc")
            .args(["-O0", "-o"])
            .arg(&out)
            .arg(&src)
            .status()
            .ok()?
            .success()
            .then_some(out)
    }

    fn screen_text(ws: &mut Workspace) -> String {
        ws.frame()
            .unwrap()
            .rows
            .iter()
            .map(|r| r.runs.iter().map(|x| x.text.as_str()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn binary_files_open_in_hex_elf_and_disassembly_views_without_changes() {
        let dir = std::env::temp_dir().join(format!("embed-ws-bin-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let Some(prog) = gcc_program(&dir) else {
            return;
        };
        let original = fs::read(&prog).unwrap();
        let mut ws = Workspace::open(&dir, 140, 40).unwrap();
        ws.open_file(Path::new("prog")).unwrap();
        assert!(ws.binary.is_some(), "binary files open in the binary view");
        assert_eq!(ws.document_count(), 1, "not as a text document");
        let screen = screen_text(&mut ws);
        assert!(screen.contains("00000000  7F 45 4C 46"), "{screen}");
        assert!(screen.contains(".ELF"));
        assert!(ws.status_line().starts_with("HEX | prog"));
        // Jump to an offset and search for bytes.
        type_keys(&mut ws, "g0x10\r");
        assert_eq!(ws.binary.as_ref().unwrap().cursor, 0x10);
        type_keys(&mut ws, "/\"ELF\"\r");
        assert_eq!(ws.binary.as_ref().unwrap().cursor, 1);
        // ELF inspector: header, sections, symbols → main in the disassembly.
        type_keys(&mut ws, "e");
        let screen = screen_text(&mut ws);
        assert!(
            screen.contains("ELF64 LE") && screen.contains("Entry point"),
            "{screen}"
        );
        type_keys(&mut ws, "3");
        assert!(screen_text(&mut ws).contains(".text"));
        type_keys(&mut ws, "4/main\r\r");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !screen_text(&mut ws).contains("<main>:") {
            assert!(std::time::Instant::now() < deadline, "no disassembly");
            ws.tick();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(ws.binary.as_ref().unwrap().mode, binview::Mode::Disasm);
        let screen = screen_text(&mut ws);
        assert!(screen.contains("<twice>:") || screen.contains("<main>:"));
        // Enter on an instruction goes back to its bytes in the hex view.
        type_keys(&mut ws, "j\r");
        assert_eq!(ws.binary.as_ref().unwrap().mode, binview::Mode::Hex);
        assert!(ws.binary.as_ref().unwrap().cursor > 0);
        // Nothing was written by viewing.
        type_keys(&mut ws, "q");
        assert!(ws.binary.is_none());
        assert_eq!(fs::read(&prog).unwrap(), original);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn hex_editing_is_explicit_undoable_confirmed_and_byte_exact() {
        let dir = std::env::temp_dir().join(format!("embed-ws-hexedit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let data: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
        let path = dir.join("blob.bin");
        fs::write(&path, &data).unwrap();
        let mut ws = Workspace::open(&dir, 120, 40).unwrap();
        ws.open_file(Path::new("blob.bin")).unwrap();
        // Navigation mode: hex digits do nothing.
        type_keys(&mut ws, "ab");
        assert!(!ws.binary.as_ref().unwrap().file.is_modified());
        // Edit mode: two digits per byte, the cursor advances.
        type_keys(&mut ws, "g0x10\riaa55");
        let view = ws.binary.as_mut().unwrap();
        assert_eq!(
            (view.file.byte(0x10).unwrap(), view.file.byte(0x11).unwrap()),
            (Some(0xaa), Some(0x55))
        );
        assert_eq!(fs::read(&path).unwrap(), data, "not written yet");
        assert!(
            ws.status_line().contains("[+2 bytes changed]"),
            "{}",
            ws.status_line()
        );
        // Undo / redo (navigation mode).
        type_keys(&mut ws, "\x1bu");
        assert_eq!(
            ws.binary.as_mut().unwrap().file.byte(0x11).unwrap(),
            Some(data[0x11])
        );
        ws.handle(Event::Key(KeyEvent::new(
            KeyCode::Char('r'),
            KeyModifiers::CONTROL,
        )));
        assert_eq!(
            ws.binary.as_mut().unwrap().file.byte(0x11).unwrap(),
            Some(0x55)
        );
        // Closing with changes asks; Esc keeps them.
        type_keys(&mut ws, "q");
        assert!(ws.dialog.is_some());
        type_keys(&mut ws, "\x1b");
        assert!(ws.binary.is_some());
        // w asks, y writes exactly the two bytes.
        type_keys(&mut ws, "w");
        assert!(matches!(ws.dialog, Some(Dialog::Confirm { .. })));
        type_keys(&mut ws, "y");
        let mut expected = data.clone();
        expected[0x10] = 0xaa;
        expected[0x11] = 0x55;
        assert_eq!(fs::read(&path).unwrap(), expected);
        assert!(!ws.binary.as_ref().unwrap().file.is_modified());
        type_keys(&mut ws, "q");
        assert!(ws.binary.is_none());
        // Read-only configuration: no edit mode.
        fs::write(
            dir.join("embedide.json"),
            r#"{"native": {"binary": {"allow_edit": false}}}"#,
        )
        .unwrap();
        let mut ws = Workspace::open(&dir, 120, 40).unwrap();
        ws.open_file(Path::new("blob.bin")).unwrap();
        type_keys(&mut ws, "i00");
        assert!(!ws.binary.as_ref().unwrap().file.is_modified());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_ai_command_typed_in_the_editor_explains_where_it_belongs() {
        let dir = std::env::temp_dir().join(format!("embed-ws-aihint-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("main.c"), "int main(void) { return 0; }\n").unwrap();
        let mut ws = Workspace::open(&dir, 120, 40).unwrap();
        ws.open_file(Path::new("main.c")).unwrap();
        type_keys(&mut ws, "/provider <codex-cli>\r");
        let message = &ws.documents[ws.active].session.message;
        assert!(
            message.contains("AI panel") && message.contains("/provider <codex-cli>"),
            "{message}"
        );
        assert!(
            ws.notification().contains("AI panel"),
            "{}",
            ws.notification()
        );
        // An ordinary failed search keeps the ordinary message.
        type_keys(&mut ws, "\x1b/nosuchword\r");
        assert!(ws.documents[ws.active]
            .session
            .message
            .contains("not found: nosuchword"));
        let _ = fs::remove_dir_all(&dir);
    }

    fn type_keys(ws: &mut Workspace, text: &str) {
        for ch in text.chars() {
            let key = match ch {
                '\r' => KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                '\x1b' => KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
                c => KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
            };
            ws.handle(Event::Key(key));
        }
    }

    #[test]
    fn goto_line_and_save_as_never_overwrite_and_untitled_buffers_get_a_name() {
        let dir = std::env::temp_dir().join(format!("embed-ws-saveas-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("taken.c"), "keep me\n").unwrap();
        let mut ws = Workspace::open(&dir, 120, 40).unwrap();
        ws.focus = Focus::Editor;
        type_keys(&mut ws, "ione\rtwo\rthree\x1b");
        // Untitled :w explains how to name the file; nothing is written.
        type_keys(&mut ws, ":w\r");
        let message = &ws.documents[ws.active].session.message;
        assert!(message.contains(":w <file>"), "{message}");
        type_keys(&mut ws, "\x1b"); // a failed command line stays editable; leave it
                                    // :2 goes to line 2; :99 clamps to the last line.
        type_keys(&mut ws, ":2\r");
        assert_eq!(ws.editor().state().position().unwrap().line, 1);
        type_keys(&mut ws, ":99\r");
        assert_eq!(ws.editor().state().position().unwrap().line, 2);
        // Save As refuses an existing file and leaves it untouched.
        type_keys(&mut ws, ":w taken.c\r");
        assert!(
            ws.notification().starts_with("Error"),
            "{}",
            ws.notification()
        );
        assert_eq!(
            fs::read_to_string(dir.join("taken.c")).unwrap(),
            "keep me\n"
        );
        assert!(ws.editor().state().is_modified());
        type_keys(&mut ws, "\x1b");
        // A new name works; later plain :w saves there, and the language follows.
        type_keys(&mut ws, ":saveas src.c\r");
        assert!(
            ws.notification().starts_with("Saved as"),
            "{}",
            ws.notification()
        );
        assert_eq!(
            fs::read_to_string(dir.join("src.c")).unwrap(),
            "one\ntwo\nthree"
        );
        assert!(!ws.editor().state().is_modified());
        type_keys(&mut ws, "o// more\x1b:w\r");
        assert_eq!(
            fs::read_to_string(dir.join("src.c")).unwrap(),
            "one\ntwo\nthree\n// more"
        );
        assert_eq!(language_of(ws.editor().state().path()), "c");
        let _ = fs::remove_dir_all(&dir);
    }

    fn wait_for(t: &mut TerminalPane, needle: &str) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            t.poll();
            if t.contents().contains(needle) {
                return;
            }
            assert!(std::time::Instant::now() < deadline, "missing {needle:?}");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn terminal_scrollback_keeps_position_colours_and_returns_live_on_typing() {
        let dir = std::env::temp_dir().join(format!("embed-ws-scroll-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let mut ws = Workspace::open(&dir, 120, 40).unwrap();
        let mut t = TerminalPane::spawn("/bin/sh", &dir, &BTreeMap::new(), &[], 10, 80).unwrap();
        let shift = |code| KeyEvent::new(code, KeyModifiers::SHIFT);
        // No history yet: Shift+PageUp belongs to the shell.
        assert!(!scrollback_key(&mut t, shift(KeyCode::PageUp)));
        t.paste("i=1; while [ $i -le 60 ]; do printf '\\033[32mL%02d\\033[0m\\n' $i; i=$((i+1)); done; echo DONE\n");
        wait_for(&mut t, "DONE");
        assert!(t.history() >= 50, "history {}", t.history());
        // Shift+PageUp: one page (rows - 1) back; the cursor is hidden.
        assert!(scrollback_key(&mut t, shift(KeyCode::PageUp)));
        assert_eq!(t.scrolled(), 9);
        let (rows, cursor) = t.rows(80, 10);
        assert!(cursor.is_none());
        let first = rows[0]
            .runs
            .iter()
            .map(|r| r.text.as_str())
            .collect::<String>();
        let line: String = first.trim().into();
        assert!(line.starts_with('L'), "{line:?}");
        // Colours survive in history.
        assert!(rows[0].runs[0].cell.is_some_and(|c| c.fg.is_some()));
        // New output does not move a scrolled view: it stays on the same lines.
        let (at, before, grown) = (t.scrolled(), t.rows(80, 10).0, t.history() + 2);
        t.paste("echo AFTER");
        t.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while t.history() < grown {
            assert!(std::time::Instant::now() < deadline, "no new output");
            t.poll();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(t.scrolled() >= at + 2, "the view follows its lines");
        assert_eq!(t.rows(80, 10).0, before, "scrolled view is stable");
        // Line keys, oldest, live.
        let at = t.scrolled();
        assert!(scrollback_key(&mut t, shift(KeyCode::Down)));
        assert_eq!(t.scrolled(), at - 1);
        assert!(scrollback_key(&mut t, shift(KeyCode::Home)));
        assert_eq!(t.scrolled(), t.history());
        assert!(scrollback_key(&mut t, shift(KeyCode::End)));
        assert_eq!(t.scrolled(), 0);
        // Live: Shift+End and Shift+Up go to the shell.
        assert!(!scrollback_key(&mut t, shift(KeyCode::End)));
        assert!(!scrollback_key(&mut t, shift(KeyCode::Up)));
        // Through the workspace: typing while scrolled returns live and types.
        ws.terminal = Some(t);
        ws.focus = Focus::Terminal;
        ws.handle(Event::Key(shift(KeyCode::PageUp)));
        assert!(ws.terminal.as_ref().unwrap().scrolled() > 0);
        let title = ws.draw_terminal(80, 11).0[0]
            .runs
            .iter()
            .map(|r| r.text.clone())
            .collect::<String>();
        assert!(title.contains("HISTORY"), "{title}");
        ws.handle(Event::Key(KeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        )));
        assert_eq!(ws.terminal.as_ref().unwrap().scrolled(), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_terminals_switch_panels_with_alt_digit_and_honour_hide_bindings() {
        // Regression: without the kitty protocol, Ctrl+Alt+<digit> arrives as
        // ESC <digit> (crossterm: Alt+<digit>), and no key switched panels.
        let dir = std::env::temp_dir().join(format!("embed-ws-alt-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("embedide.json"),
            r#"{"native": {"keys": {"hide_terminal": "alt+6", "focus_assistant": "ctrl+alt+5"}}}"#,
        )
        .unwrap();
        let mut ws = Workspace::open(&dir, 120, 40).unwrap();
        assert!(ws.config_notes.is_empty(), "{:?}", ws.config_notes);
        ws.set_keyboard_enhanced(false);
        let alt = |c| Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT));
        assert_eq!(ws.focus(), Focus::Explorer, "no document: explorer first");
        ws.handle(alt('1'));
        assert_eq!(ws.focus(), Focus::Editor);
        ws.handle(alt('1'));
        assert_eq!(ws.focus(), Focus::Explorer);
        ws.handle(alt('1'));
        assert_eq!(ws.focus(), Focus::Editor);
        // A rebound Ctrl+Alt chord follows the binding; the old default is free.
        ws.handle(alt('5'));
        assert_eq!(ws.focus(), Focus::Assistant);
        ws.handle(alt('2'));
        assert_eq!(ws.focus(), Focus::Assistant, "default moved to ctrl+alt+5");
        // A hide binding works even while the shell owns the keyboard. The
        // shell is the user's $SHELL; /bin/sh keeps the test hermetic.
        ws.terminal =
            Some(TerminalPane::spawn("/bin/sh", &dir, &BTreeMap::new(), &[], 10, 80).unwrap());
        ws.handle(alt('3'));
        assert_eq!(ws.focus(), Focus::Terminal);
        ws.handle(alt('6'));
        assert_eq!(ws.focus(), Focus::Editor);
        assert!(
            ws.terminal.as_ref().unwrap().exited().is_none(),
            "shell kept"
        );
        assert!(
            ws.notification.contains("Alt+3 reopens"),
            "{}",
            ws.notification
        );
        // Regression: Esc then Ctrl+S in one read arrives as ESC ^S
        // (Ctrl+Alt+S) and must still save, as before the Ctrl+Alt chords.
        let ctrl_alt = KeyModifiers::CONTROL | KeyModifiers::ALT;
        let save = KeyEvent::new(KeyCode::Char('s'), ctrl_alt);
        assert_eq!(ws.shortcut(save).as_deref(), Some("save"));
        // With the kitty protocol Alt+digit switches panels too.
        ws.set_keyboard_enhanced(true);
        assert_eq!(ws.shortcut(save).as_deref(), Some("save"));
        ws.handle(alt('1'));
        assert_eq!(ws.focus(), Focus::Explorer);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn panel_keys_toggle_focus_and_ex_commands_reach_the_host() {
        let dir = std::env::temp_dir().join(format!("embed-ws-unit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.c"), "int a;\n").unwrap();
        let mut ws = Workspace::open(&dir, 120, 40).unwrap();
        ws.set_keyboard_enhanced(true);
        // What crossterm decodes from kitty's `CSI 50;7u` (Ctrl+Alt+2).
        let chord = |c| {
            Event::Key(KeyEvent::new(
                KeyCode::Char(c),
                KeyModifiers::CONTROL | KeyModifiers::ALT,
            ))
        };
        ws.handle(chord('2'));
        assert_eq!(ws.focus(), Focus::Assistant);
        ws.handle(chord('2'));
        assert_eq!(ws.focus(), Focus::Editor);
        ws.handle(chord('1'));
        assert_eq!(ws.focus(), Focus::Explorer);
        ws.handle(chord('1'));
        assert_eq!(ws.focus(), Focus::Editor);
        // Alt+digit is the primary panel key in every terminal.
        let alt = |c| Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT));
        ws.handle(alt('2'));
        assert_eq!(ws.focus(), Focus::Assistant);
        ws.handle(alt('2'));
        assert_eq!(ws.focus(), Focus::Editor);
        // Held Alt+1 (kitty press/repeat) and Delete hides the explorer.
        ws.handle(Event::Key(KeyEvent::new_with_kind(
            KeyCode::Char('1'),
            KeyModifiers::ALT,
            KeyEventKind::Press,
        )));
        ws.handle(Event::Key(KeyEvent::new_with_kind(
            KeyCode::Char('1'),
            KeyModifiers::ALT,
            KeyEventKind::Repeat,
        )));
        ws.handle(Event::Key(KeyEvent::new_with_kind(
            KeyCode::Delete,
            KeyModifiers::ALT,
            KeyEventKind::Press,
        )));
        assert!(!ws.explorer_visible);
        ws.handle(Event::Key(KeyEvent::new_with_kind(
            KeyCode::Char('1'),
            KeyModifiers::ALT,
            KeyEventKind::Release,
        )));
        ws.handle(alt('1'));
        assert!(ws.explorer_visible);
        ws.handle(alt('1'));
        assert_eq!(ws.focus(), Focus::Editor);
        for ch in ":e a.c".chars() {
            ws.handle(Event::Key(KeyEvent::new(
                KeyCode::Char(ch),
                KeyModifiers::NONE,
            )));
        }
        ws.handle(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        )));
        assert!(ws
            .editor()
            .state()
            .path()
            .is_some_and(|p| p.ends_with("a.c")));
        assert!(
            ws.status_line().starts_with("NORMAL | a.c | c |"),
            "{}",
            ws.status_line()
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
