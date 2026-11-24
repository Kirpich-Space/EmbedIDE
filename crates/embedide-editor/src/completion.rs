//! Frontend-neutral completion data: the identifier at the cursor, local
//! word candidates, language-aware keywords/builtins, and ranking. Semantic
//! candidates come from a language server (see `embedide_core::lsp`) and use
//! the same [`Candidate`] type.
use crate::{syntax::Language, EditorError, EditorState};
use std::{
    collections::{BTreeSet, HashSet},
    ops::Range,
};

/// Scanned for local words; larger documents use their first part only.
const WORD_SCAN_BYTES: usize = 4 * 1024 * 1024;
/// Distinct local words kept.
const MAX_WORDS: usize = 5000;
/// Longest local word offered.
const MAX_WORD_CHARS: usize = 64;

/// One suggestion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Text shown in the list.
    pub label: String,
    /// Text inserted on acceptance (snippet syntax already removed).
    pub insert: String,
    /// Symbol kind, e.g. `function`, `struct`, or `word` for local words.
    pub kind: String,
    /// Short extra information (type, signature), if any.
    pub detail: String,
    /// Text matched against what was typed.
    pub filter: String,
    /// Server ranking key; ties keep server order.
    pub sort: String,
    /// Document byte range the server asked to replace, if it gave one.
    pub range: Option<Range<usize>>,
}

impl Candidate {
    /// A local word from the document.
    pub fn word(word: &str) -> Self {
        Self {
            label: word.into(),
            insert: word.into(),
            kind: "word".into(),
            detail: String::new(),
            filter: word.into(),
            sort: String::new(),
            range: None,
        }
    }
}

/// Characters that belong to identifiers (Unicode letters and digits, `_`).
pub fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// Byte range of the identifier fragment that ends at the cursor (empty
/// when the cursor does not follow an identifier character).
pub fn word_before_cursor(state: &EditorState) -> Result<Range<usize>, EditorError> {
    let cursor = state.cursor();
    let row = state.lines.row(cursor);
    let start = state.lines.start(row);
    let before = state.buffer.slice(start..cursor)?;
    let begin = before
        .char_indices()
        .rev()
        .take_while(|(_, ch)| is_word_char(*ch))
        .last()
        .map_or(before.len(), |(i, _)| i);
    Ok(start + begin..cursor)
}

/// Distinct identifiers of the document (at least two characters, not
/// starting with a digit), except the occurrence the cursor is typing.
pub fn document_words(state: &EditorState) -> Vec<String> {
    let buffer = state.buffer();
    let mut end = buffer.byte_len().min(WORD_SCAN_BYTES);
    while end > 0 && buffer.char_at(end).is_err() {
        end -= 1;
    }
    let Ok(text) = buffer.slice(0..end) else {
        return Vec::new();
    };
    let typing = word_before_cursor(state).ok();
    let mut words = BTreeSet::new();
    let mut start = None;
    for (index, ch) in text
        .char_indices()
        .chain(std::iter::once((text.len(), ' ')))
    {
        if is_word_char(ch) {
            start.get_or_insert(index);
            continue;
        }
        let Some(begin) = start.take() else { continue };
        let word = &text[begin..index];
        let at_cursor = typing
            .as_ref()
            .is_some_and(|t| !t.is_empty() && t.start == begin && t.end <= index);
        if at_cursor
            || word.chars().next().is_some_and(|c| c.is_ascii_digit())
            || word.chars().count() < 2
            || word.chars().count() > MAX_WORD_CHARS
        {
            continue;
        }
        words.insert(word.to_string());
        if words.len() >= MAX_WORDS {
            break;
        }
    }
    words.into_iter().collect()
}

/// Distinct identifiers of `text` (at least two characters, not starting with a digit),
/// capped at `max_words`.
pub fn words_in_text(text: &str, max_words: usize) -> Vec<String> {
    let mut words = BTreeSet::new();
    let mut start = None;
    for (index, ch) in text
        .char_indices()
        .chain(std::iter::once((text.len(), ' ')))
    {
        if is_word_char(ch) {
            start.get_or_insert(index);
            continue;
        }
        let Some(begin) = start.take() else { continue };
        let word = &text[begin..index];
        if word.chars().next().is_some_and(|c| c.is_ascii_digit())
            || word.chars().count() < 2
            || word.chars().count() > MAX_WORD_CHARS
        {
            continue;
        }
        words.insert(word.to_string());
        if words.len() >= max_words {
            break;
        }
    }
    words.into_iter().collect()
}

/// Merge two candidate lists, keeping the first occurrence of each `(label, insert)`.
pub fn merge_candidates(primary: Vec<Candidate>, secondary: Vec<Candidate>) -> Vec<Candidate> {
    let mut seen: HashSet<(String, String)> =
        HashSet::with_capacity(primary.len() + secondary.len());
    let mut out = Vec::with_capacity(primary.len() + secondary.len());
    for c in primary.into_iter().chain(secondary) {
        if seen.insert((c.label.clone(), c.insert.clone())) {
            out.push(c);
        }
    }
    out
}

/// Language-aware keywords, types, constants and builtins for local completion
/// when no language server is running (or while it starts).
pub fn language_candidates(language: Language) -> Vec<Candidate> {
    let make = |items: &[(&str, &str, &str)]| -> Vec<Candidate> {
        items
            .iter()
            .map(|&(label, kind, detail)| Candidate {
                label: label.into(),
                insert: label.into(),
                kind: kind.into(),
                detail: detail.into(),
                filter: label.into(),
                sort: "2".into(),
                range: None,
            })
            .collect()
    };
    match language {
        Language::Rust => make(&[
            ("fn", "keyword", ""),
            ("let", "keyword", ""),
            ("mut", "keyword", ""),
            ("pub", "keyword", ""),
            ("struct", "keyword", ""),
            ("enum", "keyword", ""),
            ("impl", "keyword", ""),
            ("trait", "keyword", ""),
            ("match", "keyword", ""),
            ("if", "keyword", ""),
            ("else", "keyword", ""),
            ("while", "keyword", ""),
            ("for", "keyword", ""),
            ("loop", "keyword", ""),
            ("return", "keyword", ""),
            ("break", "keyword", ""),
            ("continue", "keyword", ""),
            ("use", "keyword", ""),
            ("mod", "keyword", ""),
            ("const", "keyword", ""),
            ("static", "keyword", ""),
            ("type", "keyword", ""),
            ("where", "keyword", ""),
            ("async", "keyword", ""),
            ("await", "keyword", ""),
            ("crate", "keyword", ""),
            ("self", "keyword", ""),
            ("Self", "type", ""),
            ("Option", "type", "core::option"),
            ("Result", "type", "core::result"),
            ("Vec", "type", "alloc::vec"),
            ("String", "type", "alloc::string"),
            ("PathBuf", "type", "std::path"),
            ("HashMap", "type", "std::collections"),
            ("BTreeMap", "type", "std::collections"),
            ("usize", "type", ""),
            ("isize", "type", ""),
            ("u8", "type", ""),
            ("u16", "type", ""),
            ("u32", "type", ""),
            ("u64", "type", ""),
            ("i32", "type", ""),
            ("i64", "type", ""),
            ("f32", "type", ""),
            ("f64", "type", ""),
            ("bool", "type", ""),
            ("Some", "const", "Option::Some"),
            ("None", "const", "Option::None"),
            ("Ok", "const", "Result::Ok"),
            ("Err", "const", "Result::Err"),
            ("println!", "macro", "stdout line"),
            ("eprintln!", "macro", "stderr line"),
            ("format!", "macro", "format String"),
            ("vec!", "macro", "create Vec"),
            ("assert_eq!", "macro", ""),
            ("todo!", "macro", ""),
        ]),
        Language::C => make(&[
            ("int", "type", ""),
            ("void", "type", ""),
            ("char", "type", ""),
            ("float", "type", ""),
            ("double", "type", ""),
            ("bool", "type", "<stdbool.h>"),
            ("size_t", "type", "<stddef.h>"),
            ("ssize_t", "type", "<sys/types.h>"),
            ("uintptr_t", "type", "<stdint.h>"),
            ("uint8_t", "type", "<stdint.h>"),
            ("uint16_t", "type", "<stdint.h>"),
            ("uint32_t", "type", "<stdint.h>"),
            ("uint64_t", "type", "<stdint.h>"),
            ("int8_t", "type", "<stdint.h>"),
            ("int16_t", "type", "<stdint.h>"),
            ("int32_t", "type", "<stdint.h>"),
            ("int64_t", "type", "<stdint.h>"),
            ("const", "keyword", ""),
            ("static", "keyword", ""),
            ("extern", "keyword", ""),
            ("volatile", "keyword", ""),
            ("inline", "keyword", ""),
            ("struct", "keyword", ""),
            ("union", "keyword", ""),
            ("enum", "keyword", ""),
            ("typedef", "keyword", ""),
            ("sizeof", "keyword", ""),
            ("return", "keyword", ""),
            ("switch", "keyword", ""),
            ("default", "keyword", ""),
            ("while", "keyword", ""),
            ("break", "keyword", ""),
            ("continue", "keyword", ""),
            ("NULL", "const", ""),
            ("true", "const", ""),
            ("false", "const", ""),
            ("printf", "builtin", "<stdio.h>"),
            ("fprintf", "builtin", "<stdio.h>"),
            ("snprintf", "builtin", "<stdio.h>"),
            ("malloc", "builtin", "<stdlib.h>"),
            ("calloc", "builtin", "<stdlib.h>"),
            ("realloc", "builtin", "<stdlib.h>"),
            ("free", "builtin", "<stdlib.h>"),
            ("memcpy", "builtin", "<string.h>"),
            ("memmove", "builtin", "<string.h>"),
            ("memset", "builtin", "<string.h>"),
            ("strlen", "builtin", "<string.h>"),
            ("strcmp", "builtin", "<string.h>"),
            ("assert", "builtin", "<assert.h>"),
        ]),
        Language::Cpp => make(&[
            ("class", "keyword", ""),
            ("struct", "keyword", ""),
            ("namespace", "keyword", ""),
            ("template", "keyword", ""),
            ("typename", "keyword", ""),
            ("constexpr", "keyword", ""),
            ("consteval", "keyword", ""),
            ("const", "keyword", ""),
            ("auto", "keyword", ""),
            ("virtual", "keyword", ""),
            ("override", "keyword", ""),
            ("noexcept", "keyword", ""),
            ("nullptr", "const", ""),
            ("public", "keyword", ""),
            ("private", "keyword", ""),
            ("protected", "keyword", ""),
            ("return", "keyword", ""),
            ("static_cast", "keyword", ""),
            ("reinterpret_cast", "keyword", ""),
            ("size_t", "type", ""),
            ("uint32_t", "type", "<cstdint>"),
            ("uint64_t", "type", "<cstdint>"),
            ("int32_t", "type", "<cstdint>"),
            ("int64_t", "type", "<cstdint>"),
            ("std::vector", "type", "<vector>"),
            ("std::string", "type", "<string>"),
            ("std::string_view", "type", "<string_view>"),
            ("std::unique_ptr", "type", "<memory>"),
            ("std::shared_ptr", "type", "<memory>"),
            ("std::optional", "type", "<optional>"),
            ("std::array", "type", "<array>"),
            ("std::span", "type", "<span>"),
            ("std::cout", "builtin", "<iostream>"),
            ("std::cerr", "builtin", "<iostream>"),
            ("std::endl", "builtin", "<iostream>"),
            ("std::move", "builtin", "<utility>"),
        ]),
        Language::Zig => make(&[
            ("const", "keyword", ""),
            ("var", "keyword", ""),
            ("pub", "keyword", ""),
            ("fn", "keyword", ""),
            ("struct", "keyword", ""),
            ("enum", "keyword", ""),
            ("union", "keyword", ""),
            ("error", "keyword", ""),
            ("defer", "keyword", ""),
            ("errdefer", "keyword", ""),
            ("comptime", "keyword", ""),
            ("inline", "keyword", ""),
            ("try", "keyword", ""),
            ("catch", "keyword", ""),
            ("orelse", "keyword", ""),
            ("switch", "keyword", ""),
            ("while", "keyword", ""),
            ("for", "keyword", ""),
            ("return", "keyword", ""),
            ("unreachable", "keyword", ""),
            ("undefined", "const", ""),
            ("null", "const", ""),
            ("true", "const", ""),
            ("false", "const", ""),
            ("usize", "type", ""),
            ("isize", "type", ""),
            ("u8", "type", ""),
            ("u16", "type", ""),
            ("u32", "type", ""),
            ("u64", "type", ""),
            ("i32", "type", ""),
            ("i64", "type", ""),
            ("f32", "type", ""),
            ("f64", "type", ""),
            ("bool", "type", ""),
            ("void", "type", ""),
            ("anyerror", "type", ""),
            ("@import", "builtin", "import module"),
            ("@as", "builtin", "type coercion"),
            ("@intCast", "builtin", ""),
            ("@floatCast", "builtin", ""),
            ("@ptrCast", "builtin", ""),
            ("@alignCast", "builtin", ""),
            ("@sizeOf", "builtin", ""),
            ("@typeInfo", "builtin", ""),
            ("@embedFile", "builtin", ""),
            ("std.debug.print", "builtin", "stderr formatted print"),
            ("std.ArrayList", "type", ""),
            ("std.mem.eql", "builtin", ""),
        ]),
        Language::Fortran => make(&[
            ("program", "keyword", ""),
            ("module", "keyword", ""),
            ("submodule", "keyword", "F2008"),
            ("subroutine", "keyword", ""),
            ("function", "keyword", ""),
            ("contains", "keyword", ""),
            ("implicit none", "keyword", ""),
            ("use", "keyword", ""),
            ("only", "keyword", ""),
            ("intent(in)", "keyword", ""),
            ("intent(out)", "keyword", ""),
            ("intent(inout)", "keyword", ""),
            ("allocatable", "keyword", ""),
            ("dimension", "keyword", ""),
            ("parameter", "keyword", ""),
            ("contiguous", "keyword", "F2008"),
            ("optional", "keyword", ""),
            ("pointer", "keyword", ""),
            ("target", "keyword", ""),
            ("pure", "keyword", ""),
            ("elemental", "keyword", ""),
            ("recursive", "keyword", ""),
            ("interface", "keyword", ""),
            ("abstract", "keyword", ""),
            ("extends", "keyword", ""),
            ("select case", "keyword", ""),
            ("select type", "keyword", ""),
            ("do concurrent", "keyword", "F2008"),
            ("allocate", "keyword", ""),
            ("deallocate", "keyword", ""),
            ("call", "keyword", ""),
            ("return", "keyword", ""),
            ("cycle", "keyword", ""),
            ("exit", "keyword", ""),
            ("print", "keyword", ""),
            ("write", "keyword", ""),
            ("read", "keyword", ""),
            ("end program", "keyword", ""),
            ("end module", "keyword", ""),
            ("end submodule", "keyword", ""),
            ("end subroutine", "keyword", ""),
            ("end function", "keyword", ""),
            ("end interface", "keyword", ""),
            ("end type", "keyword", ""),
            ("end do", "keyword", ""),
            ("end if", "keyword", ""),
            ("integer", "type", ""),
            ("real", "type", ""),
            ("complex", "type", ""),
            ("logical", "type", ""),
            ("character", "type", ""),
            ("real32", "type", "iso_fortran_env"),
            ("real64", "type", "iso_fortran_env"),
            ("int32", "type", "iso_fortran_env"),
            ("int64", "type", "iso_fortran_env"),
            ("iso_fortran_env", "module", "intrinsic"),
            ("iso_c_binding", "module", "intrinsic"),
            ("ieee_arithmetic", "module", "intrinsic"),
            ("omp_lib", "module", "OpenMP"),
            ("output_unit", "const", "iso_fortran_env"),
            ("error_unit", "const", "iso_fortran_env"),
            ("allocated", "builtin", "inquiry"),
            ("present", "builtin", "inquiry"),
            ("associated", "builtin", "inquiry"),
            ("size", "builtin", "array size"),
            ("shape", "builtin", "array shape"),
            ("lbound", "builtin", "lower bound"),
            ("ubound", "builtin", "upper bound"),
            ("sum", "builtin", "reduction"),
            ("product", "builtin", "reduction"),
            ("matmul", "builtin", "matrix multiply"),
            ("dot_product", "builtin", "dot product"),
            ("transpose", "builtin", "matrix transpose"),
            ("reshape", "builtin", "array reshape"),
            ("maxval", "builtin", "reduction"),
            ("minval", "builtin", "reduction"),
            ("maxloc", "builtin", "location"),
            ("minloc", "builtin", "location"),
            ("move_alloc", "builtin", "transfer allocation"),
            ("trim", "builtin", "string"),
            ("adjustl", "builtin", "string"),
            ("len_trim", "builtin", "string"),
            ("execute_command_line", "builtin", "F2008"),
            ("get_command_argument", "builtin", "F2003"),
            ("get_environment_variable", "builtin", "F2003"),
            ("this_image", "builtin", "coarray"),
            ("num_images", "builtin", "coarray"),
            ("co_sum", "builtin", "F2018 coarray"),
        ]),
        Language::R => make(&[
            ("function", "keyword", ""),
            ("if", "keyword", ""),
            ("else", "keyword", ""),
            ("for", "keyword", ""),
            ("while", "keyword", ""),
            ("repeat", "keyword", ""),
            ("return", "keyword", ""),
            ("next", "keyword", ""),
            ("break", "keyword", ""),
            ("library", "keyword", "load package"),
            ("require", "keyword", "load package"),
            ("source", "keyword", "run R file"),
            ("tryCatch", "keyword", "error handling"),
            ("stop", "keyword", "throw error"),
            ("warning", "keyword", ""),
            ("message", "keyword", ""),
            ("TRUE", "const", ""),
            ("FALSE", "const", ""),
            ("NULL", "const", ""),
            ("NA", "const", "missing value"),
            ("NA_real_", "const", ""),
            ("NA_integer_", "const", ""),
            ("NA_character_", "const", ""),
            ("Inf", "const", ""),
            ("NaN", "const", ""),
            ("numeric", "type", ""),
            ("integer", "type", ""),
            ("character", "type", ""),
            ("logical", "type", ""),
            ("list", "type", ""),
            ("matrix", "type", ""),
            ("array", "type", ""),
            ("factor", "type", ""),
            ("data.frame", "type", ""),
            ("tibble", "type", "tibble"),
            ("cat", "builtin", "base"),
            ("print", "builtin", "base"),
            ("paste", "builtin", "base"),
            ("paste0", "builtin", "base"),
            ("sprintf", "builtin", "base"),
            ("length", "builtin", "base"),
            ("nrow", "builtin", "base"),
            ("ncol", "builtin", "base"),
            ("dim", "builtin", "base"),
            ("names", "builtin", "base"),
            ("colnames", "builtin", "base"),
            ("rownames", "builtin", "base"),
            ("summary", "builtin", "base"),
            ("str", "builtin", "utils"),
            ("head", "builtin", "utils"),
            ("tail", "builtin", "utils"),
            ("seq", "builtin", "base"),
            ("seq_len", "builtin", "base"),
            ("seq_along", "builtin", "base"),
            ("rep", "builtin", "base"),
            ("sort", "builtin", "base"),
            ("order", "builtin", "base"),
            ("unique", "builtin", "base"),
            ("which", "builtin", "base"),
            ("ifelse", "builtin", "base"),
            ("is.na", "builtin", "base"),
            ("is.null", "builtin", "base"),
            ("is.finite", "builtin", "base"),
            ("as.numeric", "builtin", "base"),
            ("as.integer", "builtin", "base"),
            ("as.character", "builtin", "base"),
            ("as.logical", "builtin", "base"),
            ("as.data.frame", "builtin", "base"),
            ("sum", "builtin", "base"),
            ("mean", "builtin", "base"),
            ("median", "builtin", "stats"),
            ("sd", "builtin", "stats"),
            ("var", "builtin", "stats"),
            ("min", "builtin", "base"),
            ("max", "builtin", "base"),
            ("quantile", "builtin", "stats"),
            ("cor", "builtin", "stats"),
            ("lm", "builtin", "stats"),
            ("glm", "builtin", "stats"),
            ("predict", "builtin", "stats"),
            ("rnorm", "builtin", "stats"),
            ("runif", "builtin", "stats"),
            ("set.seed", "builtin", "base"),
            ("sample", "builtin", "base"),
            ("apply", "builtin", "base"),
            ("lapply", "builtin", "base"),
            ("sapply", "builtin", "base"),
            ("vapply", "builtin", "base"),
            ("Reduce", "builtin", "base"),
            ("Filter", "builtin", "base"),
            ("Map", "builtin", "base"),
            ("subset", "builtin", "base"),
            ("merge", "builtin", "base"),
            ("rbind", "builtin", "base"),
            ("cbind", "builtin", "base"),
            ("read.csv", "builtin", "utils"),
            ("write.csv", "builtin", "utils"),
            ("readRDS", "builtin", "base"),
            ("saveRDS", "builtin", "base"),
            ("readLines", "builtin", "base"),
            ("writeLines", "builtin", "base"),
            ("file.path", "builtin", "base"),
            ("commandArgs", "builtin", "base"),
            ("plot", "builtin", "graphics"),
            ("ggplot", "builtin", "ggplot2"),
            ("mutate", "builtin", "dplyr"),
            ("filter", "builtin", "dplyr"),
            ("select", "builtin", "dplyr"),
            ("summarise", "builtin", "dplyr"),
            ("group_by", "builtin", "dplyr"),
        ]),
        Language::Python => make(&[
            ("def", "keyword", ""),
            ("class", "keyword", ""),
            ("import", "keyword", ""),
            ("from", "keyword", ""),
            ("return", "keyword", ""),
            ("yield", "keyword", ""),
            ("async", "keyword", ""),
            ("await", "keyword", ""),
            ("with", "keyword", ""),
            ("try", "keyword", ""),
            ("except", "keyword", ""),
            ("finally", "keyword", ""),
            ("raise", "keyword", ""),
            ("lambda", "keyword", ""),
            ("None", "const", ""),
            ("True", "const", ""),
            ("False", "const", ""),
            ("print", "builtin", ""),
            ("len", "builtin", ""),
            ("range", "builtin", ""),
            ("enumerate", "builtin", ""),
            ("zip", "builtin", ""),
            ("isinstance", "builtin", ""),
            ("dict", "type", ""),
            ("list", "type", ""),
            ("tuple", "type", ""),
            ("set", "type", ""),
            ("str", "type", ""),
            ("int", "type", ""),
            ("float", "type", ""),
            ("bool", "type", ""),
        ]),
        Language::Shell => make(&[
            ("if", "keyword", ""),
            ("then", "keyword", ""),
            ("elif", "keyword", ""),
            ("else", "keyword", ""),
            ("fi", "keyword", ""),
            ("for", "keyword", ""),
            ("while", "keyword", ""),
            ("do", "keyword", ""),
            ("done", "keyword", ""),
            ("case", "keyword", ""),
            ("esac", "keyword", ""),
            ("function", "keyword", ""),
            ("local", "keyword", ""),
            ("export", "keyword", ""),
            ("return", "keyword", ""),
            ("exit", "keyword", ""),
            ("printf", "builtin", ""),
            ("echo", "builtin", ""),
            ("set", "builtin", ""),
        ]),
        Language::JavaScript | Language::TypeScript => make(&[
            ("function", "keyword", ""),
            ("const", "keyword", ""),
            ("let", "keyword", ""),
            ("return", "keyword", ""),
            ("async", "keyword", ""),
            ("await", "keyword", ""),
            ("import", "keyword", ""),
            ("export", "keyword", ""),
            ("class", "keyword", ""),
            ("interface", "keyword", ""),
            ("type", "keyword", ""),
            ("null", "const", ""),
            ("undefined", "const", ""),
            ("true", "const", ""),
            ("false", "const", ""),
            ("console.log", "builtin", ""),
            ("Promise", "type", ""),
            ("Array", "type", ""),
            ("Map", "type", ""),
            ("Set", "type", ""),
        ]),
        Language::Asm | Language::Toml | Language::Json | Language::Plain => Vec::new(),
    }
}

/// True when `wanted` (lower-case) matches a word segment after `_`, `.`, `:`, `-`,
/// or a camelCase uppercase boundary in `text`, or matches segment initials (e.g. `mv` -> `mean_value`).
fn matches_boundary(text: &str, wanted: &str) -> bool {
    if wanted.is_empty() {
        return false;
    }
    let mut prev: Option<char> = None;
    let mut initials = String::new();
    for (idx, ch) in text.char_indices() {
        let is_start = match prev {
            None => true,
            Some(p) => {
                matches!(p, '_' | '.' | ':' | '-' | '/') || (p.is_lowercase() && ch.is_uppercase())
            }
        };
        if is_start {
            if idx > 0 && text[idx..].to_lowercase().starts_with(wanted) {
                return true;
            }
            for lc in ch.to_lowercase() {
                initials.push(lc);
            }
        }
        prev = Some(ch);
    }
    initials.len() >= 2 && initials.starts_with(wanted)
}

/// How well `query` matches `text`: lower is better, None is no match.
/// Exact-case prefix (0) < case-insensitive prefix (1) < segment/camelCase boundary (2) <
/// in-order subsequence starting with the same character (3).
fn score(text: &str, query: &str) -> Option<u8> {
    if query.is_empty() || text.starts_with(query) {
        return Some(0);
    }
    let lower = text.to_lowercase();
    let wanted = query.to_lowercase();
    if lower.starts_with(&wanted) {
        return Some(1);
    }
    if matches_boundary(text, &wanted) {
        return Some(2);
    }
    // In-order subsequence that starts with the same character: `cou`
    // finds `compute`, but `le` does not find `splice`.
    let mut chars = lower.chars();
    let mut wanted_chars = wanted.chars();
    let first = wanted_chars.next()?;
    (chars.next() == Some(first) && wanted_chars.all(|q| chars.any(|c| c == q))).then_some(3)
}

/// Indices of `items` matching `query`, best first, at most `limit`.
/// Local words identical to the query itself are left out.
pub fn rank(items: &[Candidate], query: &str, limit: usize) -> Vec<usize> {
    let mut hits: Vec<(u8, &str, usize, usize)> = items
        .iter()
        .enumerate()
        .filter(|(_, c)| {
            c.insert != query
                || !matches!(c.kind.as_str(), "word" | "workspace" | "keyword" | "type")
        })
        .filter_map(|(i, c)| {
            score(&c.filter, query).map(|s| (s, c.sort.as_str(), c.label.len(), i))
        })
        .collect();
    hits.sort();
    hits.into_iter().take(limit).map(|(.., i)| i).collect()
}

/// Plain text of an LSP snippet: `${1:name}` → `name`, `$1`/`$0` removed,
/// `\$` → `$`, choices `${1|a,b|}` → `a`.
pub fn strip_snippet(snippet: &str) -> String {
    let mut out = String::with_capacity(snippet.len());
    let mut chars = snippet.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            }
            '$' => match chars.peek() {
                Some(c) if c.is_ascii_digit() => {
                    while chars.peek().is_some_and(char::is_ascii_digit) {
                        chars.next();
                    }
                }
                Some('{') => {
                    chars.next();
                    while chars.peek().is_some_and(char::is_ascii_digit) {
                        chars.next();
                    }
                    let mut depth = 1;
                    let mut body = String::new();
                    let choice = chars.peek() == Some(&'|');
                    if matches!(chars.peek(), Some(':' | '|')) {
                        chars.next();
                    }
                    while let Some(c) = chars.next() {
                        match c {
                            '\\' => {
                                if let Some(n) = chars.next() {
                                    body.push(n);
                                }
                            }
                            '{' => {
                                depth += 1;
                                body.push(c);
                            }
                            '}' => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                                body.push(c);
                            }
                            _ => body.push(c),
                        }
                    }
                    if choice {
                        body = body
                            .trim_end_matches('|')
                            .split(',')
                            .next()
                            .unwrap_or_default()
                            .into();
                    }
                    out.push_str(&strip_snippet(&body));
                }
                _ => out.push('$'),
            },
            _ => out.push(ch),
        }
    }
    out
}
