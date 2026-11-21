//! What an agent may do without asking. EmbedIDE answers the permission
//! requests of the official CLI (Claude Code sends one per tool call that
//! its own rules do not already allow); these rules decide which ones are
//! answered automatically and which ones go to the user. They only ever
//! narrow what the CLI allows: nothing here turns off a CLI's own checks or
//! sandbox, and skill text never reaches this code.
use serde_json::Value;
use std::{
    fmt,
    path::{Component, Path, PathBuf},
    str::FromStr,
};

/// Agent permission policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Policy {
    /// Read and search only; nothing is changed or run (except read-only
    /// inspection commands).
    ReadOnly,
    /// Ordinary development in a trusted workspace: edit files inside it and
    /// run build/test commands without asking; destructive, privileged,
    /// network, hardware and out-of-workspace actions need approval.
    #[default]
    WorkspaceWrite,
    /// Every change and every command that is not a plain read needs approval.
    ApprovalRequired,
}

impl Policy {
    /// All policies.
    pub const ALL: [Policy; 3] = [
        Policy::ReadOnly,
        Policy::WorkspaceWrite,
        Policy::ApprovalRequired,
    ];
    /// Configuration name.
    pub fn id(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::WorkspaceWrite => "workspace-write",
            Self::ApprovalRequired => "approval-required",
        }
    }
    /// One-line description.
    pub fn describe(self) -> &'static str {
        match self {
            Self::ReadOnly => "read and search only; no file changes, no commands except read-only inspection",
            Self::WorkspaceWrite => "edits inside the workspace and build/test commands run without asking; destructive, privileged, network, hardware and out-of-workspace actions ask first",
            Self::ApprovalRequired => "every file change and every command (except plain reads) asks first",
        }
    }
}

impl fmt::Display for Policy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}

impl FromStr for Policy {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        Self::ALL
            .into_iter()
            .find(|p| p.id() == s.trim())
            .ok_or_else(|| {
                format!("Unknown policy '{s}': use read-only, workspace-write or approval-required")
            })
    }
}

/// The answer to one permission request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Allowed without asking.
    Allow,
    /// Needs the user's decision; the reason is shown to them.
    Ask(String),
    /// Refused; the reason is returned to the agent.
    Deny(String),
}

/// What a command does, as far as the rules can tell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandClass {
    /// Only reads (listing, searching, `git status`).
    Read,
    /// Ordinary development: builds, tests, running the project's programs,
    /// creating/moving files inside the workspace.
    Dev,
    /// Needs approval, with the reason.
    Sensitive(String),
}

/// Where a path points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathClass {
    /// Inside the workspace.
    Inside,
    /// Inside, but a file that should not change or be read silently
    /// (`.git` internals, `.env`, keys).
    Sensitive(String),
    /// Outside the workspace.
    Outside,
}

/// Permission rules for one workspace.
#[derive(Debug, Clone)]
pub struct Rules {
    /// Policy.
    pub policy: Policy,
    workspace: PathBuf,
    allow: Vec<String>,
}

const READ_COMMANDS: &[&str] = &[
    "ls",
    "cat",
    "head",
    "tail",
    "wc",
    "grep",
    "egrep",
    "fgrep",
    "rg",
    "file",
    "stat",
    "du",
    "df",
    "tree",
    "pwd",
    "echo",
    "printf",
    "which",
    "type",
    "true",
    "false",
    "test",
    "[",
    "sort",
    "uniq",
    "cut",
    "tr",
    "diff",
    "cmp",
    "md5sum",
    "sha1sum",
    "sha256sum",
    "readelf",
    "objdump",
    "nm",
    "size",
    "strings",
    "hexdump",
    "xxd",
    "od",
    "realpath",
    "dirname",
    "basename",
    "date",
    "uname",
    "column",
    "less",
    "more",
    "nl",
    "fold",
    "comm",
    "jq",
    "sleep",
];

const DEV_COMMANDS: &[&str] = &[
    "make",
    "gmake",
    "bmake",
    "cmake",
    "ctest",
    "ninja",
    "meson",
    "rustc",
    "rustfmt",
    "gcc",
    "g++",
    "cc",
    "c++",
    "clang",
    "clang++",
    "cpp",
    "ld",
    "ld.lld",
    "lld",
    "as",
    "nasm",
    "yasm",
    "ar",
    "ranlib",
    "objcopy",
    "strip",
    "clang-format",
    "clang-tidy",
    "cppcheck",
    "ctags",
    "bear",
    "valgrind",
    "mkdir",
    "touch",
    "cp",
    "mv",
    "ln",
    "awk",
    "gawk",
    "sed",
    "patch",
    "tee",
    "xz",
    "gzip",
    "gunzip",
    "tar",
    "unzip",
    "zip",
    "python",
    "python3",
    "pytest",
    "gdb",
    "lldb",
    "chmod",
    "pkg-config",
    "sdcc",
    "iverilog",
    "verilator",
    "ghdl",
    "gfortran",
    "go",
    "addr2line",
    "c++filt",
];

const CROSS_TOOLS: &[&str] = &[
    "gcc",
    "g++",
    "as",
    "ld",
    "objdump",
    "objcopy",
    "size",
    "nm",
    "readelf",
    "gdb",
    "strip",
    "ar",
    "ranlib",
    "addr2line",
    "cpp",
];

const HARDWARE: &[&str] = &[
    "openocd",
    "st-flash",
    "st-util",
    "avrdude",
    "esptool",
    "esptool.py",
    "dfu-util",
    "picotool",
    "probe-rs",
    "pyocd",
    "nrfjprog",
    "JLinkExe",
    "JLinkGDBServer",
    "stm32flash",
    "teensy_loader_cli",
    "minicom",
    "screen",
    "picocom",
    "stty",
    "idf.py",
    "uf2conv.py",
    "bossac",
    "flashrom",
];

const NETWORK: &[&str] = &[
    "curl", "wget", "ssh", "scp", "sftp", "rsync", "nc", "ncat", "netcat", "telnet", "ftp", "pip",
    "pip3", "pipx", "npm", "npx", "pnpm", "yarn", "apt", "apt-get", "pacman", "dnf", "yum", "brew",
    "docker", "podman", "gh", "rustup", "conda", "uv",
];

const PRIVILEGE: &[&str] = &[
    "sudo",
    "su",
    "doas",
    "pkexec",
    "chown",
    "chgrp",
    "mount",
    "umount",
    "systemctl",
    "service",
    "kill",
    "pkill",
    "killall",
    "reboot",
    "shutdown",
    "modprobe",
    "insmod",
    "rmmod",
    "setcap",
    "crontab",
    "useradd",
    "passwd",
];

const DESTRUCTIVE: &[&str] = &[
    "rm", "rmdir", "shred", "dd", "truncate", "wipefs", "fdisk", "parted", "unlink",
];

const SHELLS: &[&str] = &[
    "bash", "sh", "zsh", "fish", "dash", "ksh", "eval", "exec", "source", ".", "xargs", "nohup",
    "setsid", "script", "expect", "perl", "ruby", "node", "env", "printenv", "export",
];

const WRAPPERS: &[&str] = &["time", "nice", "timeout", "command", "stdbuf", "ionice"];

/// A shell token: a word (quotes removed) or an operator.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Word(String),
    /// `&&`, `||`, `;`, `|`, `&`, newline.
    Sep,
    /// `>`, `>>`, `2>`, `&>`, `<` (the next word is a path).
    Redirect(bool),
}

/// Split a command line into words like a POSIX shell would (no expansion).
/// `None` when it uses features the rules cannot see through (command
/// substitution, process substitution, unterminated quotes, here-documents).
fn tokenize(cmd: &str) -> Option<Vec<Token>> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = cmd.chars().peekable();
    let flush = |word: &mut String, in_word: &mut bool, out: &mut Vec<Token>| {
        if *in_word {
            out.push(Token::Word(std::mem::take(word)));
            *in_word = false;
        }
    };
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_word = true;
                loop {
                    match chars.next()? {
                        '\'' => break,
                        c => word.push(c),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next()? {
                        '"' => break,
                        '\\' => word.push(chars.next()?),
                        '`' => return None,
                        '$' if chars.peek() == Some(&'(') => return None,
                        c => word.push(c),
                    }
                }
            }
            '\\' => {
                in_word = true;
                match chars.next() {
                    Some('\n') | None => {}
                    Some(c) => word.push(c),
                }
            }
            '`' => return None,
            '$' if chars.peek() == Some(&'(') => return None,
            '<' | '>' if chars.peek() == Some(&'(') => return None,
            ' ' | '\t' => flush(&mut word, &mut in_word, &mut out),
            '\n' | ';' => {
                flush(&mut word, &mut in_word, &mut out);
                out.push(Token::Sep);
            }
            '&' | '|' => {
                // `2>&1`: a descriptor duplication, not a separator
                if c == '&' && matches!(out.last(), Some(Token::Redirect(_))) && !in_word {
                    while chars
                        .peek()
                        .is_some_and(|c| c.is_ascii_digit() || *c == '-')
                    {
                        chars.next();
                    }
                    out.pop();
                    continue;
                }
                if c == '&' && chars.peek() == Some(&'>') {
                    flush(&mut word, &mut in_word, &mut out);
                    chars.next();
                    chars.next_if_eq(&'>');
                    out.push(Token::Redirect(true));
                    continue;
                }
                flush(&mut word, &mut in_word, &mut out);
                chars.next_if_eq(&c);
                out.push(Token::Sep);
            }
            '>' | '<' => {
                if c == '<' && chars.peek() == Some(&'<') {
                    return None; // here-document / here-string
                }
                // `2>` / `1>`: drop the descriptor number
                if in_word && word.chars().all(|d| d.is_ascii_digit()) {
                    word.clear();
                    in_word = false;
                }
                flush(&mut word, &mut in_word, &mut out);
                if c == '>' {
                    chars.next_if_eq(&'>');
                    chars.next_if_eq(&'|');
                }
                if chars.peek() == Some(&'&') {
                    // `>&2`
                    chars.next();
                    while chars
                        .peek()
                        .is_some_and(|c| c.is_ascii_digit() || *c == '-')
                    {
                        chars.next();
                    }
                    continue;
                }
                out.push(Token::Redirect(c == '>'));
            }
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    flush(&mut word, &mut in_word, &mut out);
    Some(out)
}

/// Lexically normalise `path` against `base` (no filesystem access).
fn normalize(base: &Path, path: &Path) -> PathBuf {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    };
    let mut out = PathBuf::new();
    for c in joined.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// Resolve symlinks of the longest existing ancestor (a new file does not
/// exist yet), then append the rest.
fn resolve(path: &Path) -> PathBuf {
    let mut existing = path.to_path_buf();
    let mut rest = Vec::new();
    loop {
        if let Ok(real) = existing.canonicalize() {
            let mut p = real;
            for r in rest.iter().rev() {
                p.push(r);
            }
            return p;
        }
        match (
            existing.file_name().map(|n| n.to_os_string()),
            existing.parent(),
        ) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                existing = parent.to_path_buf();
            }
            _ => return path.to_path_buf(),
        }
    }
}

fn looks_like_path(word: &str) -> bool {
    word.starts_with('/')
        || word.starts_with('~')
        || word == ".."
        || word.starts_with("../")
        || word.contains("/../")
        || word.ends_with("/..")
}

impl Rules {
    /// Rules for `workspace` (made absolute and resolved). `allow` adds
    /// command names that count as ordinary development.
    pub fn new(policy: Policy, workspace: &Path, allow: &[String]) -> Self {
        let workspace = resolve(&normalize(
            &std::env::current_dir().unwrap_or_default(),
            workspace,
        ));
        Self {
            policy,
            workspace,
            allow: allow.to_vec(),
        }
    }

    /// The workspace root.
    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    /// Classify a path given by an agent (relative paths are relative to `cwd`).
    pub fn path(&self, cwd: &Path, path: &str) -> PathClass {
        if path.starts_with('~') {
            return PathClass::Outside;
        }
        let p = resolve(&normalize(cwd, Path::new(path)));
        let Ok(rel) = p.strip_prefix(&self.workspace) else {
            return PathClass::Outside;
        };
        let names: Vec<String> = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        if names.iter().any(|n| n == ".git") {
            return PathClass::Sensitive("Git internals (.git)".into());
        }
        if let Some(name) = names.last() {
            let lower = name.to_ascii_lowercase();
            if lower == ".env" || lower.starts_with(".env.") {
                return PathClass::Sensitive(format!("{name} usually holds secrets"));
            }
            if [".pem", ".key", ".p12", ".pfx", ".keystore"]
                .iter()
                .any(|e| lower.ends_with(e))
                || lower.starts_with("id_rsa")
                || lower.starts_with("id_ed25519")
                || lower == "credentials.toml"
                || lower == ".netrc"
            {
                return PathClass::Sensitive(format!("{name} looks like a key or credential"));
            }
        }
        PathClass::Inside
    }

    fn path_issue(&self, cwd: &Path, word: &str) -> Option<String> {
        if ["/dev/null", "/dev/stdout", "/dev/stderr", "/dev/zero"].contains(&word) {
            return None;
        }
        match self.path(cwd, word) {
            PathClass::Inside => None,
            PathClass::Sensitive(why) => Some(why),
            PathClass::Outside => Some(format!("{word} is outside the workspace")),
        }
    }

    /// Why an argument word needs approval: a path outside the workspace, a
    /// sensitive file (`.env`, `.git`, keys, or a symlink escaping the
    /// workspace), or a path built from a variable the rules cannot resolve.
    fn word_issue(&self, cwd: &Path, word: &str) -> Option<String> {
        if word.contains("$HOME") || word.contains("${HOME") {
            return Some(format!("{word} points into the home directory"));
        }
        if word.contains('$') && word.contains('/') {
            return Some(format!("{word} is a path built from a variable"));
        }
        if word.is_empty() {
            return None;
        }
        if looks_like_path(word) || !word.starts_with('-') {
            return self.path_issue(cwd, word);
        }
        None
    }

    /// Classify a shell command line.
    pub fn command(&self, cmd: &str) -> CommandClass {
        let Some(tokens) = tokenize(cmd) else {
            return CommandClass::Sensitive(
                "uses command substitution, a here-document or unbalanced quotes".into(),
            );
        };
        let mut cwd = self.workspace.clone();
        let mut worst = CommandClass::Read;
        for segment in tokens.split(|t| *t == Token::Sep) {
            let mut words = Vec::new();
            let mut iter = segment.iter();
            while let Some(t) = iter.next() {
                match t {
                    Token::Word(w) => words.push(w.clone()),
                    Token::Redirect(write) => {
                        let Some(Token::Word(target)) = iter.next() else {
                            raise(
                                CommandClass::Sensitive("incomplete redirection".into()),
                                &mut worst,
                            );
                            continue;
                        };
                        if target == "/dev/null" {
                            continue;
                        }
                        if let Some(why) = self.path_issue(&cwd, target) {
                            raise(CommandClass::Sensitive(why), &mut worst);
                        } else if *write {
                            raise(CommandClass::Dev, &mut worst);
                        }
                    }
                    Token::Sep => {}
                }
            }
            let c = self.segment(&mut cwd, &words);
            raise(c, &mut worst);
        }
        worst
    }

    fn segment(&self, cwd: &mut PathBuf, words: &[String]) -> CommandClass {
        let mut i = 0;
        // Shell grammar around commands: `for x in a b; do …; done`, `if …; then …; fi`
        while let Some(w) = words.get(i) {
            match w.as_str() {
                "do" | "then" | "else" | "elif" | "if" | "while" | "until" | "!" | "{" | "time" => {
                    i += 1
                }
                "done" | "fi" | "}" if words.len() == i + 1 => return CommandClass::Read,
                "for" | "select" => {
                    // The loop list is data, but it must not name outside paths.
                    for a in words.iter().skip(i + 1) {
                        if let Some(why) = self.word_issue(cwd, a) {
                            return CommandClass::Sensitive(why);
                        }
                    }
                    return CommandClass::Read;
                }
                "case" | "function" | "(" | "((" | "[[" => {
                    return CommandClass::Sensitive(format!("shell '{w}' construct"))
                }
                _ => break,
            }
        }
        // Leading `NAME=value` assignments
        while let Some(w) = words.get(i) {
            let Some((name, _)) = w.split_once('=') else {
                break;
            };
            if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                break;
            }
            if name.starts_with("LD_")
                || name.starts_with("DYLD_")
                || [
                    "PATH",
                    "HOME",
                    "SHELL",
                    "IFS",
                    "BASH_ENV",
                    "ENV",
                    "COMPILER_PATH",
                    "GCC_EXEC_PREFIX",
                    "PYTHONPATH",
                    "PYTHONSTARTUP",
                    "NODE_OPTIONS",
                    "PERL5OPT",
                    "RUSTC_WRAPPER",
                    "GIT_SSH_COMMAND",
                ]
                .contains(&name)
            {
                return CommandClass::Sensitive(format!("changes {name}"));
            }
            i += 1;
        }
        // Harmless wrappers: classify what they run
        while let Some(w) = words.get(i) {
            if !WRAPPERS.contains(&w.as_str()) {
                break;
            }
            i += 1;
            while words.get(i).is_some_and(|a| {
                a.starts_with('-')
                    || (w == "timeout" && a.chars().next().is_some_and(|c| c.is_ascii_digit()))
            }) {
                i += 1;
            }
        }
        let Some(program) = words.get(i) else {
            return CommandClass::Read;
        };
        let args = &words[i + 1..];
        let name = program.rsplit('/').next().unwrap_or(program).to_string();
        // Paths in arguments (also `-I/x`, `--out=/x`, `-o/x`)
        for a in args {
            let candidate = if a.starts_with('-') {
                match (a.find('='), a.find('/')) {
                    (Some(eq), _) => &a[eq + 1..],
                    (None, Some(slash)) if slash <= 3 => &a[slash..],
                    _ => continue,
                }
            } else {
                a.as_str()
            };
            if let Some(why) = self.word_issue(cwd, candidate) {
                return CommandClass::Sensitive(why);
            }
        }
        if name == "cd" || name == "pushd" {
            return match args.first() {
                Some(dir) => match self.path_issue(cwd, dir) {
                    Some(why) => CommandClass::Sensitive(why),
                    None => {
                        *cwd = normalize(cwd, Path::new(dir));
                        CommandClass::Read
                    }
                },
                None => CommandClass::Sensitive("cd to the home directory".into()),
            };
        }
        let has = |flag: &str| args.iter().any(|a| a == flag);
        let sub = args
            .iter()
            .find(|a| !a.starts_with('-'))
            .map(String::as_str);
        if PRIVILEGE.contains(&name.as_str()) {
            return CommandClass::Sensitive(format!(
                "{name} needs privileges or affects other processes"
            ));
        }
        if DESTRUCTIVE.contains(&name.as_str()) || name.starts_with("mkfs") {
            return CommandClass::Sensitive(format!("{name} deletes or overwrites data"));
        }
        if NETWORK.contains(&name.as_str()) {
            return CommandClass::Sensitive(format!(
                "{name} uses the network or installs software"
            ));
        }
        if HARDWARE.contains(&name.as_str()) {
            return CommandClass::Sensitive(format!("{name} talks to connected hardware"));
        }
        if SHELLS.contains(&name.as_str()) {
            if (name == "env" || name == "printenv") && args.is_empty() {
                return CommandClass::Sensitive(
                    "prints the environment (it may hold secrets)".into(),
                );
            }
            return CommandClass::Sensitive(format!(
                "{name} runs commands the rules cannot inspect"
            ));
        }
        match name.as_str() {
            "git" => return git(args),
            "embed" => return embed_command(args),
            "cargo" => {
                return match sub {
                    Some(
                        "build" | "b" | "check" | "c" | "test" | "t" | "run" | "r" | "clippy"
                        | "fmt" | "doc" | "bench" | "clean" | "tree" | "metadata" | "expand"
                        | "nextest",
                    ) => CommandClass::Dev,
                    Some("--version" | "version") | None => CommandClass::Read,
                    Some(s) => CommandClass::Sensitive(format!(
                        "cargo {s} may use the network or change global state"
                    )),
                }
            }
            "zig" => {
                return match sub {
                    Some(
                        "build" | "test" | "run" | "fmt" | "cc" | "c++" | "ast-check"
                        | "translate-c" | "build-exe" | "build-obj" | "build-lib" | "objcopy"
                        | "ar" | "env" | "version" | "targets",
                    ) => CommandClass::Dev,
                    Some(s) => CommandClass::Sensitive(format!("zig {s} may use the network")),
                    None => CommandClass::Read,
                }
            }
            "find" => {
                if args.iter().any(|a| {
                    [
                        "-delete", "-exec", "-execdir", "-ok", "-okdir", "-fprint", "-fprintf",
                        "-fls",
                    ]
                    .contains(&a.as_str())
                }) {
                    return CommandClass::Sensitive("find with -delete/-exec".into());
                }
                return CommandClass::Read;
            }
            "sed"
                if !args
                    .iter()
                    .any(|a| a.starts_with("-i") || a == "--in-place") =>
            {
                return CommandClass::Read
            }
            "python" | "python3" if has("-m") => {
                let module = args
                    .iter()
                    .skip_while(|a| *a != "-m")
                    .nth(1)
                    .map(String::as_str);
                return match module {
                    Some("pytest" | "unittest" | "py_compile" | "json.tool") => CommandClass::Dev,
                    Some(m) => CommandClass::Sensitive(format!("python -m {m}")),
                    None => CommandClass::Sensitive("python -m".into()),
                };
            }
            "chmod" if has("-R") => return CommandClass::Sensitive("recursive chmod".into()),
            _ => {}
        }
        if READ_COMMANDS.contains(&name.as_str()) {
            return CommandClass::Read;
        }
        if DEV_COMMANDS.contains(&name.as_str()) || self.allow.contains(&name) {
            return CommandClass::Dev;
        }
        // Cross toolchains: arm-none-eabi-gcc, riscv64-unknown-elf-objdump, …
        if let Some((_, tool)) = name.rsplit_once('-') {
            if name.matches('-').count() >= 2 && CROSS_TOOLS.contains(&tool) {
                return CommandClass::Dev;
            }
        }
        if name.starts_with("qemu-") {
            return CommandClass::Dev;
        }
        // A program inside the workspace (`./app`, `build/test_runner`)
        if program.contains('/') && self.path_issue(cwd, program).is_none() {
            return CommandClass::Dev;
        }
        CommandClass::Sensitive(format!("{name} is not on the development allow list"))
    }

    /// Decide a Claude Code tool call (`tool` name and its JSON input).
    pub fn tool(&self, tool: &str, input: &Value) -> Verdict {
        let cwd = self.workspace.clone();
        let path_of = |keys: &[&str]| {
            keys.iter()
                .find_map(|k| input.get(*k).and_then(Value::as_str))
                .map(str::to_string)
        };
        match tool {
            "Read" | "Glob" | "Grep" | "LS" | "NotebookRead" => {
                let Some(p) = path_of(&["file_path", "path", "notebook_path"]) else {
                    return Verdict::Allow;
                };
                match self.path(&cwd, &p) {
                    PathClass::Inside => Verdict::Allow,
                    PathClass::Sensitive(why) => Verdict::Ask(format!("read {p}: {why}")),
                    PathClass::Outside => Verdict::Ask(format!("read {p}: outside the workspace")),
                }
            }
            "TodoWrite" | "TodoRead" | "Task" | "Agent" | "ToolSearch" => Verdict::Allow,
            "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => {
                let p = path_of(&["file_path", "notebook_path", "path"]).unwrap_or_default();
                if self.policy == Policy::ReadOnly {
                    return Verdict::Deny("The read-only policy allows no file changes".into());
                }
                match self.path(&cwd, &p) {
                    PathClass::Outside => {
                        Verdict::Ask(format!("change {p}: outside the workspace"))
                    }
                    PathClass::Sensitive(why) => Verdict::Ask(format!("change {p}: {why}")),
                    PathClass::Inside if self.policy == Policy::ApprovalRequired => {
                        Verdict::Ask(format!("change {p}"))
                    }
                    PathClass::Inside => Verdict::Allow,
                }
            }
            "Bash" | "BashOutput" | "PowerShell" => {
                let cmd = input
                    .get("command")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if tool == "BashOutput" {
                    return Verdict::Allow;
                }
                match (self.command(cmd), self.policy) {
                    (CommandClass::Read, _) => Verdict::Allow,
                    (_, Policy::ReadOnly) => Verdict::Deny(
                        "The read-only policy allows only read-only inspection commands".into(),
                    ),
                    (CommandClass::Dev, Policy::WorkspaceWrite) => Verdict::Allow,
                    (CommandClass::Dev, _) => Verdict::Ask("run a command".into()),
                    (CommandClass::Sensitive(why), _) => Verdict::Ask(why),
                }
            }
            "KillShell" | "KillBash" => Verdict::Allow,
            "AskUserQuestion" => Verdict::Deny(
                "EmbedIDE shows no interactive questions; ask in your reply and stop".into(),
            ),
            "WebFetch" | "WebSearch" => match self.policy {
                Policy::ReadOnly => {
                    Verdict::Deny("The read-only policy allows no network access".into())
                }
                _ => Verdict::Ask(format!("{tool} uses the network")),
            },
            other => match self.policy {
                Policy::ReadOnly => {
                    Verdict::Deny(format!("The read-only policy does not allow {other}"))
                }
                _ => Verdict::Ask(format!(
                    "{other} is not a file or command tool EmbedIDE knows"
                )),
            },
        }
    }
    /// Scoped key for "allow similar" decisions. Returns `None` for requests
    /// that must always be approved individually: out-of-workspace or sensitive
    /// file accesses, compound/redirected shell pipelines, and privileged,
    /// shell, network or hardware commands.
    pub fn similar_key(&self, tool: &str, input: &Value) -> Option<String> {
        let cwd = self.workspace.clone();
        let path_of = |keys: &[&str]| {
            keys.iter()
                .find_map(|k| input.get(*k).and_then(Value::as_str))
                .map(str::to_string)
        };
        match tool {
            "Read" | "Glob" | "Grep" | "LS" | "NotebookRead" | "Edit" | "Write" | "MultiEdit"
            | "NotebookEdit" => {
                let p = path_of(&["file_path", "notebook_path", "path"])?;
                matches!(self.path(&cwd, &p), PathClass::Inside).then(|| format!("{tool}:inside"))
            }
            "Bash" => {
                let cmd = input.get("command").and_then(Value::as_str).unwrap_or("");
                let tokens = tokenize(cmd)?;
                if tokens
                    .iter()
                    .any(|t| matches!(t, Token::Sep | Token::Redirect(_)))
                {
                    return None;
                }
                let words: Vec<String> = tokens
                    .into_iter()
                    .filter_map(|t| match t {
                        Token::Word(w) => Some(w),
                        _ => None,
                    })
                    .collect();
                let mut i = 0;
                while let Some(w) = words.get(i) {
                    if let Some((name, _)) = w.split_once('=') {
                        if !name.is_empty()
                            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                        {
                            i += 1;
                            continue;
                        }
                    }
                    break;
                }
                let program = words.get(i)?;
                let args = &words[i + 1..];
                let name = program.rsplit('/').next().unwrap_or(program);
                if PRIVILEGE.contains(&name)
                    || SHELLS.contains(&name)
                    || NETWORK.contains(&name)
                    || HARDWARE.contains(&name)
                    || WRAPPERS.contains(&name)
                {
                    return None;
                }
                for a in args {
                    let candidate = if a.starts_with('-') {
                        match (a.find('='), a.find('/')) {
                            (Some(eq), _) => &a[eq + 1..],
                            (None, Some(slash)) if slash <= 3 => &a[slash..],
                            _ => continue,
                        }
                    } else {
                        a.as_str()
                    };
                    if self.word_issue(&cwd, candidate).is_some() {
                        return None;
                    }
                }
                match self.command(cmd) {
                    CommandClass::Read => Some(format!("Bash:read:{name}")),
                    CommandClass::Dev => Some(format!("Bash:dev:{name}")),
                    CommandClass::Sensitive(_) => Some(format!("Bash:sensitive:{name}")),
                }
            }
            other => Some(other.to_string()),
        }
    }
}

/// Keep the most restrictive class.
fn raise(c: CommandClass, worst: &mut CommandClass) {
    let rank = |c: &CommandClass| match c {
        CommandClass::Read => 0,
        CommandClass::Dev => 1,
        CommandClass::Sensitive(_) => 2,
    };
    if rank(&c) > rank(worst) {
        *worst = c;
    }
}

/// EmbedIDE's own command line: building and inspecting is ordinary development; anything that changes
/// global configuration, starts a shell or talks to a provider asks.
fn embed_command(args: &[String]) -> CommandClass {
    let mut words = args
        .iter()
        .filter(|a| !a.starts_with('-'))
        .map(String::as_str);
    match (words.next(), words.next()) {
        (Some("build" | "clean"), _) => CommandClass::Dev,
        (Some("doctor" | "targets" | "toolchains"), _) | (None, _) => CommandClass::Read,
        (Some("toolchain"), Some("list" | "detect" | "info" | "recommend")) => CommandClass::Read,
        (Some("project"), Some("info" | "templates")) => CommandClass::Read,
        (Some("project"), Some("init")) => CommandClass::Dev,
        (Some("config"), Some("list" | "get" | "path")) => CommandClass::Read,
        (Some(other), _) => CommandClass::Sensitive(format!(
            "embed {other} changes configuration, starts a shell or contacts a provider"
        )),
    }
}

fn git(args: &[String]) -> CommandClass {
    for a in args {
        if matches!(
            a.as_str(),
            "-c" | "--config-env"
                | "--exec-path"
                | "--git-dir"
                | "--work-tree"
                | "--upload-pack"
                | "--receive-pack"
                | "--ext-diff"
        ) || a.starts_with("-c") && a.len() > 2 && !a.starts_with("--")
            || [
                "--config-env=",
                "--exec-path=",
                "--git-dir=",
                "--work-tree=",
                "--upload-pack=",
                "--receive-pack=",
            ]
            .iter()
            .any(|p| a.starts_with(p))
        {
            return CommandClass::Sensitive(format!(
                "git option '{a}' can run commands or change the repository location"
            ));
        }
    }
    let sub = args
        .iter()
        .find(|a| !a.starts_with('-'))
        .map(String::as_str)
        .unwrap_or("");
    let has = |f: &str| args.iter().any(|a| a == f);
    match sub {
        "status" | "diff" | "log" | "show" | "blame" | "rev-parse" | "ls-files" | "grep"
        | "describe" | "shortlog" | "reflog" | "cat-file" | "ls-tree" | "whatchanged"
        | "version" | "help" => CommandClass::Read,
        "branch" | "tag" | "remote"
            if args.len() == 1 || has("--list") || has("-l") || has("-v") || has("-a") =>
        {
            CommandClass::Read
        }
        "stash" if args.get(1).is_some_and(|a| a == "list" || a == "show") => CommandClass::Read,
        "add" | "init" | "mv" | "apply" | "format-patch" => CommandClass::Dev,
        "" => CommandClass::Read,
        other => CommandClass::Sensitive(format!(
            "git {other} changes history, discards work or uses the network"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rules(policy: Policy) -> (Rules, PathBuf) {
        let dir = std::env::temp_dir().join(format!("embedide-policy-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        (Rules::new(policy, &dir, &["just".into()]), dir)
    }

    #[test]
    fn ordinary_development_commands_are_allowed() {
        let (r, _) = rules(Policy::WorkspaceWrite);
        for cmd in [
            "gcc -Wall -o app main.c add.c && ./app",
            "make -j8 2>&1 | tail -20",
            "cargo test --workspace",
            "zig build test",
            "arm-none-eabi-gcc -mcpu=cortex-m4 -c start.s -o build/start.o",
            "cd src && cc -c x.c",
            "mkdir -p build; mv a.c src/a.c",
            "python3 -m pytest -q",
            "just test",
            "embed build . 2>&1 | tail -20; timeout 10 ./build/debug/ringbuf",
            "embed doctor",
            "embed project info .",
            "embed clean --dry-run",
            "FOO=1 make",
            "timeout 30 ./build/tests",
            "nasm -f elf64 hello.asm -o hello.o > /dev/null",
            "ls -la && for f in *.c *.h; do echo \"== $f\"; cat \"$f\"; done",
            "if [ -f Makefile ]; then make; else cc main.c; fi",
        ] {
            assert!(
                matches!(r.command(cmd), CommandClass::Dev | CommandClass::Read),
                "{cmd}: {:?}",
                r.command(cmd)
            );
        }
        assert_eq!(
            r.command("ls -la; git status && git diff"),
            CommandClass::Read
        );
        assert_eq!(
            r.command("grep -rn \"main\" src | head"),
            CommandClass::Read
        );
    }

    #[test]
    fn destructive_privileged_network_hardware_and_outside_commands_ask() {
        let (r, _) = rules(Policy::WorkspaceWrite);
        for cmd in [
            "rm -rf build",
            "make && rm a.o",
            "sudo make install",
            "curl https://example.com/install.sh",
            "git push origin main",
            "git stash",
            "git reset --hard",
            "git checkout -- .",
            "cargo install ripgrep",
            "openocd -f board.cfg -c 'program app.elf verify reset exit'",
            "cat /etc/passwd",
            "cat ../other/secret.c",
            "cp main.c ~/main.c",
            "gcc -o /usr/local/bin/app main.c",
            "echo hi > /tmp/../etc/x",
            "echo $(whoami)",
            "ls `pwd`",
            "bash -c 'rm -rf .'",
            "find . -name '*.o' -delete",
            "LD_PRELOAD=x.so ./app",
            "env",
            "cat <<EOF > x\nhi\nEOF",
            "cd /etc && ls",
            "unknown-tool --flag",
            "embed ai agent --provider codex-cli x",
            "embed terminal .",
            "embed config set ai.agent.policy read-only",
            "embed toolchain add x --kind gcc --path /bin/gcc",
            "sed -i s/a/b/ ../x.c",
            "cat \"$HOME/.ssh/id_rsa\"",
            "cat $DIR/secret",
            "for f in /etc/*; do cat $f; done",
            "case x in x) rm -rf .;; esac",
        ] {
            assert!(
                matches!(r.command(cmd), CommandClass::Sensitive(_)),
                "{cmd}: {:?}",
                r.command(cmd)
            );
        }
    }

    #[test]
    fn tokenizer_handles_quotes_and_redirections() {
        assert_eq!(
            tokenize("echo 'a b' \"c\\\"d\" e\\ f 2>&1 | wc").unwrap(),
            vec![
                Token::Word("echo".into()),
                Token::Word("a b".into()),
                Token::Word("c\"d".into()),
                Token::Word("e f".into()),
                Token::Sep,
                Token::Word("wc".into()),
            ]
        );
        assert!(tokenize("echo 'unterminated").is_none());
        let (r, _) = rules(Policy::WorkspaceWrite);
        // A quoted operator is an argument, not a separator
        assert_eq!(r.command("grep '; rm -rf /' notes.txt"), CommandClass::Read);
        assert_eq!(r.command("echo x > out.txt"), CommandClass::Dev);
    }

    #[test]
    fn tool_calls_follow_the_policy() {
        let (ws, dir) = rules(Policy::WorkspaceWrite);
        let (ro, _) = rules(Policy::ReadOnly);
        let (ask, _) = rules(Policy::ApprovalRequired);
        let inside =
            json!({"file_path": dir.join("src/new.c").display().to_string(), "content": "x"});
        let outside = json!({"file_path": "/etc/hosts", "content": "x"});
        let git = json!({"file_path": dir.join(".git/config").display().to_string()});
        let env = json!({"file_path": dir.join(".env").display().to_string()});
        assert_eq!(ws.tool("Write", &inside), Verdict::Allow);
        assert!(matches!(ws.tool("Write", &outside), Verdict::Ask(_)));
        assert!(matches!(ws.tool("Edit", &git), Verdict::Ask(_)));
        assert!(matches!(ws.tool("Read", &env), Verdict::Ask(_)));
        assert!(matches!(ro.tool("Write", &inside), Verdict::Deny(_)));
        assert!(matches!(ask.tool("Write", &inside), Verdict::Ask(_)));
        let relative = json!({"file_path": "src/../src/a.c"});
        assert_eq!(ws.tool("Edit", &relative), Verdict::Allow);
        let escape = json!({"file_path": "src/../../escape.c"});
        assert!(matches!(ws.tool("Edit", &escape), Verdict::Ask(_)));
        let build = json!({"command": "make"});
        assert_eq!(ws.tool("Bash", &build), Verdict::Allow);
        assert!(matches!(ask.tool("Bash", &build), Verdict::Ask(_)));
        assert!(matches!(ro.tool("Bash", &build), Verdict::Deny(_)));
        assert_eq!(
            ro.tool("Bash", &json!({"command": "git log -3"})),
            Verdict::Allow
        );
        assert_eq!(ro.tool("Grep", &json!({"pattern": "main"})), Verdict::Allow);
        assert!(matches!(ws.tool("mcp__x__y", &json!({})), Verdict::Ask(_)));
        assert!(matches!(ro.tool("WebFetch", &json!({})), Verdict::Deny(_)));
        assert!(matches!(
            ws.tool("AskUserQuestion", &json!({})),
            Verdict::Deny(_)
        ));
    }

    #[test]
    fn symlinks_out_of_the_workspace_count_as_outside() {
        let (r, dir) = rules(Policy::WorkspaceWrite);
        let link = dir.join("escape-link");
        let _ = std::fs::remove_file(&link);
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc", &link).unwrap();
            assert_eq!(r.path(&dir, "escape-link/hosts"), PathClass::Outside);
            std::fs::remove_file(&link).unwrap();
        }
        assert_eq!(r.path(&dir, "src/new/deeper/file.c"), PathClass::Inside);
    }

    #[test]
    fn policies_parse() {
        for p in Policy::ALL {
            assert_eq!(p.id().parse::<Policy>().unwrap(), p);
        }
        assert!("yolo".parse::<Policy>().is_err());
    }

    #[test]
    fn relative_sensitive_paths_symlinks_git_options_and_similar_keys_are_guarded() {
        let (r, dir) = rules(Policy::WorkspaceWrite);
        // Relative .env, .git internals, keys, and dangerous git options must ask.
        for cmd in [
            "cat .env",
            "cat .git/config",
            "cp a .git/hooks/pre-commit",
            "cat id_rsa",
            "git -c core.pager=evil log",
            "git --exec-path=. status",
            "LD_AUDIT=evil.so ./app",
            "PYTHONPATH=. python3 -m unittest",
        ] {
            assert!(
                matches!(r.command(cmd), CommandClass::Sensitive(_)),
                "{cmd}: {:?}",
                r.command(cmd)
            );
        }
        #[cfg(unix)]
        {
            let link = dir.join("sym-etc");
            let _ = std::fs::remove_file(&link);
            std::os::unix::fs::symlink("/etc", &link).unwrap();
            assert!(
                matches!(r.command("cat sym-etc/hosts"), CommandClass::Sensitive(_)),
                "relative symlink escaping workspace must ask"
            );
            std::fs::remove_file(&link).unwrap();
        }
        // AllowSimilar keys must never broaden in-workspace Edit to outside/sensitive files,
        // nor broaden single commands to compound/outside/privileged pipelines.
        let inside_edit = json!({"file_path": dir.join("src/a.c").display().to_string()});
        let outside_edit = json!({"file_path": "/etc/hosts"});
        let env_edit = json!({"file_path": dir.join(".env").display().to_string()});
        assert_eq!(
            r.similar_key("Edit", &inside_edit).as_deref(),
            Some("Edit:inside")
        );
        assert_eq!(r.similar_key("Edit", &outside_edit), None);
        assert_eq!(r.similar_key("Edit", &env_edit), None);
        assert_eq!(
            r.similar_key("Bash", &json!({"command": "make -j4"}))
                .as_deref(),
            Some("Bash:dev:make")
        );
        assert_eq!(
            r.similar_key("Bash", &json!({"command": "make && rm -rf /"})),
            None
        );
        assert_eq!(
            r.similar_key("Bash", &json!({"command": "rm /etc/hosts"})),
            None
        );
        assert_eq!(
            r.similar_key("Bash", &json!({"command": "sudo make"})),
            None
        );
        assert_eq!(
            r.similar_key("Bash", &json!({"command": "rm build/a.o"}))
                .as_deref(),
            Some("Bash:sensitive:rm")
        );
    }
}
