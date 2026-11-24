//! Frontend-neutral syntax highlighting: language detection and a
//! line-incremental lexer.
//!
//! The lexer produces byte ranges and categories only; frontends choose
//! colours. Lexing is per line with an explicit carried state (block comments,
//! multi-line strings, heredocs), so a frame lexes only its visible lines plus
//! any lines above them that are not yet cached. The cache stores the lexer
//! state at the end of every line it has seen together with a hash of that
//! line, and is revalidated (hash only, no lexing) after an edit, so editing
//! re-lexes from the first changed line rather than from the top.
//!
//! All delimiters are ASCII; identifiers are Unicode-aware. Every span starts
//! and ends on a `char` boundary, so grapheme layout and cursor mapping are
//! unaffected.
use crate::EditorState;
use std::{
    ops::Range,
    path::{Path, PathBuf},
};

/// Documents larger than this are shown without highlighting.
pub const MAX_HIGHLIGHT_BYTES: usize = 8 * 1024 * 1024;
/// Lines longer than this are not lexed (they keep the previous state).
const MAX_LINE_BYTES: usize = 64 * 1024;

/// Highlight category of a span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Highlight {
    /// Language keywords and assembly mnemonics.
    Keyword,
    /// Built-in and user type names, lifetimes, TOML table headers.
    Type,
    /// Function names at calls and definitions, macros, shell commands, labels.
    Function,
    /// String and character literals.
    String,
    /// Numeric literals.
    Number,
    /// Comments.
    Comment,
    /// Named constants: booleans, null, ALL_CAPS identifiers.
    Constant,
    /// Operators.
    Operator,
    /// Preprocessor directives, attributes, decorators, assembler directives.
    Preprocessor,
    /// Variables with syntactic markers: shell `$VAR`, registers, keys.
    Variable,
    /// Control transfer: assembly jumps, calls, returns and branches.
    Branch,
}

/// One highlighted byte range (absolute document offsets).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    /// Byte range on `char` boundaries.
    pub range: Range<usize>,
    /// Category.
    pub kind: Highlight,
}

/// Languages with highlighting rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    /// C.
    C,
    /// C++.
    Cpp,
    /// Rust.
    Rust,
    /// Zig.
    Zig,
    /// GNU as / NASM style assembly (x86, ARM, RISC-V).
    Asm,
    /// Fortran (free and fixed form).
    Fortran,
    /// R statistical computing language.
    R,
    /// POSIX shell / Bash.
    Shell,
    /// Python.
    Python,
    /// JavaScript.
    JavaScript,
    /// TypeScript.
    TypeScript,
    /// TOML.
    Toml,
    /// JSON (comments tolerated).
    Json,
    /// No highlighting.
    Plain,
}

impl Language {
    /// Stable identifier, matching the toolchain language ids where they exist.
    pub fn id(self) -> &'static str {
        match self {
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::Rust => "rust",
            Self::Zig => "zig",
            Self::Asm => "asm",
            Self::Fortran => "fortran",
            Self::R => "r",
            Self::Shell => "shell",
            Self::Python => "python",
            Self::JavaScript => "javascript",
            Self::TypeScript => "typescript",
            Self::Toml => "toml",
            Self::Json => "json",
            Self::Plain => "text",
        }
    }

    /// Detect from the file extension, then the file name, then a `#!` line.
    pub fn detect(path: Option<&Path>, first_line: &str) -> Self {
        if let Some(path) = path {
            if let Some(lang) = path
                .extension()
                .and_then(|e| e.to_str())
                .and_then(Self::from_extension)
            {
                return lang;
            }
            if let Some(lang) = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(Self::from_file_name)
            {
                return lang;
            }
        }
        Self::from_shebang(first_line).unwrap_or(Self::Plain)
    }

    fn from_extension(ext: &str) -> Option<Self> {
        Some(match ext {
            "c" | "h" => Self::C,
            "cpp" | "cc" | "cxx" | "c++" | "hpp" | "hh" | "hxx" | "h++" | "ipp" | "tpp" | "inl"
            | "ino" | "cu" | "cuh" => Self::Cpp,
            "rs" => Self::Rust,
            "zig" | "zon" => Self::Zig,
            "s" | "S" | "sx" | "asm" | "nasm" => Self::Asm,
            "sh" | "bash" | "zsh" | "ksh" | "ash" | "dash" | "mksh" | "bats" => Self::Shell,
            "py" | "pyw" | "pyi" => Self::Python,
            "js" | "mjs" | "cjs" | "jsx" => Self::JavaScript,
            "ts" | "mts" | "cts" | "tsx" => Self::TypeScript,
            "toml" => Self::Toml,
            "json" | "jsonc" | "json5" | "geojson" | "code-workspace" => Self::Json,
            _ => match ext.to_ascii_lowercase().as_str() {
                "f" | "for" | "ftn" | "f77" | "f90" | "f95" | "f03" | "f08" | "f18" | "fpp" => {
                    Self::Fortran
                }
                "r" | "rmd" | "qmd" => Self::R,
                _ => return None,
            },
        })
    }

    fn from_file_name(name: &str) -> Option<Self> {
        Some(match name {
            ".bashrc" | ".bash_profile" | ".bash_login" | ".bash_logout" | ".profile"
            | ".zshrc" | ".zprofile" | ".zshenv" | ".kshrc" | "PKGBUILD" | "APKBUILD"
            | ".envrc" => Self::Shell,
            ".Rprofile" | "Rprofile" => Self::R,
            "Cargo.lock" | "Pipfile" | "uv.lock" | "poetry.lock" => Self::Toml,
            ".babelrc" | ".eslintrc" | "tsconfig.json" => Self::Json,
            _ => return None,
        })
    }

    fn from_shebang(line: &str) -> Option<Self> {
        let rest = line.strip_prefix("#!")?;
        let mut words = rest.split_whitespace();
        let mut program = words.next()?.rsplit('/').next()?;
        if program == "env" {
            program = words.find(|w| !w.starts_with('-') && !w.contains('='))?;
        }
        let base = program.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
        Some(match base {
            "sh" | "bash" | "zsh" | "ksh" | "dash" | "ash" | "mksh" | "busybox" => Self::Shell,
            "python" | "pypy" => Self::Python,
            "Rscript" | "R" => Self::R,
            "node" | "nodejs" | "deno" | "bun" => Self::JavaScript,
            "ts-node" | "tsx" => Self::TypeScript,
            _ => return None,
        })
    }
}

/// Lexer state carried from the end of one line to the start of the next.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum LineState {
    /// Ordinary code.
    #[default]
    Normal,
    /// Inside a block comment (nesting depth for Rust).
    Comment(u8),
    /// Inside a string that continues on the next line.
    Str(Quote),
    /// Inside a Rust raw string with this many `#`.
    RawStr(u8),
    /// Inside a shell heredoc ended by `tag` (`<<-` strips leading tabs).
    Heredoc {
        /// Terminator word.
        tag: Box<str>,
        /// Leading tabs are ignored before the terminator.
        strip_tabs: bool,
    },
}

/// Delimiter of a multi-line string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Quote {
    /// `"…"` with backslash escapes.
    Double,
    /// `'…'` (shell: no escapes).
    Single,
    /// JavaScript template literal.
    Backtick,
    /// `"""…"""`.
    TripleDouble,
    /// `'''…'''`.
    TripleSingle,
}

/// Highlight one line of text starting in `state`; spans are relative to the
/// line start. Returns the state at the end of the line.
pub fn highlight_line(
    language: Language,
    line: &str,
    state: &LineState,
    out: &mut Vec<Span>,
) -> LineState {
    highlight_line_in(language, None, line, state, out)
}

/// [`highlight_line`] with the assembly dialect known (None: tolerant rules
/// that accept every dialect's comments).
pub fn highlight_line_in(
    language: Language,
    dialect: Option<crate::asm::Dialect>,
    line: &str,
    state: &LineState,
    out: &mut Vec<Span>,
) -> LineState {
    let mut lexer = Lexer {
        s: line,
        b: line.as_bytes(),
        i: 0,
        out,
        pending: None,
    };
    match language {
        Language::Plain => LineState::Normal,
        Language::Shell => lexer.shell(state.clone()),
        Language::Asm => lexer.asm(state.clone(), dialect),
        Language::Toml => lexer.toml(state.clone()),
        Language::Json => lexer.code(&JSON, state.clone()),
        Language::Fortran => lexer.code(&FORTRAN, state.clone()),
        Language::R => lexer.code(&R, state.clone()),
        Language::C => lexer.code(&C, state.clone()),
        Language::Cpp => lexer.code(&CPP, state.clone()),
        Language::Rust => lexer.code(&RUST, state.clone()),
        Language::Zig => lexer.code(&ZIG, state.clone()),
        Language::Python => lexer.code(&PYTHON, state.clone()),
        Language::JavaScript => lexer.code(&JAVASCRIPT, state.clone()),
        Language::TypeScript => lexer.code(&TYPESCRIPT, state.clone()),
    }
}

/// Incremental per-document highlighter (one per open document).
#[derive(Debug, Default)]
pub struct Highlighter {
    language: Option<Language>,
    dialect: Option<crate::asm::Dialect>,
    /// The dialect was set by configuration, not detected.
    forced: bool,
    path: Option<PathBuf>,
    revision: Option<u64>,
    validated: bool,
    /// (hash of line i, lexer state at the end of line i)
    lines: Vec<(u64, LineState)>,
}

impl Highlighter {
    /// Assembly dialect in use (detected, or set by [`Highlighter::set_dialect`]).
    pub fn dialect(&self) -> Option<crate::asm::Dialect> {
        self.dialect
    }

    /// Force an assembly dialect (configuration), or None to detect it.
    pub fn set_dialect(&mut self, dialect: Option<crate::asm::Dialect>) {
        if self.dialect != dialect || self.forced != dialect.is_some() {
            self.dialect = dialect;
            self.forced = dialect.is_some();
            self.lines.clear();
            self.revision = None;
        }
    }

    /// Language detected for the document at the last call.
    pub fn language(&self) -> Language {
        self.language.unwrap_or(Language::Plain)
    }

    fn sync(&mut self, state: &EditorState) {
        let revision = state.buffer().revision();
        let path = state.path();
        if self.revision == Some(revision) && self.path.as_deref() == path {
            return;
        }
        let first = state
            .line_range(0)
            .map(|r| r.start..r.end.min(r.start + 256))
            .and_then(|r| {
                let mut end = r.end;
                while end > r.start && state.buffer().char_at(end).is_err() {
                    end -= 1;
                }
                state.buffer().slice(r.start..end).ok()
            })
            .unwrap_or_default();
        let language = if state.buffer().byte_len() > MAX_HIGHLIGHT_BYTES {
            Language::Plain
        } else {
            Language::detect(path, &first)
        };
        if self.language != Some(language) || self.path.as_deref() != path {
            self.lines.clear();
        }
        if language == Language::Asm && !self.forced {
            // Detection reads the first 16 KiB; a changed verdict re-lexes.
            let buffer = state.buffer();
            let mut end = buffer.byte_len().min(16 * 1024);
            while end > 0 && buffer.char_at(end).is_err() {
                end -= 1;
            }
            let sample = buffer.slice(0..end).unwrap_or_default();
            let dialect = Some(crate::asm::Dialect::detect(path, &sample));
            if dialect != self.dialect {
                self.dialect = dialect;
                self.lines.clear();
            }
        }
        self.language = Some(language);
        self.path = path.map(Path::to_path_buf);
        self.revision = Some(revision);
        self.validated = false;
    }

    /// Keep cached end states only up to the first line whose text changed.
    fn validate(&mut self, state: &EditorState) {
        if self.validated {
            return;
        }
        self.validated = true;
        let count = state.line_count();
        self.lines.truncate(count);
        for index in 0..self.lines.len() {
            if line_hash(state, index) != Some(self.lines[index].0) {
                self.lines.truncate(index);
                break;
            }
        }
    }

    fn start_state(&self, line: usize) -> LineState {
        line.checked_sub(1)
            .and_then(|i| self.lines.get(i))
            .map(|(_, s)| s.clone())
            .unwrap_or_default()
    }

    /// Highlight spans of document line `line` with absolute byte offsets.
    pub fn spans(&mut self, state: &EditorState, line: usize) -> Vec<Span> {
        self.sync(state);
        let language = self.language();
        if language == Language::Plain || line >= state.line_count() {
            return Vec::new();
        }
        self.validate(state);
        let mut scratch = Vec::new();
        while self.lines.len() < line {
            let index = self.lines.len();
            let start = self.start_state(index);
            let end = match line_text(state, index) {
                Some((_, text)) => {
                    scratch.clear();
                    highlight_line_in(language, self.dialect, &text, &start, &mut scratch)
                }
                None => start,
            };
            let hash = line_hash(state, index).unwrap_or_default();
            self.lines.push((hash, end));
        }
        let start = self.start_state(line);
        let Some((base, text)) = line_text(state, line) else {
            return Vec::new();
        };
        let mut spans = Vec::new();
        let end = highlight_line_in(language, self.dialect, &text, &start, &mut spans);
        if self.lines.len() == line {
            let hash = line_hash(state, line).unwrap_or_default();
            self.lines.push((hash, end));
        }
        for span in &mut spans {
            span.range = span.range.start + base..span.range.end + base;
        }
        spans
    }
}

fn line_text(state: &EditorState, line: usize) -> Option<(usize, String)> {
    let range = state.line_range(line)?;
    if range.len() > MAX_LINE_BYTES {
        return None;
    }
    Some((range.start, state.buffer().slice(range).ok()?))
}

fn line_hash(state: &EditorState, line: usize) -> Option<u64> {
    let range = state.line_range(line)?;
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325 ^ range.len() as u64;
    for chunk in state.buffer().chunks_in(range).ok()? {
        for &byte in chunk.as_bytes() {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    Some(hash)
}

/// Per-language rules for the generic code lexer.
struct Spec {
    line_comments: &'static [&'static str],
    block_comment: Option<(&'static str, &'static str)>,
    nested_comments: bool,
    keywords: &'static [&'static str],
    types: &'static [&'static str],
    constants: &'static [&'static str],
    builtins: &'static [&'static str],
    /// Keywords after which the next identifier names a function.
    defines_function: &'static [&'static str],
    /// Keywords after which the next identifier names a type.
    defines_type: &'static [&'static str],
    case_insensitive: bool,
    /// `#` as the first non-blank character starts a preprocessor line.
    hash_preprocessor: bool,
    /// Double-quoted strings may span lines.
    multiline_double: bool,
    single: SingleQuote,
    backtick_template: bool,
    triple_quotes: bool,
    /// `ident(` is a call.
    calls: bool,
    /// Capitalised identifiers are types.
    pascal_types: bool,
    /// Identifiers ending in `_t` are types (C).
    suffix_t_types: bool,
    /// ALL_CAPS identifiers are constants.
    caps_constants: bool,
    rust: bool,
    zig: bool,
    python: bool,
    fortran: bool,
    json: bool,
    /// `@name` decorators.
    decorators: bool,
    /// `$` is an identifier character (JavaScript).
    dollar_ident: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SingleQuote {
    /// `'a'` character literal.
    Char,
    /// `'…'` string literal.
    Str,
    /// Not a string delimiter.
    None,
}

const BASE: Spec = Spec {
    line_comments: &["//"],
    block_comment: Some(("/*", "*/")),
    nested_comments: false,
    keywords: &[],
    types: &[],
    constants: &[],
    builtins: &[],
    defines_function: &[],
    defines_type: &[],
    case_insensitive: false,
    hash_preprocessor: false,
    multiline_double: false,
    single: SingleQuote::Char,
    backtick_template: false,
    triple_quotes: false,
    calls: true,
    pascal_types: false,
    suffix_t_types: false,
    caps_constants: true,
    rust: false,
    zig: false,
    python: false,
    fortran: false,
    json: false,
    decorators: false,
    dollar_ident: false,
};

const C_KEYWORDS: &[&str] = &[
    "auto",
    "break",
    "case",
    "const",
    "continue",
    "default",
    "do",
    "else",
    "enum",
    "extern",
    "for",
    "goto",
    "if",
    "inline",
    "register",
    "restrict",
    "return",
    "sizeof",
    "static",
    "struct",
    "switch",
    "typedef",
    "union",
    "volatile",
    "while",
    "_Alignas",
    "_Alignof",
    "_Atomic",
    "_Generic",
    "_Noreturn",
    "_Static_assert",
    "_Thread_local",
    "alignas",
    "alignof",
    "static_assert",
    "thread_local",
    "typeof",
    "typeof_unqual",
    "asm",
    "__asm__",
    "__attribute__",
    "__volatile__",
    "__inline__",
    "__restrict__",
    "__extension__",
];
const C_TYPES: &[&str] = &[
    "void",
    "char",
    "short",
    "int",
    "long",
    "float",
    "double",
    "signed",
    "unsigned",
    "bool",
    "_Bool",
    "_Complex",
    "_Imaginary",
    "FILE",
    "va_list",
    "wchar_t",
];
const C_CONSTANTS: &[&str] = &[
    "NULL", "true", "false", "nullptr", "EOF", "stdin", "stdout", "stderr",
];

const C: Spec = Spec {
    keywords: C_KEYWORDS,
    types: C_TYPES,
    constants: C_CONSTANTS,
    defines_type: &["struct", "union", "enum"],
    hash_preprocessor: true,
    suffix_t_types: true,
    ..BASE
};

const CPP: Spec = Spec {
    keywords: &[
        "auto",
        "break",
        "case",
        "catch",
        "class",
        "concept",
        "const",
        "consteval",
        "constexpr",
        "constinit",
        "const_cast",
        "continue",
        "co_await",
        "co_return",
        "co_yield",
        "decltype",
        "default",
        "delete",
        "do",
        "dynamic_cast",
        "else",
        "enum",
        "explicit",
        "export",
        "extern",
        "final",
        "for",
        "friend",
        "goto",
        "if",
        "inline",
        "mutable",
        "namespace",
        "new",
        "noexcept",
        "operator",
        "override",
        "private",
        "protected",
        "public",
        "register",
        "reinterpret_cast",
        "requires",
        "return",
        "sizeof",
        "static",
        "static_assert",
        "static_cast",
        "struct",
        "switch",
        "template",
        "this",
        "throw",
        "try",
        "typedef",
        "typeid",
        "typename",
        "union",
        "using",
        "virtual",
        "volatile",
        "while",
        "alignas",
        "alignof",
        "thread_local",
        "asm",
        "__attribute__",
        "and",
        "or",
        "not",
        "xor",
        "bitand",
        "bitor",
        "compl",
    ],
    types: &[
        "void",
        "char",
        "char8_t",
        "char16_t",
        "char32_t",
        "wchar_t",
        "short",
        "int",
        "long",
        "float",
        "double",
        "signed",
        "unsigned",
        "bool",
        "string",
        "string_view",
        "vector",
        "array",
        "map",
        "unordered_map",
        "set",
        "optional",
        "variant",
        "unique_ptr",
        "shared_ptr",
        "weak_ptr",
        "span",
        "FILE",
    ],
    constants: C_CONSTANTS,
    defines_type: &["struct", "union", "enum", "class", "namespace", "concept"],
    hash_preprocessor: true,
    suffix_t_types: true,
    pascal_types: true,
    ..BASE
};

const RUST: Spec = Spec {
    keywords: &[
        "as",
        "async",
        "await",
        "break",
        "const",
        "continue",
        "crate",
        "dyn",
        "else",
        "enum",
        "extern",
        "fn",
        "for",
        "if",
        "impl",
        "in",
        "let",
        "loop",
        "match",
        "mod",
        "move",
        "mut",
        "pub",
        "ref",
        "return",
        "self",
        "static",
        "struct",
        "super",
        "trait",
        "type",
        "union",
        "unsafe",
        "use",
        "where",
        "while",
        "yield",
        "macro_rules",
        "gen",
        "try",
    ],
    types: &[
        "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize",
        "f32", "f64", "bool", "char", "str", "Self", "String", "Vec", "Option", "Result", "Box",
    ],
    constants: &["true", "false", "None", "Some", "Ok", "Err"],
    defines_function: &["fn"],
    defines_type: &["struct", "enum", "trait", "type", "union"],
    nested_comments: true,
    multiline_double: true,
    pascal_types: true,
    rust: true,
    ..BASE
};

const ZIG: Spec = Spec {
    block_comment: None,
    keywords: &[
        "addrspace",
        "align",
        "allowzero",
        "and",
        "anyframe",
        "anytype",
        "asm",
        "break",
        "callconv",
        "catch",
        "comptime",
        "const",
        "continue",
        "defer",
        "else",
        "enum",
        "errdefer",
        "error",
        "export",
        "extern",
        "fn",
        "for",
        "if",
        "inline",
        "linksection",
        "noalias",
        "noinline",
        "nosuspend",
        "opaque",
        "or",
        "orelse",
        "packed",
        "pub",
        "resume",
        "return",
        "struct",
        "suspend",
        "switch",
        "test",
        "threadlocal",
        "try",
        "union",
        "unreachable",
        "usingnamespace",
        "var",
        "volatile",
        "while",
    ],
    types: &[
        "isize",
        "usize",
        "f16",
        "f32",
        "f64",
        "f80",
        "f128",
        "bool",
        "void",
        "noreturn",
        "type",
        "anyerror",
        "anyopaque",
        "comptime_int",
        "comptime_float",
        "c_char",
        "c_short",
        "c_ushort",
        "c_int",
        "c_uint",
        "c_long",
        "c_ulong",
        "c_longlong",
        "c_ulonglong",
        "c_longdouble",
    ],
    constants: &["true", "false", "null", "undefined"],
    defines_function: &["fn"],
    pascal_types: true,
    zig: true,
    ..BASE
};

const PYTHON: Spec = Spec {
    line_comments: &["#"],
    block_comment: None,
    keywords: &[
        "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del",
        "elif", "else", "except", "finally", "for", "from", "global", "if", "import", "in", "is",
        "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while", "with",
        "yield", "match", "case",
    ],
    types: &[
        "int",
        "float",
        "complex",
        "str",
        "bytes",
        "bytearray",
        "bool",
        "list",
        "dict",
        "set",
        "frozenset",
        "tuple",
        "object",
        "type",
        "range",
    ],
    constants: &[
        "True",
        "False",
        "None",
        "NotImplemented",
        "Ellipsis",
        "__name__",
        "__file__",
    ],
    builtins: &[
        "print",
        "len",
        "open",
        "isinstance",
        "issubclass",
        "super",
        "enumerate",
        "zip",
        "map",
        "filter",
        "sorted",
        "reversed",
        "min",
        "max",
        "sum",
        "abs",
        "any",
        "all",
        "iter",
        "next",
        "repr",
        "getattr",
        "setattr",
        "hasattr",
        "input",
        "round",
        "format",
        "id",
        "hash",
        "vars",
        "dir",
    ],
    defines_function: &["def"],
    defines_type: &["class"],
    single: SingleQuote::Str,
    triple_quotes: true,
    pascal_types: true,
    python: true,
    decorators: true,
    ..BASE
};

const JS_KEYWORDS: &[&str] = &[
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "export",
    "extends",
    "finally",
    "for",
    "from",
    "function",
    "get",
    "if",
    "import",
    "in",
    "instanceof",
    "let",
    "new",
    "of",
    "return",
    "set",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
];
const JS_CONSTANTS: &[&str] = &["true", "false", "null", "undefined", "NaN", "Infinity"];

const JAVASCRIPT: Spec = Spec {
    keywords: JS_KEYWORDS,
    constants: JS_CONSTANTS,
    builtins: &["require"],
    defines_function: &["function"],
    defines_type: &["class"],
    single: SingleQuote::Str,
    backtick_template: true,
    pascal_types: true,
    dollar_ident: true,
    ..BASE
};

const TYPESCRIPT: Spec = Spec {
    keywords: &[
        "async",
        "await",
        "break",
        "case",
        "catch",
        "class",
        "const",
        "continue",
        "debugger",
        "default",
        "delete",
        "do",
        "else",
        "export",
        "extends",
        "finally",
        "for",
        "from",
        "function",
        "get",
        "if",
        "import",
        "in",
        "instanceof",
        "let",
        "new",
        "of",
        "return",
        "set",
        "static",
        "super",
        "switch",
        "this",
        "throw",
        "try",
        "typeof",
        "var",
        "void",
        "while",
        "with",
        "yield",
        "type",
        "interface",
        "enum",
        "implements",
        "declare",
        "namespace",
        "module",
        "abstract",
        "readonly",
        "keyof",
        "infer",
        "is",
        "as",
        "satisfies",
        "private",
        "public",
        "protected",
        "override",
        "asserts",
        "unique",
    ],
    types: &[
        "string", "number", "boolean", "any", "unknown", "never", "object", "bigint", "symbol",
        "Array", "Record", "Partial", "Readonly", "Promise", "Map", "Set",
    ],
    constants: JS_CONSTANTS,
    builtins: &["require"],
    defines_function: &["function"],
    defines_type: &["class", "interface", "type", "enum", "namespace"],
    single: SingleQuote::Str,
    backtick_template: true,
    pascal_types: true,
    dollar_ident: true,
    decorators: true,
    ..BASE
};

const FORTRAN: Spec = Spec {
    line_comments: &["!"],
    block_comment: None,
    keywords: &[
        "program",
        "end",
        "module",
        "submodule",
        "subroutine",
        "function",
        "procedure",
        "contains",
        "use",
        "only",
        "implicit",
        "none",
        "intent",
        "in",
        "out",
        "inout",
        "parameter",
        "dimension",
        "allocatable",
        "pointer",
        "target",
        "optional",
        "save",
        "public",
        "private",
        "protected",
        "value",
        "pure",
        "impure",
        "elemental",
        "recursive",
        "result",
        "interface",
        "abstract",
        "extends",
        "class",
        "select",
        "case",
        "rank",
        "default",
        "if",
        "then",
        "else",
        "elseif",
        "endif",
        "do",
        "enddo",
        "while",
        "concurrent",
        "exit",
        "cycle",
        "return",
        "stop",
        "call",
        "allocate",
        "deallocate",
        "nullify",
        "where",
        "elsewhere",
        "endwhere",
        "forall",
        "endforall",
        "block",
        "endblock",
        "associate",
        "endassociate",
        "go",
        "goto",
        "continue",
        "print",
        "write",
        "read",
        "open",
        "close",
        "inquire",
        "flush",
        "rewind",
        "backspace",
        "endfile",
        "namelist",
        "format",
        "data",
        "common",
        "equivalence",
        "include",
        "bind",
        "import",
        "external",
        "intrinsic",
        "non_intrinsic",
        "sequence",
        "enum",
        "enumerator",
        "generic",
        "operator",
        "assignment",
        "deferred",
        "non_overridable",
        "nopass",
        "pass",
        "final",
        "error",
        "sync",
        "all",
        "images",
        "memory",
        "team",
        "form",
        "change",
        "lock",
        "unlock",
        "event",
        "post",
        "wait",
        "codimension",
        "contiguous",
        "asynchronous",
        "volatile",
        "critical",
        "endcritical",
        "endprogram",
        "endmodule",
        "endsubmodule",
        "endsubroutine",
        "endfunction",
        "endtype",
        "endinterface",
        "endselect",
    ],
    types: &[
        "integer",
        "real",
        "double",
        "precision",
        "complex",
        "logical",
        "character",
        "type",
        "kind",
        "len",
        "int8",
        "int16",
        "int32",
        "int64",
        "real32",
        "real64",
        "real128",
        "c_int",
        "c_short",
        "c_long",
        "c_long_long",
        "c_int8_t",
        "c_int16_t",
        "c_int32_t",
        "c_int64_t",
        "c_size_t",
        "c_float",
        "c_double",
        "c_bool",
        "c_char",
        "c_ptr",
        "c_funptr",
    ],
    constants: &[
        "input_unit",
        "output_unit",
        "error_unit",
        "iostat_end",
        "iostat_eor",
        "c_null_ptr",
        "c_null_funptr",
        "c_null_char",
    ],
    builtins: &[
        "abs",
        "sqrt",
        "exp",
        "log",
        "log10",
        "sin",
        "cos",
        "tan",
        "asin",
        "acos",
        "atan",
        "atan2",
        "sinh",
        "cosh",
        "tanh",
        "hypot",
        "erf",
        "erfc",
        "gamma",
        "log_gamma",
        "min",
        "max",
        "mod",
        "modulo",
        "floor",
        "ceiling",
        "nint",
        "int",
        "dble",
        "cmplx",
        "conjg",
        "aimag",
        "sum",
        "product",
        "size",
        "shape",
        "lbound",
        "ubound",
        "allocated",
        "present",
        "associated",
        "move_alloc",
        "matmul",
        "dot_product",
        "transpose",
        "reshape",
        "pack",
        "unpack",
        "merge",
        "spread",
        "cshift",
        "eoshift",
        "any",
        "all",
        "count",
        "findloc",
        "maxval",
        "minval",
        "maxloc",
        "minloc",
        "trim",
        "adjustl",
        "adjustr",
        "len_trim",
        "index",
        "scan",
        "verify",
        "repeat",
        "new_line",
        "huge",
        "tiny",
        "epsilon",
        "precision",
        "range",
        "radix",
        "digits",
        "storage_size",
        "bit_size",
        "selected_real_kind",
        "selected_int_kind",
        "iso_fortran_env",
        "iso_c_binding",
        "ieee_arithmetic",
        "ieee_exceptions",
        "ieee_is_nan",
        "ieee_is_finite",
        "omp_lib",
        "c_loc",
        "c_funloc",
        "c_f_pointer",
        "c_f_procpointer",
        "c_associated",
        "c_sizeof",
        "this_image",
        "num_images",
        "co_sum",
        "co_min",
        "co_max",
        "co_broadcast",
        "co_reduce",
        "command_argument_count",
        "get_command_argument",
        "get_environment_variable",
        "execute_command_line",
        "cpu_time",
        "random_number",
        "random_seed",
        "date_and_time",
        "system_clock",
    ],
    defines_function: &[
        "subroutine",
        "function",
        "program",
        "module",
        "submodule",
        "call",
    ],
    case_insensitive: true,
    hash_preprocessor: true,
    single: SingleQuote::Str,
    calls: false,
    caps_constants: false,
    fortran: true,
    ..BASE
};

const R: Spec = Spec {
    line_comments: &["#"],
    block_comment: None,
    keywords: &[
        "if",
        "else",
        "repeat",
        "while",
        "function",
        "for",
        "in",
        "next",
        "break",
        "return",
        "library",
        "require",
        "source",
        "switch",
        "try",
        "tryCatch",
        "withCallingHandlers",
        "stop",
        "warning",
        "message",
        "suppressWarnings",
        "suppressMessages",
    ],
    types: &[
        "logical",
        "integer",
        "double",
        "numeric",
        "complex",
        "character",
        "raw",
        "vector",
        "list",
        "matrix",
        "array",
        "factor",
        "data.frame",
        "tibble",
        "environment",
        "formula",
        "Date",
        "POSIXct",
        "POSIXlt",
    ],
    constants: &[
        "TRUE",
        "FALSE",
        "T",
        "F",
        "NULL",
        "NA",
        "NA_integer_",
        "NA_real_",
        "NA_complex_",
        "NA_character_",
        "Inf",
        "NaN",
        "pi",
        "LETTERS",
        "letters",
        "month.abb",
        "month.name",
    ],
    builtins: &[
        "c",
        "cat",
        "print",
        "paste",
        "paste0",
        "sprintf",
        "format",
        "length",
        "nrow",
        "ncol",
        "dim",
        "names",
        "colnames",
        "rownames",
        "class",
        "typeof",
        "str",
        "summary",
        "head",
        "tail",
        "seq",
        "seq_len",
        "seq_along",
        "rep",
        "rev",
        "sort",
        "order",
        "unique",
        "duplicated",
        "which",
        "match",
        "any",
        "all",
        "ifelse",
        "is.na",
        "is.null",
        "is.finite",
        "is.numeric",
        "is.character",
        "is.logical",
        "is.list",
        "is.data.frame",
        "as.numeric",
        "as.integer",
        "as.character",
        "as.logical",
        "as.factor",
        "as.matrix",
        "as.data.frame",
        "sum",
        "prod",
        "mean",
        "median",
        "sd",
        "var",
        "min",
        "max",
        "range",
        "quantile",
        "cor",
        "cov",
        "lm",
        "glm",
        "predict",
        "rnorm",
        "runif",
        "rbinom",
        "dnorm",
        "pnorm",
        "qnorm",
        "set.seed",
        "sample",
        "apply",
        "lapply",
        "sapply",
        "vapply",
        "mapply",
        "tapply",
        "Reduce",
        "Filter",
        "Map",
        "do.call",
        "subset",
        "merge",
        "rbind",
        "cbind",
        "read.csv",
        "write.csv",
        "read.table",
        "readRDS",
        "saveRDS",
        "readLines",
        "writeLines",
        "file.path",
        "file.exists",
        "dir.create",
        "list.files",
        "commandArgs",
        "Sys.getenv",
        "Sys.time",
        "plot",
        "lines",
        "points",
        "hist",
        "boxplot",
        "barplot",
        "ggplot",
        "aes",
        "mutate",
        "filter",
        "select",
        "summarise",
        "summarize",
        "group_by",
        "arrange",
    ],
    multiline_double: true,
    single: SingleQuote::Str,
    calls: true,
    caps_constants: false,
    ..BASE
};

const JSON: Spec = Spec {
    constants: &["true", "false", "null"],
    single: SingleQuote::None,
    calls: false,
    caps_constants: false,
    json: true,
    ..BASE
};

struct Lexer<'a, 'o> {
    s: &'a str,
    b: &'a [u8],
    i: usize,
    out: &'o mut Vec<Span>,
    /// State left open by a construct lexed inside a helper (prefixed strings).
    pending: Option<LineState>,
}

fn is_ident_start(ch: char) -> bool {
    ch == '_' || ch.is_alphabetic()
}
fn is_ident(ch: char) -> bool {
    ch == '_' || ch.is_alphanumeric()
}

impl Lexer<'_, '_> {
    fn emit(&mut self, start: usize, end: usize, kind: Highlight) {
        if start >= end {
            return;
        }
        if let Some(last) = self.out.last_mut() {
            if last.kind == kind && last.range.end == start {
                last.range.end = end;
                return;
            }
        }
        self.out.push(Span {
            range: start..end,
            kind,
        });
    }
    fn peek(&self) -> Option<char> {
        self.s[self.i..].chars().next()
    }
    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.b.get(self.i + offset).copied()
    }
    fn at(&self, text: &str) -> bool {
        self.b[self.i..].starts_with(text.as_bytes())
    }
    fn bump(&mut self) {
        self.i += self.peek().map_or(1, char::len_utf8);
    }
    fn rest(&mut self, kind: Highlight) {
        self.emit(self.i, self.b.len(), kind);
        self.i = self.b.len();
    }
    fn skip_spaces(&mut self) {
        while matches!(self.peek_at(0), Some(b' ' | b'\t')) {
            self.i += 1;
        }
    }
    fn ident_end(&self, from: usize, dollar: bool) -> usize {
        let mut end = from;
        for ch in self.s[from..].chars() {
            if is_ident(ch) || (dollar && ch == '$') {
                end += ch.len_utf8();
            } else {
                break;
            }
        }
        end
    }
    /// Next non-blank byte after `from`.
    fn next_significant(&self, from: usize) -> Option<u8> {
        self.b[from..]
            .iter()
            .copied()
            .find(|b| !matches!(b, b' ' | b'\t'))
    }
    fn number(&mut self) {
        let start = self.i;
        let hex = self.at("0x") || self.at("0X");
        let mut prev = 0u8;
        while let Some(byte) = self.peek_at(0) {
            let exponent_sign = matches!(byte, b'+' | b'-')
                && (matches!(prev, b'e' | b'E') && !hex || matches!(prev, b'p' | b'P'));
            if byte == b'.' && self.peek_at(1) == Some(b'.') {
                break;
            }
            if byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'.' || exponent_sign {
                prev = byte;
                self.i += 1;
            } else if byte == b'\'' && self.peek_at(1).is_some_and(|b| b.is_ascii_hexdigit()) {
                // C++14 digit separator.
                self.i += 1;
            } else {
                break;
            }
        }
        self.emit(start, self.i, Highlight::Number);
    }
    /// Consume a quoted string body after the opening delimiter; returns
    /// whether it closed on this line.
    fn string_body(&mut self, close: &str, escapes: bool) -> bool {
        while self.i < self.b.len() {
            if escapes && self.b[self.i] == b'\\' {
                self.i += 1;
                if self.i < self.b.len() {
                    self.bump();
                }
                continue;
            }
            if self.at(close) {
                self.i += close.len();
                return true;
            }
            self.bump();
        }
        false
    }
    fn block_comment(&mut self, spec: &Spec, mut depth: u8, start: usize) -> LineState {
        let (open, close) = spec.block_comment.unwrap_or(("/*", "*/"));
        while self.i < self.b.len() {
            if spec.nested_comments && self.at(open) {
                depth = depth.saturating_add(1);
                self.i += open.len();
            } else if self.at(close) {
                self.i += close.len();
                depth -= 1;
                if depth == 0 {
                    self.emit(start, self.i, Highlight::Comment);
                    return LineState::Normal;
                }
            } else {
                self.bump();
            }
        }
        self.emit(start, self.i, Highlight::Comment);
        LineState::Comment(depth)
    }
    fn resume_string(&mut self, quote: Quote, spec: &Spec) -> LineState {
        let start = self.i;
        let (close, escapes) = match quote {
            Quote::Double => ("\"", true),
            Quote::Single => ("'", !spec.json),
            Quote::Backtick => ("`", true),
            Quote::TripleDouble => ("\"\"\"", true),
            Quote::TripleSingle => ("'''", true),
        };
        let closed = self.string_body(close, escapes);
        self.emit(start, self.i, Highlight::String);
        if closed {
            LineState::Normal
        } else {
            LineState::Str(quote)
        }
    }
    fn raw_string(&mut self, hashes: u8, start: usize) -> LineState {
        let close = format!("\"{}", "#".repeat(usize::from(hashes)));
        let closed = self.string_body(&close, false);
        self.emit(start, self.i, Highlight::String);
        if closed {
            LineState::Normal
        } else {
            LineState::RawStr(hashes)
        }
    }

    /// Generic lexer for C-family, Rust, Zig, Python, JS/TS, Fortran and JSON.
    fn code(&mut self, spec: &Spec, state: LineState) -> LineState {
        let resumed = match state {
            LineState::Comment(depth) => self.block_comment(spec, depth, 0),
            LineState::Str(quote) => self.resume_string(quote, spec),
            LineState::RawStr(hashes) => self.raw_string(hashes, 0),
            LineState::Normal | LineState::Heredoc { .. } => {
                self.skip_spaces();
                if spec.hash_preprocessor && self.at("#") {
                    return self.preprocessor_line(spec);
                }
                if spec.fortran && matches!(self.b.first(), Some(b'*')) {
                    self.i = 0;
                    self.rest(Highlight::Comment);
                    return LineState::Normal;
                }
                if spec.zig && self.at("\\\\") {
                    self.rest(Highlight::String);
                    return LineState::Normal;
                }
                LineState::Normal
            }
        };
        if resumed != LineState::Normal {
            return resumed;
        }
        self.code_loop(spec)
    }

    fn code_loop(&mut self, spec: &Spec) -> LineState {
        let mut expect: Option<Highlight> = None;
        while self.i < self.b.len() {
            let start = self.i;
            let byte = self.b[self.i];
            let pending = expect.take();
            if spec.line_comments.iter().any(|c| self.at(c)) {
                self.rest(Highlight::Comment);
                break;
            }
            if let Some((open, _)) = spec.block_comment {
                if self.at(open) {
                    self.i += open.len();
                    let state = self.block_comment(spec, 1, start);
                    if state != LineState::Normal {
                        return state;
                    }
                    continue;
                }
            }
            match byte {
                b' ' | b'\t' => {
                    self.i += 1;
                    expect = pending;
                }
                b'"' if spec.triple_quotes && self.at("\"\"\"") => {
                    self.i += 3;
                    let state = self.resume_string(Quote::TripleDouble, spec);
                    self.extend_last(start);
                    if state != LineState::Normal {
                        return state;
                    }
                }
                b'"' => {
                    self.i += 1;
                    let closed = self.string_body("\"", true);
                    let kind = if spec.json && self.next_significant(self.i) == Some(b':') {
                        Highlight::Variable
                    } else {
                        Highlight::String
                    };
                    self.emit(start, self.i, kind);
                    if !closed && spec.multiline_double {
                        return LineState::Str(Quote::Double);
                    }
                }
                b'\''
                    if spec.single == SingleQuote::Str
                        && spec.triple_quotes
                        && self.at("\'\'\'") =>
                {
                    self.i += 3;
                    let state = self.resume_string(Quote::TripleSingle, spec);
                    self.extend_last(start);
                    if state != LineState::Normal {
                        return state;
                    }
                }
                b'\'' if spec.single == SingleQuote::Str => {
                    self.i += 1;
                    if spec.fortran {
                        // Fortran doubles the quote instead of escaping it.
                        while self.i < self.b.len() {
                            if self.at("\'\'") {
                                self.i += 2;
                            } else if self.at("\'") {
                                self.i += 1;
                                break;
                            } else {
                                self.bump();
                            }
                        }
                    } else {
                        self.string_body("\'", true);
                    }
                    self.emit(start, self.i, Highlight::String);
                }
                b'\'' if spec.single == SingleQuote::Char => self.char_or_lifetime(spec),
                b'`' if spec.backtick_template => {
                    self.i += 1;
                    let state = self.resume_string(Quote::Backtick, spec);
                    self.extend_last(start);
                    if state != LineState::Normal {
                        return state;
                    }
                }
                b'0'..=b'9' => self.number(),
                b'.' if spec.fortran => self.fortran_dot(),
                b'.' if self.peek_at(1).is_some_and(|b| b.is_ascii_digit())
                    && (start == 0 || self.b[start - 1] != b'.') =>
                {
                    self.number()
                }
                b'#' if spec.rust && matches!(self.peek_at(1), Some(b'[' | b'!')) => {
                    self.rust_attribute()
                }
                b'@' if spec.decorators || spec.zig => {
                    self.i += 1;
                    let end = self.ident_end(self.i, false);
                    if end > self.i {
                        self.i = end;
                        let kind = if spec.zig {
                            Highlight::Function
                        } else {
                            Highlight::Preprocessor
                        };
                        self.emit(start, self.i, kind);
                    } else {
                        self.emit(start, self.i, Highlight::Operator);
                    }
                }
                b'+' | b'-' | b'*' | b'/' | b'%' | b'=' | b'&' | b'|' | b'^' | b'!' | b'<'
                | b'>' | b'~' | b'?' | b':' => {
                    self.i += 1;
                    if !spec.json {
                        while self.i - start < 3
                            && self.peek_at(0).is_some_and(|b| {
                                matches!(b, b'=' | b'&' | b'|' | b'<' | b'>' | b':' | b'+' | b'-')
                            })
                        {
                            self.i += 1;
                        }
                        self.emit(start, self.i, Highlight::Operator);
                    }
                }
                _ => {
                    let ch = self.peek().unwrap_or(' ');
                    if is_ident_start(ch) || (spec.dollar_ident && ch == '$') {
                        expect = self.identifier(spec, pending);
                        if let Some(state) = self.pending.take() {
                            return state;
                        }
                    } else {
                        self.bump();
                    }
                }
            }
        }
        LineState::Normal
    }

    /// Merge the span just emitted with a prefix starting at `start` (string
    /// prefixes and opening delimiters).
    fn extend_last(&mut self, start: usize) {
        if let Some(last) = self.out.last_mut() {
            if last.kind == Highlight::String && last.range.start >= start {
                last.range.start = start;
                return;
            }
        }
        self.emit(start, self.i, Highlight::String);
    }

    fn preprocessor_line(&mut self, spec: &Spec) -> LineState {
        let start = self.i;
        self.i += 1;
        self.skip_spaces();
        let word_end = self.ident_end(self.i, false);
        let word = &self.s[self.i..word_end];
        let include = word == "include" || word == "import";
        self.i = word_end;
        self.emit(start, self.i, Highlight::Preprocessor);
        if include {
            self.skip_spaces();
            if self.at("<") {
                let s = self.i;
                while self.i < self.b.len() && self.b[self.i] != b'>' {
                    self.bump();
                }
                self.i = (self.i + 1).min(self.b.len());
                self.emit(s, self.i, Highlight::String);
            }
        }
        // The rest of a directive is ordinary code (macro bodies, conditions).
        self.code_loop(spec)
    }

    fn char_or_lifetime(&mut self, spec: &Spec) {
        let start = self.i;
        self.i += 1;
        let next = self.peek();
        if next == Some('\\') {
            self.i += 1;
            self.bump();
            while self.i < self.b.len() && self.b[self.i] != b'\'' && self.i - start < 12 {
                self.bump();
            }
            self.i = (self.i + 1).min(self.b.len());
            self.emit(start, self.i, Highlight::String);
            return;
        }
        if let Some(ch) = next {
            let after = self.i + ch.len_utf8();
            if self.b.get(after) == Some(&b'\'') {
                self.i = after + 1;
                self.emit(start, self.i, Highlight::String);
                return;
            }
            if spec.rust && is_ident_start(ch) {
                self.i = self.ident_end(self.i, false);
                self.emit(start, self.i, Highlight::Type);
                return;
            }
        }
        self.emit(start, self.i, Highlight::Operator);
    }

    fn rust_attribute(&mut self) {
        let start = self.i;
        let mut depth = 0usize;
        while self.i < self.b.len() {
            match self.b[self.i] {
                b'[' => depth += 1,
                b']' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        self.i += 1;
                        break;
                    }
                }
                _ => {}
            }
            self.bump();
        }
        self.emit(start, self.i, Highlight::Preprocessor);
    }

    fn fortran_dot(&mut self) {
        let start = self.i;
        let end = self.ident_end(self.i + 1, false);
        if end > self.i + 1 && self.b.get(end) == Some(&b'.') {
            let word = self.s[self.i + 1..end].to_ascii_lowercase();
            let kind = match word.as_str() {
                "true" | "false" => Some(Highlight::Constant),
                "and" | "or" | "not" | "eqv" | "neqv" | "eq" | "ne" | "lt" | "le" | "gt" | "ge" => {
                    Some(Highlight::Operator)
                }
                _ => None,
            };
            if let Some(kind) = kind {
                self.i = end + 1;
                // `.true._lk` kind suffix
                if self.at("_") {
                    self.i = self.ident_end(self.i, false);
                }
                self.emit(start, self.i, kind);
                return;
            }
        }
        if self.peek_at(1).is_some_and(|b| b.is_ascii_digit()) {
            self.number();
        } else {
            self.i += 1;
        }
    }

    fn identifier(&mut self, spec: &Spec, expect: Option<Highlight>) -> Option<Highlight> {
        let start = self.i;
        let end = self.ident_end(self.i, spec.dollar_ident);
        let word = &self.s[start..end];
        self.i = end;
        // String prefixes: Python r"", f"", b''…; Rust b"", r#""#, br""; C L"", u8"".
        if let Some(&quote) = self.b.get(end) {
            let lower = word.to_ascii_lowercase();
            if spec.python
                && matches!(quote, b'"' | b'\'')
                && matches!(
                    lower.as_str(),
                    "r" | "b" | "f" | "u" | "rb" | "br" | "fr" | "rf"
                )
            {
                let triple =
                    self.b[end..].starts_with(if quote == b'"' { b"\"\"\"" } else { b"'''" });
                self.i = end + if triple { 3 } else { 1 };
                let q = match (quote, triple) {
                    (b'"', true) => Quote::TripleDouble,
                    (b'\'', true) => Quote::TripleSingle,
                    (b'"', false) => Quote::Double,
                    _ => Quote::Single,
                };
                let close = match q {
                    Quote::TripleDouble => "\"\"\"",
                    Quote::TripleSingle => "'''",
                    Quote::Double => "\"",
                    _ => "'",
                };
                let closed = self.string_body(close, true);
                self.emit(start, self.i, Highlight::String);
                if !closed && triple {
                    // Remember the open triple-quoted string for the next line.
                    self.pending = Some(LineState::Str(q));
                }
                return None;
            }
            if spec.rust && matches!(word, "r" | "br" | "b" | "c" | "cr") {
                if quote == b'#' || (quote == b'"' && word.ends_with('r')) {
                    let mut hashes = 0u8;
                    let mut j = end;
                    while self.b.get(j) == Some(&b'#') {
                        hashes = hashes.saturating_add(1);
                        j += 1;
                    }
                    if self.b.get(j) == Some(&b'"') {
                        self.i = j + 1;
                        let state = self.raw_string(hashes, start);
                        if state != LineState::Normal {
                            self.pending = Some(state);
                        }
                        return None;
                    }
                } else if quote == b'"' {
                    self.i = end + 1;
                    let closed = self.string_body("\"", true);
                    self.emit(start, self.i, Highlight::String);
                    if !closed {
                        self.pending = Some(LineState::Str(Quote::Double));
                    }
                    return None;
                } else if quote == b'\'' && word == "b" {
                    self.i = end;
                    self.char_or_lifetime(spec);
                    self.extend_last(start);
                    return None;
                }
            }
            if !spec.rust
                && !spec.python
                && quote == b'"'
                && matches!(
                    word,
                    "L" | "u" | "U" | "u8" | "R" | "LR" | "uR" | "UR" | "u8R"
                )
            {
                self.i = end + 1;
                self.string_body("\"", !word.ends_with('R'));
                self.emit(start, self.i, Highlight::String);
                return None;
            }
        }
        let folded;
        let key = if spec.case_insensitive {
            folded = word.to_ascii_lowercase();
            folded.as_str()
        } else {
            word
        };
        let next = self.b.get(end).copied();
        let kind = if let Some(kind) = expect.filter(|_| !spec.keywords.contains(&key)) {
            Some(kind)
        } else if spec.constants.contains(&key) {
            Some(Highlight::Constant)
        } else if spec.keywords.contains(&key) {
            Some(Highlight::Keyword)
        } else if spec.types.contains(&key)
            || (spec.zig && is_zig_int_type(key))
            || (spec.suffix_t_types && key.len() > 2 && key.ends_with("_t"))
        {
            Some(Highlight::Type)
        } else if spec.rust && next == Some(b'!') && self.b.get(end + 1) != Some(&b'=') {
            self.i = end + 1;
            Some(Highlight::Function)
        } else if spec.json {
            None
        } else if spec.caps_constants && is_caps(word) {
            Some(Highlight::Constant)
        } else if (spec.calls && self.next_significant(end) == Some(b'('))
            || spec.builtins.contains(&key)
        {
            Some(Highlight::Function)
        } else if spec.pascal_types && word.chars().next().is_some_and(char::is_uppercase) {
            Some(Highlight::Type)
        } else if spec.python && (word == "self" || word == "cls") {
            Some(Highlight::Variable)
        } else {
            None
        };
        if let Some(kind) = kind {
            self.emit(start, self.i, kind);
        }
        if spec.defines_function.contains(&key) {
            Some(Highlight::Function)
        } else if spec.defines_type.contains(&key) {
            Some(Highlight::Type)
        } else {
            None
        }
    }
}

fn is_caps(word: &str) -> bool {
    word.len() > 1
        && word.chars().any(|c| c.is_ascii_uppercase())
        && word
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

fn is_zig_int_type(word: &str) -> bool {
    let mut chars = word.chars();
    matches!(chars.next(), Some('i' | 'u'))
        && word.len() > 1
        && word.len() <= 6
        && chars.all(|c| c.is_ascii_digit())
}

const SHELL_KEYWORDS: &[&str] = &[
    "if", "then", "else", "elif", "fi", "case", "esac", "for", "select", "while", "until", "do",
    "done", "in", "function", "time", "coproc", "[[", "]]", "!",
];
/// Keywords after which a new command starts.
const SHELL_COMMAND_KEYWORDS: &[&str] = &[
    "if", "then", "else", "elif", "while", "until", "do", "time", "!", "coproc",
];

fn shell_delimiter(byte: u8) -> bool {
    matches!(
        byte,
        b' ' | b'\t'
            | b';'
            | b'|'
            | b'&'
            | b'('
            | b')'
            | b'<'
            | b'>'
            | b'\''
            | b'"'
            | b'`'
            | b'$'
            | b'\\'
    )
}

impl Lexer<'_, '_> {
    /// POSIX shell / Bash.
    fn shell(&mut self, state: LineState) -> LineState {
        match state {
            LineState::Heredoc { tag, strip_tabs } => {
                let body = if strip_tabs {
                    self.s.trim_start_matches('\t')
                } else {
                    self.s
                };
                let done = body == &*tag;
                self.rest(Highlight::String);
                return if done {
                    LineState::Normal
                } else {
                    LineState::Heredoc { tag, strip_tabs }
                };
            }
            LineState::Str(Quote::Single) => {
                let closed = self.string_body("'", false);
                self.emit(0, self.i, Highlight::String);
                if !closed {
                    return state;
                }
            }
            LineState::Str(quote @ (Quote::Double | Quote::Backtick)) => {
                let close = if quote == Quote::Double { b'"' } else { b'`' };
                if !self.shell_quoted(0, close) {
                    return state;
                }
            }
            _ => {}
        }
        if self.i == 0 && self.at("#!") {
            self.rest(Highlight::Preprocessor);
            return LineState::Normal;
        }
        let mut command = true;
        let mut value = false;
        let mut heredoc: Option<(Box<str>, bool)> = None;
        while self.i < self.b.len() {
            let start = self.i;
            let byte = self.b[self.i];
            match byte {
                b' ' | b'\t' => {
                    self.i += 1;
                    value = false;
                }
                b'#' if start == 0 || shell_delimiter(self.b[start - 1]) => {
                    self.rest(Highlight::Comment);
                    break;
                }
                b'\'' => {
                    self.i += 1;
                    let closed = self.string_body("'", false);
                    self.emit(start, self.i, Highlight::String);
                    command = false;
                    if !closed {
                        return LineState::Str(Quote::Single);
                    }
                }
                b'"' | b'`' => {
                    self.i += 1;
                    command = false;
                    if !self.shell_quoted(start, byte) {
                        return LineState::Str(if byte == b'"' {
                            Quote::Double
                        } else {
                            Quote::Backtick
                        });
                    }
                }
                b'$' => {
                    command = self.shell_dollar();
                    if command {
                        value = false;
                    }
                }
                b'\\' => {
                    self.i += 1;
                    if self.i < self.b.len() {
                        self.bump();
                    }
                }
                b';' | b'|' | b'&' | b'(' | b')' => {
                    self.i += 1;
                    if matches!(self.peek_at(0), Some(b';' | b'|' | b'&')) {
                        self.i += 1;
                    }
                    self.emit(start, self.i, Highlight::Operator);
                    command = byte != b')';
                }
                b'<' if self.at("<<") && !self.at("<<<") => {
                    self.i += 2;
                    let strip_tabs = self.at("-");
                    if strip_tabs {
                        self.i += 1;
                    }
                    self.emit(start, self.i, Highlight::Operator);
                    self.skip_spaces();
                    let tag_start = self.i;
                    let tag = match self.peek_at(0) {
                        Some(q @ (b'\'' | b'"')) => {
                            self.i += 1;
                            let inner = self.i;
                            while self.i < self.b.len() && self.b[self.i] != q {
                                self.bump();
                            }
                            let tag = self.s[inner..self.i].to_string();
                            self.i = (self.i + 1).min(self.b.len());
                            tag
                        }
                        _ => {
                            if self.at("\\") {
                                self.i += 1;
                            }
                            let inner = self.i;
                            while self.i < self.b.len() && !shell_delimiter(self.b[self.i]) {
                                self.bump();
                            }
                            self.s[inner..self.i].to_string()
                        }
                    };
                    self.emit(tag_start, self.i, Highlight::String);
                    if !tag.is_empty() {
                        heredoc = Some((tag.into(), strip_tabs));
                    }
                    command = false;
                }
                b'<' | b'>' => {
                    self.i += 1;
                    while self.i - start < 3
                        && matches!(self.peek_at(0), Some(b'<' | b'>' | b'&' | b'|'))
                    {
                        self.i += 1;
                    }
                    self.emit(start, self.i, Highlight::Operator);
                }
                _ => {
                    let mut end = self.i;
                    while end < self.b.len() && !shell_delimiter(self.b[end]) {
                        end += 1;
                    }
                    // Never split a UTF-8 sequence (delimiters are ASCII).
                    let word = &self.s[start..end];
                    if command && !value {
                        if let Some(eq) = word.find('=').filter(|&eq| {
                            let name = word[..eq].trim_end_matches('+');
                            !name.is_empty()
                                && name.chars().next().is_some_and(is_ident_start)
                                && name.chars().all(is_ident)
                        }) {
                            let name_end = start + word[..eq].trim_end_matches('+').len();
                            self.emit(start, name_end, Highlight::Variable);
                            self.emit(name_end, start + eq + 1, Highlight::Operator);
                            self.i = start + eq + 1;
                            value = true;
                            continue;
                        }
                    }
                    self.i = end;
                    if word == "{" || word == "}" || word == "[" || word == "]" {
                        self.emit(start, end, Highlight::Operator);
                        command = word == "{";
                    } else if SHELL_KEYWORDS.contains(&word) && (command || word == "in") {
                        self.emit(start, end, Highlight::Keyword);
                        command = SHELL_COMMAND_KEYWORDS.contains(&word);
                    } else if command && !value {
                        self.emit(start, end, Highlight::Function);
                        command = false;
                    } else if !word.is_empty() && word.bytes().all(|b| b.is_ascii_digit()) {
                        self.emit(start, end, Highlight::Number);
                    } else if end == start {
                        self.bump();
                    }
                }
            }
        }
        match heredoc {
            Some((tag, strip_tabs)) => LineState::Heredoc { tag, strip_tabs },
            None => LineState::Normal,
        }
    }

    /// `$name`, `${…}`, `$1`, `$?`, `$(…)`, `$((…))`, `$'…'`. Returns whether
    /// a new command starts (command substitution).
    fn shell_dollar(&mut self) -> bool {
        let start = self.i;
        self.i += 1;
        match self.peek_at(0) {
            Some(b'(') => {
                let arithmetic = self.at("((");
                self.i += if arithmetic { 2 } else { 1 };
                self.emit(start, self.i, Highlight::Operator);
                !arithmetic
            }
            Some(b'{') => {
                let mut depth = 0usize;
                while self.i < self.b.len() {
                    match self.b[self.i] {
                        b'{' => depth += 1,
                        b'}' => {
                            depth -= 1;
                            if depth == 0 {
                                self.i += 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                    self.bump();
                }
                self.emit(start, self.i, Highlight::Variable);
                false
            }
            Some(b'\'') => {
                self.i += 1;
                self.string_body("'", true);
                self.emit(start, self.i, Highlight::String);
                false
            }
            Some(b'0'..=b'9' | b'@' | b'*' | b'#' | b'?' | b'$' | b'!' | b'-') => {
                self.i += 1;
                self.emit(start, self.i, Highlight::Variable);
                false
            }
            _ => {
                let end = self.ident_end(self.i, false);
                self.i = end;
                if end > start + 1 {
                    self.emit(start, end, Highlight::Variable);
                }
                false
            }
        }
    }

    /// Body of a `"…"` or `` `…` `` string with `$var` expansions; returns
    /// whether it closed on this line.
    fn shell_quoted(&mut self, start: usize, close: u8) -> bool {
        let mut segment = start;
        while self.i < self.b.len() {
            let byte = self.b[self.i];
            if byte == b'\\' {
                self.i += 1;
                if self.i < self.b.len() {
                    self.bump();
                }
            } else if byte == close {
                self.i += 1;
                self.emit(segment, self.i, Highlight::String);
                return true;
            } else if byte == b'$'
                && close == b'"'
                && self.peek_at(1).is_some_and(|b| {
                    b == b'{' || b == b'_' || b.is_ascii_alphanumeric() || b"@*#?$!".contains(&b)
                })
            {
                self.emit(segment, self.i, Highlight::String);
                self.shell_dollar();
                segment = self.i;
            } else {
                self.bump();
            }
        }
        self.emit(segment, self.i, Highlight::String);
        false
    }
}

const ASM_SPEC: Spec = Spec { ..BASE };
const CPP_DIRECTIVES: &[&str] = &[
    "include", "define", "undef", "if", "ifdef", "ifndef", "elif", "else", "endif", "error",
    "warning", "pragma", "line",
];

fn is_register(word: &str) -> bool {
    const FIXED: &[&str] = &[
        "rax", "rbx", "rcx", "rdx", "rsi", "rdi", "rsp", "rbp", "rip", "eax", "ebx", "ecx", "edx",
        "esi", "edi", "esp", "ebp", "eip", "ax", "bx", "cx", "dx", "si", "di", "bp", "ip", "al",
        "ah", "bl", "bh", "cl", "ch", "dl", "dh", "sil", "dil", "spl", "bpl", "cs", "ds", "es",
        "fs", "gs", "ss", "st", "sp", "lr", "pc", "fp", "sb", "sl", "xzr", "wzr", "apsr", "cpsr",
        "spsr", "zero", "ra", "gp", "tp", "rflags", "eflags",
    ];
    if FIXED.contains(&word) {
        return true;
    }
    let digits = word.trim_start_matches(|c: char| c.is_ascii_alphabetic());
    let prefix = &word[..word.len() - digits.len()];
    let (number, suffix) = digits.split_at(
        digits
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(digits.len()),
    );
    let Ok(n) = number.parse::<u32>() else {
        return false;
    };
    if !suffix.is_empty() {
        return prefix == "r" && (8..=15).contains(&n) && matches!(suffix, "b" | "w" | "d");
    }
    match prefix {
        "r" => n <= 15,
        "x" | "w" | "v" | "q" | "d" | "s" | "f" | "xmm" | "ymm" | "zmm" => n <= 31,
        "a" | "fa" => n <= 7,
        "t" | "ft" => n <= 11,
        "fs" => n <= 11,
        "k" | "mm" | "st" | "dr" => n <= 7,
        "cr" => n <= 15,
        _ => false,
    }
}

impl Lexer<'_, '_> {
    /// GNU as / NASM assembly for x86, ARM and RISC-V.
    fn asm(&mut self, state: LineState, dialect: Option<crate::asm::Dialect>) -> LineState {
        use crate::asm::Dialect;
        // Which characters start a comment depends on the dialect; without
        // one every convention is accepted (the historical behaviour).
        let semicolon_comment = matches!(dialect, None | Some(Dialect::Nasm));
        let hash_comment = matches!(
            dialect,
            Some(Dialect::GasAtt | Dialect::GasIntel | Dialect::RiscV)
        );
        let at_comment_anywhere = dialect == Some(Dialect::Arm);
        let at_comment_after_blank = dialect.is_none();
        let mut branch = false;
        if let LineState::Comment(depth) = state {
            let state = self.block_comment(&ASM_SPEC, depth, 0);
            if state != LineState::Normal {
                return state;
            }
        }
        self.skip_spaces();
        let mut mnemonic = false;
        if self.at("#") || self.at("%") {
            let word_end = self.ident_end(self.i + 1, false);
            let word = &self.s[self.i + 1..word_end];
            let nasm = self.at("%") && !word.is_empty();
            if nasm || CPP_DIRECTIVES.contains(&word) {
                let start = self.i;
                self.i = word_end;
                self.emit(start, self.i, Highlight::Preprocessor);
                // Directive arguments are operands, not mnemonics.
                mnemonic = true;
            } else if self.at("#") {
                self.rest(Highlight::Comment);
                return LineState::Normal;
            }
        }
        while self.i < self.b.len() {
            let start = self.i;
            let byte = self.b[self.i];
            let after_blank = start == 0 || matches!(self.b[start - 1], b' ' | b'\t');
            match byte {
                b' ' | b'\t' | b',' => self.i += 1,
                b';' if semicolon_comment => {
                    self.rest(Highlight::Comment);
                    break;
                }
                b';' => {
                    // GNU as: a statement separator; a new mnemonic follows.
                    self.i += 1;
                    mnemonic = false;
                    branch = false;
                }
                b'/' if self.at("//") => {
                    self.rest(Highlight::Comment);
                    break;
                }
                b'/' if self.at("/*") => {
                    self.i += 2;
                    let state = self.block_comment(&ASM_SPEC, 1, start);
                    if state != LineState::Normal {
                        return state;
                    }
                }
                b'@' if at_comment_anywhere || (at_comment_after_blank && after_blank) => {
                    self.rest(Highlight::Comment);
                    break;
                }
                b'#' if hash_comment
                    || (dialect.is_none()
                        && matches!(self.peek_at(1), None | Some(b' ' | b'\t'))) =>
                {
                    self.rest(Highlight::Comment);
                    break;
                }
                b'#' | b'$' => {
                    // Immediates: ARM `#4`, AT&T `$0x10`, `$symbol`.
                    self.i += 1;
                    if matches!(self.peek_at(0), Some(b'-' | b'+')) {
                        self.i += 1;
                    }
                    if self.peek_at(0).is_some_and(|b| b.is_ascii_digit()) {
                        self.number();
                        self.extend_number(start);
                    } else {
                        let end = self.ident_end(self.i, false);
                        self.i = end.max(self.i);
                        self.emit(start, self.i, Highlight::Constant);
                    }
                }
                b'%' => {
                    self.i += 1;
                    let end = self.ident_end(self.i, false);
                    self.i = end;
                    self.emit(start, end, Highlight::Variable);
                }
                b'"' => {
                    self.i += 1;
                    self.string_body("\"", true);
                    self.emit(start, self.i, Highlight::String);
                }
                b'\'' => self.char_or_lifetime(&ASM_SPEC),
                b'0'..=b'9' => self.number(),
                b'+' | b'-' | b'*' | b'/' | b'=' | b'<' | b'>' | b'&' | b'|' | b'~' | b'!'
                | b'^' => {
                    self.i += 1;
                    self.emit(start, self.i, Highlight::Operator);
                }
                _ => {
                    let ch = self.peek().unwrap_or(' ');
                    if is_ident_start(ch) || ch == '.' {
                        let mut end = start + ch.len_utf8();
                        for c in self.s[end..].chars() {
                            if is_ident(c) || c == '.' || c == '$' {
                                end += c.len_utf8();
                            } else {
                                break;
                            }
                        }
                        let word = &self.s[start..end];
                        self.i = end;
                        if !mnemonic && self.at(":") {
                            self.i += 1;
                            self.emit(start, self.i, Highlight::Function);
                        } else if !mnemonic {
                            mnemonic = true;
                            branch = dialect.is_some_and(|d| d.is_branch(word));
                            let kind = if word.starts_with('.') && !word.starts_with("b.") {
                                Highlight::Preprocessor
                            } else if branch {
                                Highlight::Branch
                            } else {
                                Highlight::Keyword
                            };
                            self.emit(start, end, kind);
                        } else if is_register(&word.to_ascii_lowercase()) {
                            self.emit(start, end, Highlight::Variable);
                        } else if branch {
                            // The target of a jump or call: a label.
                            self.emit(start, end, Highlight::Function);
                        } else if is_caps(word) {
                            self.emit(start, end, Highlight::Constant);
                        }
                    } else {
                        self.bump();
                    }
                }
            }
        }
        LineState::Normal
    }

    fn extend_number(&mut self, start: usize) {
        if let Some(last) = self.out.last_mut() {
            if last.kind == Highlight::Number {
                last.range.start = start;
            }
        }
    }

    /// TOML: tables, keys, strings (including multi-line), numbers, dates.
    fn toml(&mut self, state: LineState) -> LineState {
        match state {
            LineState::Str(quote @ (Quote::TripleDouble | Quote::TripleSingle)) => {
                let double = quote == Quote::TripleDouble;
                let closed = self.string_body(if double { "\"\"\"" } else { "'''" }, double);
                self.emit(0, self.i, Highlight::String);
                if !closed {
                    return state;
                }
            }
            _ => {
                self.skip_spaces();
                if self.at("[") {
                    let start = self.i;
                    while self.i < self.b.len() && self.b[self.i] != b'#' {
                        let closing = self.b[self.i] == b']';
                        self.bump();
                        if closing && !self.at("]") {
                            break;
                        }
                    }
                    self.emit(start, self.i, Highlight::Type);
                }
            }
        }
        let mut key = true;
        while self.i < self.b.len() {
            let start = self.i;
            let byte = self.b[self.i];
            match byte {
                b' ' | b'\t' => self.i += 1,
                b'#' => {
                    self.rest(Highlight::Comment);
                    break;
                }
                b'"' | b'\'' => {
                    let double = byte == b'"';
                    let triple = self.at(if double { "\"\"\"" } else { "'''" });
                    if triple {
                        self.i += 3;
                        let closed =
                            self.string_body(if double { "\"\"\"" } else { "'''" }, double);
                        self.emit(start, self.i, Highlight::String);
                        if !closed {
                            return LineState::Str(if double {
                                Quote::TripleDouble
                            } else {
                                Quote::TripleSingle
                            });
                        }
                    } else {
                        self.i += 1;
                        self.string_body(if double { "\"" } else { "'" }, double);
                        let is_key =
                            key && matches!(self.next_significant(self.i), Some(b'=' | b'.'));
                        self.emit(
                            start,
                            self.i,
                            if is_key {
                                Highlight::Variable
                            } else {
                                Highlight::String
                            },
                        );
                    }
                }
                b'=' => {
                    self.i += 1;
                    self.emit(start, self.i, Highlight::Operator);
                    key = false;
                }
                b'{' | b',' => {
                    self.i += 1;
                    key = true;
                }
                b'[' | b']' | b'}' => {
                    self.i += 1;
                    key = false;
                }
                _ => {
                    let mut end = self.i;
                    while end < self.b.len()
                        && (self.b[end].is_ascii_alphanumeric()
                            || matches!(self.b[end], b'_' | b'-' | b'.' | b'+' | b':'))
                    {
                        end += 1;
                    }
                    if end == self.i {
                        self.bump();
                        continue;
                    }
                    let word = &self.s[start..end];
                    let first = word.as_bytes()[0];
                    let kind = if key && self.next_significant(end) == Some(b'=') {
                        Some(Highlight::Variable)
                    } else if word.len() >= 10
                        && word.as_bytes()[4] == b'-'
                        && word[..4].bytes().all(|b| b.is_ascii_digit())
                        || word.len() >= 8 && word.as_bytes()[2] == b':'
                    {
                        // Offset date-time, local date or local time.
                        let mut e = end;
                        if self.b.get(e) == Some(&b' ')
                            && self.b.get(e + 1).is_some_and(|b| b.is_ascii_digit())
                        {
                            e += 1;
                            while e < self.b.len()
                                && (self.b[e].is_ascii_alphanumeric()
                                    || matches!(self.b[e], b':' | b'.' | b'-' | b'+'))
                            {
                                e += 1;
                            }
                        }
                        end = e;
                        Some(Highlight::Constant)
                    } else if matches!(word, "true" | "false") {
                        Some(Highlight::Constant)
                    } else if first.is_ascii_digit()
                        || matches!(first, b'+' | b'-')
                        || matches!(word, "inf" | "nan" | "+inf" | "-inf" | "+nan" | "-nan")
                    {
                        Some(Highlight::Number)
                    } else {
                        None
                    };
                    self.i = end;
                    if let Some(kind) = kind {
                        self.emit(start, end, kind);
                    }
                }
            }
        }
        LineState::Normal
    }
}
