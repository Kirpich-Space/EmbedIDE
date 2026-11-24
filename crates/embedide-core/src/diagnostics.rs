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
    // R `parse(file=...)` errors: `Error in parse(...) : file.R:3:1: unexpected symbol`
    // or continuation line `  file.R:3:1: unexpected symbol`.
    if let Some(diag) = parse_r_location_message(line) {
        return Some(diag);
    }
    None
}

/// Parse R `parse()` diagnostics: `[Error in parse(...) : ]path.R:line:col: message`.
fn parse_r_location_message(line: &str) -> Option<Diagnostic> {
    let trimmed = line.trim();
    let body = if let Some(rest) = trimmed.strip_prefix("Error in ") {
        rest.split_once(" : ").map(|(_, tail)| tail.trim())?
    } else if line.starts_with(' ') {
        trimmed
    } else {
        return None;
    };
    let (loc, message) = body.split_once(": ")?;
    let mut parts = loc.rsplitn(3, ':');
    let col = parts.next()?.trim().parse::<usize>().ok()?;
    let row = parts.next()?.trim().parse::<usize>().ok()?;
    let file = parts.next()?;
    let lower = file.to_ascii_lowercase();
    if file.is_empty()
        || (!lower.ends_with(".r") && !lower.ends_with(".rmd") && !lower.ends_with(".qmd"))
    {
        return None;
    }
    Some(Diagnostic {
        severity: "error".into(),
        message: message.trim().into(),
        file: Some(PathBuf::from(file)),
        line: row.max(1),
        column: col.max(1),
        code: None,
        source: "R".into(),
    })
}

/// Stateful line parser. It understands everything [`parse`] does and also gfortran's layout, where
/// the location, a source excerpt and the message are on separate lines:
///
/// ```text
/// src/physics.f90:11:27:
///
///    11 |       u(i) = t(i) + 0.25 +*(t(i-1))
///       |                           1
/// Error: Syntax error in expression at (1)
/// ```
///
/// Feed output lines in order; a diagnostic is returned when its message line arrives.
#[derive(Debug, Default, Clone)]
pub struct Parser {
    pending: Option<(PathBuf, usize, usize)>,
}

/// `path:line:col:`, `path:line:col-col:` or the older `path:line.col:` with nothing after the colon.
fn location_only(line: &str) -> Option<(PathBuf, usize, usize)> {
    let body = line.trim_end().strip_suffix(':')?;
    if body.is_empty() || body.starts_with(' ') {
        return None;
    }
    // Older gfortran: `file.f90:12.5`
    if let Some((file, rest)) = body.rsplit_once(':') {
        if let Some((row, col)) = rest.split_once('.') {
            if let (Ok(row), Ok(col)) = (row.parse::<usize>(), col.parse::<usize>()) {
                return Some((PathBuf::from(file), row.max(1), col.max(1)));
            }
        }
    }
    let mut parts = body.rsplitn(3, ':');
    let col = parts.next()?;
    let row = parts.next()?;
    let file = parts.next()?;
    // A column range (`27-29`) keeps its first column.
    let col = col.split('-').next()?;
    let (Ok(row), Ok(col)) = (row.trim().parse::<usize>(), col.trim().parse::<usize>()) else {
        return None;
    };
    (!file.is_empty()).then(|| (PathBuf::from(file), row.max(1), col.max(1)))
}

impl Parser {
    /// Forget any half-read diagnostic (call at the start of a new build).
    pub fn reset(&mut self) {
        self.pending = None;
    }

    /// Consume one output line.
    pub fn feed(&mut self, line: &str) -> Option<Diagnostic> {
        let line = line.trim_end();
        if let Some(loc) = location_only(line) {
            self.pending = Some(loc);
            return None;
        }
        if let Some((file, row, column)) = self.pending.clone() {
            for (marker, severity) in [
                ("Fatal Error: ", "fatal error"),
                ("Internal Error: ", "error"),
                ("Error: ", "error"),
                ("Warning: ", "warning"),
                ("Note: ", "note"),
            ] {
                if let Some(rest) = line.strip_prefix(marker) {
                    self.pending = None;
                    // `... [-Wunused-variable]` carries the warning option as a code.
                    let (message, code) = match rest.rsplit_once(" [") {
                        Some((m, c)) if c.ends_with(']') && c.starts_with('-') => {
                            (m.trim_end(), Some(c.trim_end_matches(']').to_string()))
                        }
                        _ => (rest, None),
                    };
                    return Some(Diagnostic {
                        severity: severity.into(),
                        message: message.into(),
                        file: Some(file),
                        line: row,
                        column,
                        code,
                        source: "compiler".into(),
                    });
                }
            }
            // Source excerpt, caret line or blank line: keep waiting for the message.
            let t = line.trim_start();
            let excerpt = t.is_empty()
                || t.starts_with('|')
                || t.chars().next().is_some_and(|c| c.is_ascii_digit());
            if !excerpt {
                self.pending = None;
            }
        }
        parse(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed_all(lines: &[&str]) -> Vec<Diagnostic> {
        let mut p = Parser::default();
        lines.iter().filter_map(|l| p.feed(l)).collect()
    }

    #[test]
    fn gfortran_multi_line_errors_and_warnings() {
        let d = feed_all(&[
            "src/physics.f90:11:27:",
            "",
            "   11 |       u(i) = t(i) + 0.25 +*(t(i-1))",
            "      |                           1",
            "Error: Syntax error in expression at (1)",
            "a_main.f90:12:31:",
            "",
            "   12 |   print '(a,f8.3)', 'mid=', t(n/2)",
            "      |                               1",
            "Warning: Integer division truncated to constant '5' at (1) [-Winteger-division]",
        ]);
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].severity, "error");
        assert_eq!(
            (
                d[0].file.as_deref().unwrap().to_str().unwrap(),
                d[0].line,
                d[0].column
            ),
            ("src/physics.f90", 11, 27)
        );
        assert_eq!(d[0].message, "Syntax error in expression at (1)");
        assert_eq!(d[1].severity, "warning");
        assert_eq!(d[1].code.as_deref(), Some("-Winteger-division"));
        assert_eq!((d[1].line, d[1].column), (12, 31));
    }

    #[test]
    fn gfortran_column_ranges_and_old_dot_style() {
        let d = feed_all(&[
            "x.f90:3:5-9:",
            "",
            "Error: bad",
            "y.f:7.2:",
            "Fatal Error: Cannot open module file 'm.mod'",
        ]);
        assert_eq!((d[0].line, d[0].column), (3, 5));
        assert_eq!(
            (d[1].line, d[1].column, d[1].severity.as_str()),
            (7, 2, "fatal error")
        );
    }

    #[test]
    fn single_line_formats_still_work_and_stale_locations_are_dropped() {
        let d = feed_all(&[
            "a.c:1:2: error: boom",
            "b.f90:4:1:",
            "unrelated text",
            "Error: not attached",
        ]);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].file.as_deref().unwrap().to_str().unwrap(), "a.c");
    }

    #[test]
    fn r_parse_errors_single_and_multiline() {
        let d = feed_all(&[
            "Error in parse(file = commandArgs(trailingOnly = TRUE)[1]) : ",
            "  main.R:4:7: unexpected symbol",
            "Error in parse(file = \"a.R\") : a.R:2:1: unexpected ')'",
        ]);
        assert_eq!(d.len(), 2);
        assert_eq!(
            (
                d[0].file.as_deref().unwrap().to_str().unwrap(),
                d[0].line,
                d[0].column,
                d[0].message.as_str()
            ),
            ("main.R", 4, 7, "unexpected symbol")
        );
        assert_eq!(
            (
                d[1].file.as_deref().unwrap().to_str().unwrap(),
                d[1].line,
                d[1].column,
                d[1].message.as_str()
            ),
            ("a.R", 2, 1, "unexpected ')'")
        );
    }
}
