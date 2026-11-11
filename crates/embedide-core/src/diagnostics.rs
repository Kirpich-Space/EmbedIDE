//! Conservative compiler diagnostics, independent of terminal rendering.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// One compiler location. Lines and columns are one based.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// error, warning, note or help.
    pub severity: String,
    /// Compiler explanation.
    pub message: String,
    /// May refer outside the workspace.
    pub file: Option<PathBuf>,
    /// One based source line.
    pub line: usize,
    /// One based Unicode scalar column, as emitted by the compiler.
    pub column: usize,
    /// Optional diagnostic identifier.
    pub code: Option<String>,
    /// Producing tool.
    pub source: String,
}

/// Parse Cargo JSON first, then GCC/Clang `path:line:column: severity: text`.
/// Human output without a reliable location remains in the output console.
pub fn parse(line: &str) -> Option<Diagnostic> {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
        if value["reason"] != "compiler-message" {
            return None;
        }
        let message = &value["message"];
        let span = message["spans"]
            .as_array()?
            .iter()
            .find(|s| s["is_primary"] == true);
        return Some(Diagnostic {
            severity: message["level"].as_str()?.into(),
            message: message["message"].as_str()?.into(),
            file: span
                .and_then(|s| s["file_name"].as_str())
                .map(PathBuf::from),
            line: span.and_then(|s| s["line_start"].as_u64()).unwrap_or(1) as usize,
            column: span.and_then(|s| s["column_start"].as_u64()).unwrap_or(1) as usize,
            code: message["code"]["code"].as_str().map(str::to_owned),
            source: "rustc".into(),
        });
    }
    // GCC/Clang/llvm-mc: `path:line:column: severity: text`; NASM:
    // `path:line: error: text`; GNU as: `path:line: Error: text`.
    for (marker, severity) in [
        (": fatal error: ", "fatal error"),
        (": error: ", "error"),
        (": Error: ", "error"),
        (": warning: ", "warning"),
        (": Warning: ", "warning"),
        (": note: ", "note"),
    ] {
        let Some((location, message)) = line.split_once(marker) else {
            continue;
        };
        let number = |s: &str| s.trim().parse::<usize>().ok();
        let mut parts = location.rsplitn(3, ':');
        let (last, middle, first) = (parts.next(), parts.next(), parts.next());
        let (file, row, column) = match (first, middle.and_then(number), last.and_then(number)) {
            (Some(file), Some(row), Some(column)) => (file, row, column),
            _ => match (middle, last.and_then(number)) {
                // No column: `path:line`, where the path may itself contain ':'.
                (Some(_), Some(row)) => (
                    location.rsplit_once(':').map_or(location, |(f, _)| f),
                    row,
                    1,
                ),
                _ => continue,
            },
        };
        if file.is_empty() || file.starts_with(' ') {
            continue;
        }
        return Some(Diagnostic {
            severity: severity.into(),
            message: message.into(),
            file: Some(file.into()),
            line: row.max(1),
            column: column.max(1),
            code: None,
            source: "compiler".into(),
        });
    }
    if line.starts_with("CMake Error") {
        return Some(Diagnostic {
            severity: "error".into(),
            message: line.into(),
            file: None,
            line: 1,
            column: 1,
            code: None,
            source: "cmake".into(),
        });
    }
    None
}
