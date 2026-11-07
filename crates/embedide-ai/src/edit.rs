//! AI Edit: the model proposes a new version of one file; EmbedIDE shows a
//! unified diff. Applying it is a separate, explicit step that refuses to
//! write if the file changed after the proposal was made.
use crate::{Backend, Cancel, Error, Message, Request, Result};
use serde::Serialize;
use std::path::Path;

/// Instructions for edits.
pub const SYSTEM: &str =
    "You edit one source file inside an engineering IDE. Reply with the COMPLETE new \
content of the file and nothing else: no explanation, no markdown fences. Keep everything that the \
instruction does not ask you to change exactly as it is.";

/// A proposed change.
#[derive(Debug, Clone, Serialize)]
pub struct Proposal {
    /// File path.
    pub path: String,
    /// Content the proposal was based on.
    #[serde(skip)]
    pub original: String,
    /// Proposed content.
    #[serde(skip)]
    pub proposed: String,
    /// Unified diff.
    pub diff: String,
}

/// Build the request (no network).
pub fn request(
    path: &str,
    content: &str,
    instruction: &str,
    model: &str,
    max_tokens: u32,
) -> Request {
    Request {
        model: model.into(),
        system: Some(SYSTEM.into()),
        messages: vec![Message::user(format!(
            "File: {path}\n```\n{content}\n```\n\nInstruction: {instruction}"
        ))],
        max_tokens,
        temperature: Some(0.0),
        stream: false,
    }
}

fn strip_fences(s: &str) -> String {
    let t = s.trim();
    if t.starts_with("```") {
        let lines: Vec<&str> = t.lines().skip(1).collect();
        let lines = if lines.last().is_some_and(|l| l.trim() == "```") {
            &lines[..lines.len() - 1]
        } else {
            &lines[..]
        };
        let mut out = lines.join("\n");
        out.push('\n');
        out
    } else {
        s.to_string()
    }
}

/// Ask for an edit and diff it.
pub fn propose(
    backend: &dyn Backend,
    path: &str,
    content: &str,
    instruction: &str,
    model: &str,
    max_tokens: u32,
    cancel: &Cancel,
) -> Result<Proposal> {
    let resp = backend.send(
        &request(path, content, instruction, model, max_tokens),
        cancel,
        &mut |_| {},
    )?;
    if resp
        .stop_reason
        .as_deref()
        .is_some_and(|r| r == "max_tokens" || r == "length")
    {
        return Err(Error::Provider(
            0,
            "The reply was cut off by the token limit; raise ai.max_tokens or edit a smaller file"
                .into(),
        ));
    }
    let proposed = strip_fences(&resp.text);
    Ok(Proposal {
        path: path.into(),
        diff: unified_diff(path, content, &proposed),
        original: content.into(),
        proposed,
    })
}

/// Write a proposal only if the file still holds the content it was based on.
pub fn apply(file: &Path, p: &Proposal) -> Result<()> {
    let now = std::fs::read_to_string(file)
        .map_err(|e| Error::Config(format!("Cannot read {}: {e}", file.display())))?;
    if now != p.original {
        return Err(Error::Config(format!(
            "{} changed after the proposal; nothing was written",
            file.display()
        )));
    }
    embedide_workspace::write_atomic(file, p.proposed.as_bytes(), false)
        .map_err(|e| Error::Config(e.0))
}

/// Line-based unified diff (LCS), 3 lines of context.
pub fn unified_diff(path: &str, a: &str, b: &str) -> String {
    let x: Vec<&str> = a.lines().collect();
    let y: Vec<&str> = b.lines().collect();
    if x == y {
        return String::new();
    }
    let (n, m) = (x.len(), y.len());
    let mut lcs = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if x[i] == y[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    // ops: (' ' | '-' | '+', a_index, b_index, text)
    let mut ops = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && x[i] == y[j] {
            ops.push((' ', i, j, x[i]));
            i += 1;
            j += 1;
        } else if i < n && (j == m || lcs[i + 1][j] >= lcs[i][j + 1]) {
            // deletions before insertions, as diff(1) prints them
            ops.push(('-', i, j, x[i]));
            i += 1;
        } else {
            ops.push(('+', i, j, y[j]));
            j += 1;
        }
    }
    let mut out = format!("--- a/{path}\n+++ b/{path}\n");
    let changed: Vec<usize> = ops
        .iter()
        .enumerate()
        .filter(|(_, o)| o.0 != ' ')
        .map(|(k, _)| k)
        .collect();
    let mut k = 0;
    while k < changed.len() {
        let start = changed[k].saturating_sub(3);
        let mut end = changed[k];
        while k < changed.len() && changed[k] <= end + 6 {
            end = changed[k];
            k += 1;
        }
        let end = (end + 3).min(ops.len() - 1);
        let hunk = &ops[start..=end];
        let a0 = hunk
            .iter()
            .find(|o| o.0 != '+')
            .map(|o| o.1)
            .unwrap_or(hunk[0].1);
        let b0 = hunk
            .iter()
            .find(|o| o.0 != '-')
            .map(|o| o.2)
            .unwrap_or(hunk[0].2);
        let al = hunk.iter().filter(|o| o.0 != '+').count();
        let bl = hunk.iter().filter(|o| o.0 != '-').count();
        out.push_str(&format!("@@ -{},{al} +{},{bl} @@\n", a0 + 1, b0 + 1));
        for o in hunk {
            out.push(o.0);
            out.push_str(o.3);
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_shows_only_changes_with_context() {
        let a = "a\nb\nc\nd\ne\nf\ng\nh\n";
        let b = "a\nb\nc\nD\ne\nf\ng\nh\n";
        let d = unified_diff("x.c", a, b);
        assert!(d.contains("-d\n+D\n"));
        assert!(d.contains("@@ -1,7 +1,7 @@"), "{d}");
        assert_eq!(unified_diff("x", a, a), "");
    }

    #[test]
    fn apply_refuses_when_file_changed() {
        let f = std::env::temp_dir().join(format!("embedide-edit-{}.c", std::process::id()));
        std::fs::write(&f, "old\n").unwrap();
        let p = Proposal {
            path: "x".into(),
            original: "old\n".into(),
            proposed: "new\n".into(),
            diff: String::new(),
        };
        std::fs::write(&f, "changed by user\n").unwrap();
        assert!(apply(&f, &p).is_err());
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "changed by user\n");
        std::fs::write(&f, "old\n").unwrap();
        apply(&f, &p).unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "new\n");
        std::fs::remove_file(&f).unwrap();
    }
}
