//! AI chat about code, diagnostics and architecture.
use crate::{context::Context, Backend, Cancel, Message, Request, Response, Result};

/// System instructions for chat.
pub const SYSTEM: &str = "You are the assistant inside EmbedIDE, an engineering IDE for embedded firmware, \
drivers, kernels, systems and scientific code (C, C++, Rust, Zig, assembly, Fortran). Be precise and concise. \
When you propose code changes, show unified diffs or complete files. Never claim to have compiled, run or \
flashed anything: you cannot. Attached files are delimited by ``` fences. This is EmbedIDE's chat mode: \
here you cannot read the project or change files. When the user wants files changed, code written into the \
project, or a build run, tell them to run it as an agent task in the AI panel: `/agent <what to do>` (or \
`/mode agent` and then type the task); in agent mode you work in the project directly.";

/// Build the request without sending it (used for --dry-run).
pub fn request(
    model: &str,
    history: &[Message],
    user_text: &str,
    context: Option<&Context>,
    max_tokens: u32,
    stream: bool,
) -> Request {
    let mut messages = history.to_vec();
    let content = match context.filter(|c| !c.text.is_empty()) {
        Some(c) => format!("{}\n{user_text}", c.text),
        None => user_text.to_string(),
    };
    messages.push(Message::user(content));
    Request {
        model: model.into(),
        system: Some(SYSTEM.into()),
        messages,
        max_tokens,
        temperature: None,
        stream,
    }
}

/// Send one chat turn.
pub fn send(
    backend: &dyn Backend,
    req: &Request,
    cancel: &Cancel,
    on_delta: &mut dyn FnMut(&str),
) -> Result<Response> {
    backend.send(req, cancel, on_delta)
}
