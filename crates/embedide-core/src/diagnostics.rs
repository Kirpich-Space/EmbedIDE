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
    for severity in ["fatal error", "error", "warning", "note"] {
        let marker = format!(": {severity}: ");
        let Some((location, message)) = line.split_once(&marker) else {
            continue;
        };
        let mut parts = location.rsplitn(3, ':');
        let column = parts.next()?.trim().parse::<usize>().ok()?;
        let row = parts.next()?.trim().parse::<usize>().ok()?;
        let file = parts.next()?;
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
