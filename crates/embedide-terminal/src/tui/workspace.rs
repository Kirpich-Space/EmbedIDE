//! Project workspace adapter. Services and document mutations remain outside widgets.
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
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    terminal,
};
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
    ("assistant", "Show / Hide AI Panel"),
    ("terminal_restart", "Restart Terminal Shell"),
    (
        "complete",
        "AI: Suggest Completion at Cursor (Tab accepts, Esc rejects)",
    ),
    ("explain", "AI: Explain Current File or Selection"),
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
    ("help", "Keyboard Help"),
    ("close", "Close Document"),
    ("quit", "Quit Workspace"),
];
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
        let (user, manifest, config_notes) = load_configs(&root);
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
        let mut doc = TuiEditor::new(editor, self.size.0, self.size.1);
        doc.viewport.set_tab_width(self.config.tab_width);
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
    fn refresh(&mut self) -> core::Result<()> {
        self.tree = explorer::read_directory(&self.root, &self.root)?
            .into_iter()
            .map(|entry| TreeRow {
                entry,
                depth: 0,
                expanded: false,
            })
            .collect();
        self.selected = self.selected.min(self.tree.len().saturating_sub(1));
        self.tree_top = 0;
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
                let (user, manifest, notes) = load_configs(&self.root);
                self.assistant = Assistant::new(user.ai.clone());
                self.user = user;
                self.manifest = manifest;
                self.config_notes = notes;
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
        let binding = if key.modifiers.contains(KeyModifiers::CONTROL) {
            if let KeyCode::Char(ch) = key.code {
                format!("ctrl+{}", ch.to_ascii_lowercase())
            } else {
                return None;
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
        if let Some((id, _)) = self
            .config
            .keys
            .iter()
            .find(|(_, chord)| chord.to_lowercase() == binding)
        {
            return Some(id.clone());
        }
        let default = match binding.as_str() {
            "alt+1" => "focus_explorer",
            "alt+2" => "focus_assistant",
            "alt+3" => "focus_terminal",
            "ctrl+p" => "find",
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
        };
        if self.config.keys.contains_key(default) {
            None
        } else {
            Some(default.into())
        }
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
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                if self.dialog.is_some() {
                    return self.handle_dialog(key);
                }
                let shortcut = self.shortcut(key);
                if self.focus == Focus::Terminal
                    && !shortcut
                        .as_deref()
                        .is_some_and(|id| id.starts_with("focus_"))
                {
                    // The shell owns the keyboard: Ctrl+C, Ctrl+D, Tab, Esc, F-keys.
                    match &mut self.terminal {
                        Some(t) if t.exited().is_none() => t.key(key),
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
                        }
                        self.documents[self.active].handle(Event::Key(key));
                        if let Some(command) = self.documents[self.active].take_host_command() {
                            self.host_command(&command)?;
                        }
                        self.after_editor_input();
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
                        KeyCode::Delete => self.command("delete")?,
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
            Event::Paste(text) => {
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
        if self.assistant.suggestion.is_some() {
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
                    "AI: {} | inline suggestions: {} (ai.inline)",
                    self.assistant.status(),
                    if self.assistant.inline_enabled() {
                        "on"
                    } else {
                        "off"
                    }
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
            Panel::Help => help_lines(&self.config.keys),
            Panel::Terminal => self.terminal.as_ref().map_or_else(
                || vec!["Alt+3 starts your shell here".into()],
                |t| t.contents().lines().map(str::to_string).collect(),
            ),
        }
    }
    fn draw_terminal(&mut self, width: usize, height: usize) -> (Vec<Row>, Option<(u16, u16)>) {
        let focused = self.focus == Focus::Terminal;
        let (title, mut body, cursor) = match &mut self.terminal {
            Some(t) => {
                let state = match t.exited() {
                    Some(why) => format!("shell ended ({why}); Enter restarts"),
                    None => t.shell().to_string(),
                };
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
                    "keys go to the shell; Alt+3 back to editor"
                } else {
                    "Alt+3 focus"
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
    /// Show a pending AI completion at the cursor without touching the buffer.
    fn ghost_text(&self, frame: &mut Frame, width: usize) {
        let Some(s) = &self.assistant.suggestion else {
            return;
        };
        let state = self.editor().state();
        if s.offset != state.cursor() || s.path.as_deref() != state.path() {
            return;
        }
        let Some((x, y)) = frame.cursor else { return };
        let Some(row) = frame.rows.get_mut(usize::from(y)) else {
            return;
        };
        let mut lines = s.text.lines();
        let first = lines.next().unwrap_or("");
        let more = lines.count();
        let mut ghost = first.replace('\t', "    ");
        if more > 0 {
            ghost.push_str(&format!(" … (+{more} lines)"));
        }
        let text = sanitize(&ghost);
        *row = insert_at(row, usize::from(x), &text, width);
    }
    /// Compose an adaptive bounded frame, reusing the editor's cached grapheme layout.
    pub fn frame(&mut self) -> Result<Frame, FrontendError> {
        let width = usize::from(self.size.0.min(1024)).saturating_sub(1);
        let height = usize::from(self.size.1.min(256));
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
                "EmbedIDE 2.0 | {} | {project} | {target} | {}",
                self.build_status, self.profile
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
        if editor_height > 0 {
            if overlay_tree {
                rows.extend(self.draw_tree(width, editor_height));
            } else if overlay_ai {
                let (ai, ai_cursor) = self.assistant.rows(width, editor_height, true);
                cursor = ai_cursor.map(|(x, y)| (x, y + 2));
                rows.extend(ai);
            } else {
                let left = if side_width > 0 { side_width + 1 } else { 0 };
                let right = if ai_width > 0 { ai_width + 1 } else { 0 };
                let editor_width = width.saturating_sub(left + right);
                self.documents[self.active].size =
                    ((editor_width + 1) as u16, editor_height as u16);
                let mut frame = self.documents[self.active].frame()?;
                self.ghost_text(&mut frame, editor_width);
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
/// Run a project workspace. Startup performs local metadata and one directory read only.
pub fn run(root: &Path) -> Result<(), FrontendError> {
    if !(io::stdin().is_terminal() && io::stdout().is_terminal()) {
        return Err(FrontendError::NeedsTerminal);
    }
    if std::env::var_os("TERM").is_some_and(|term| term == "dumb") {
        return Err(service_error("TERM=dumb cannot run workspace TUI"));
    }
    let (width, height) = terminal::size()?;
    let mut app = Workspace::open(root, width, height)?;
    let guard = guard::TerminalGuard::enter()?;
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
    app.operation = None;
    app.serial = None;
    app.debug = None;
    app.terminal = None;
    app.assistant.cancel();
    Ok(())
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
        _ => "text",
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
fn insert_at(row: &Row, col: usize, text: &str, width: usize) -> Row {
    use unicode_width::UnicodeWidthChar;
    fn put(out: &mut Row, used: &mut usize, text: &str, selected: bool, width: usize) {
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
            out.push(&piece, selected);
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
                    out.push(&std::mem::take(&mut piece), run.selected);
                }
                put(&mut out, &mut used, text, true, width);
                inserted = true;
            }
            let w = ch.width().unwrap_or(0);
            if used + w > width {
                if !piece.is_empty() {
                    out.push(&piece, run.selected);
                }
                break 'runs;
            }
            piece.push(ch);
            used += w;
        }
        if !piece.is_empty() {
            out.push(&piece, run.selected);
        }
    }
    if !inserted && !text.is_empty() {
        if used < col {
            let gap = " ".repeat(col - used);
            put(&mut out, &mut used, &gap, false, width);
        }
        put(&mut out, &mut used, text, true, width);
    }
    out
}

fn help_lines(keys: &BTreeMap<String, String>) -> Vec<String> {
    let mut lines: Vec<String> = [
        "PANELS  Alt+1 explorer | Alt+2 AI assistant | Alt+3 terminal (press again to return to the editor)",
        "        F2 show/hide explorer | F4 cycle bottom panels | F6 cycle focus | Ctrl+K command palette",
        "EDITOR  Vim modes: Normal (hjkl w b e 0 $ gg G x dd yy p u Ctrl+R / ? n N v V), Insert (i a o O, Esc),",
        "        Command (:w :q :q! :wq, plus :build :clean :cancel :e <file> :help :settings :complete :ai <question>)",
        "        Leader (Space): e explorer | b build | d debug | t terminal | s serial | f find file",
        "        Ctrl+S save | Ctrl+W close document | F10 next document | Ctrl+P find file | Ctrl+Q quit",
        "BUILD   F5 build | F7 cargo check | F8 cancel | F3 problems: j/k select, Enter jumps to the source line",
        "        Compilers come from embedide.toml and config.toml (Settings panel); nothing is installed for you",
        "TERMINAL Alt+3: your shell in a real pseudo-terminal; all keys go to it except Alt+1/2/3",
        "AI      Alt+2, type a message, Enter sends (only then). /file /sel /explain /edit <instr> /apply /reject",
        "        /complete or :complete shows a suggestion at the cursor: Tab accepts, Esc rejects. /help in the panel",
        "        AI is optional: without a provider the IDE works fully offline. EMBEDIDE_AI_OFFLINE=1 blocks requests",
        "EXPLORER j/k move, Enter/Right open or expand, Left collapse, n new file, Delete delete, r refresh, Esc editor",
        "PANEL   j/k scroll, PgUp/PgDn, End follow, / filter, Tab next panel, Esc editor",
        "Debugging, flashing and serial run only from explicit commands (Ctrl+K: debug, flash, serial).",
    ]
    .iter()
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

    #[test]
    fn panel_keys_toggle_focus_and_ex_commands_reach_the_host() {
        let dir = std::env::temp_dir().join(format!("embed-ws-unit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.c"), "int a;\n").unwrap();
        let mut ws = Workspace::open(&dir, 120, 40).unwrap();
        let alt = |c| Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT));
        ws.handle(alt('2'));
        assert_eq!(ws.focus(), Focus::Assistant);
        ws.handle(alt('2'));
        assert_eq!(ws.focus(), Focus::Editor);
        ws.handle(alt('1'));
        assert_eq!(ws.focus(), Focus::Explorer);
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
