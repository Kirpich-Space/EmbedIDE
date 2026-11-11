//! Editor-independent code completion (inline suggestions, whole function
//! bodies). [`complete`] answers one request; [`Engine`] is what an editor
//! uses while the user types: it debounces, keeps at most one request in
//! flight, cancels superseded requests and drops their results.
use crate::{Backend, Cancel, Error, Message, Request, Result, Usage};
use serde::Serialize;
use std::{
    collections::VecDeque,
    sync::{mpsc, Arc, Condvar, Mutex},
    thread,
    time::{Duration, Instant},
};

/// Characters of code before / after the cursor sent to the model.
pub const PREFIX_CHARS: usize = 6000;
/// See [`PREFIX_CHARS`].
pub const SUFFIX_CHARS: usize = 2000;
/// Marker placed at the insertion point.
pub const CURSOR: &str = "<|cursor|>";

/// Instructions for completion.
pub const SYSTEM: &str = "You are a code completion engine inside an IDE. You receive a source file with the \
marker <|cursor|> at the insertion point. Reply with ONLY the text to insert at the marker: no explanation, no \
markdown fences, no repetition of code before or after the marker. Complete the current statement, block or \
whole function body as appropriate. If nothing sensible fits, reply with nothing.";

/// One completion request.
#[derive(Debug, Clone, Default, Serialize)]
pub struct CompletionRequest {
    /// Source language id (c, cpp, rust, zig, asm, fortran, …).
    pub language: String,
    /// File name shown to the model.
    pub path: Option<String>,
    /// Whole buffer.
    pub text: String,
    /// 1-based line of the cursor.
    pub line: usize,
    /// 1-based column (Unicode scalar values).
    pub column: usize,
    /// Related files: (name, content), already filtered by the caller.
    pub related: Vec<(String, String)>,
    /// Output token cap.
    pub max_tokens: u32,
    /// Declarations elsewhere in the file (see [`symbols`]).
    pub symbols: Vec<String>,
    /// One line about the project (name, target, language).
    pub project: Option<String>,
    /// Characters before the cursor sent (0: [`PREFIX_CHARS`]).
    pub prefix_chars: usize,
    /// Characters after the cursor sent (0: [`SUFFIX_CHARS`]).
    pub suffix_chars: usize,
}

/// Result.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Completion {
    /// Text to insert at the cursor.
    pub insertion: String,
    /// Usage, if reported.
    pub usage: Usage,
}

/// Byte offset of a 1-based line/column; errors when outside the buffer.
pub fn offset(text: &str, line: usize, column: usize) -> Result<usize> {
    if line == 0 || column == 0 {
        return Err(Error::Config("line and column are 1-based".into()));
    }
    let mut start = 0;
    for (i, l) in text.split_inclusive('\n').enumerate() {
        if i + 1 == line {
            let content = l.strip_suffix('\n').unwrap_or(l);
            let content = content.strip_suffix('\r').unwrap_or(content);
            let chars = content.chars().count();
            if column > chars + 1 {
                return Err(Error::Config(format!(
                    "column {column} is past the end of line {line} ({chars} characters)"
                )));
            }
            return Ok(start
                + content
                    .char_indices()
                    .nth(column - 1)
                    .map(|(b, _)| b)
                    .unwrap_or(content.len()));
        }
        start += l.len();
    }
    if line == text.split_inclusive('\n').count() + 1 && column == 1 {
        return Ok(text.len());
    }
    Err(Error::Config(format!(
        "line {line} is past the end of the file"
    )))
}

fn tail(s: &str, n: usize) -> &str {
    let count = s.chars().count();
    if count <= n {
        return s;
    }
    let skip = s.char_indices().nth(count - n).map(|(b, _)| b).unwrap_or(0);
    &s[skip..]
}

fn head(s: &str, n: usize) -> &str {
    s.char_indices().nth(n).map(|(b, _)| &s[..b]).unwrap_or(s)
}

/// Declarations of the file outside the sent window (function, type and
/// macro heads, assembly labels): cheap context about nearby symbols.
/// `window` is the byte range already sent.
pub fn symbols(
    text: &str,
    language: &str,
    window: std::ops::Range<usize>,
    max: usize,
) -> Vec<String> {
    let mut out = Vec::new();
    let mut pos = 0;
    for line in text.split_inclusive('\n') {
        let start = pos;
        pos += line.len();
        if start >= window.start && start < window.end {
            continue;
        }
        let l = line.trim_end();
        if l.is_empty() || l.starts_with([' ', '\t']) || l.len() > 160 {
            continue;
        }
        let words = [
            "fn ", "pub ", "struct ", "enum ", "union ", "typedef ", "impl ", "trait ", "type ",
            "const ", "static ", "extern ", "#define ", "mod ", "class ", "def ", "export ",
        ];
        let decl = match language {
            "asm" => l.ends_with(':') && !l.starts_with(['.', ';', '#']),
            _ => {
                words.iter().any(|w| l.starts_with(w))
                    || (l.contains('(')
                        && !l.starts_with(['#', '/', '*', '}'])
                        && !l.ends_with(';')
                        && l.chars()
                            .next()
                            .is_some_and(|c| c.is_alphabetic() || c == '_'))
                    || (l.contains('(') && l.ends_with(");") && !l.starts_with(['#', '/']))
            }
        };
        if decl {
            out.push(l.trim_end_matches('{').trim_end().to_string());
            if out.len() >= max {
                break;
            }
        }
    }
    out
}

/// Re-indent the continuation lines of a suggestion in the buffer's style
/// (tabs, or `width` spaces).
pub fn reindent(text: &str, tabs: bool, width: usize) -> String {
    let width = width.max(1);
    let mut out = String::with_capacity(text.len());
    for (i, line) in text.split_inclusive('\n').enumerate() {
        if i == 0 {
            out.push_str(line);
            continue;
        }
        let lead: String = line
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        let cells: usize = lead
            .chars()
            .map(|c| if c == '\t' { width } else { 1 })
            .sum();
        if tabs {
            out.push_str(&"\t".repeat(cells / width));
            out.push_str(&" ".repeat(cells % width));
        } else {
            out.push_str(&" ".repeat(cells));
        }
        out.push_str(&line[lead.len()..]);
    }
    out
}

/// At most `max` lines of a suggestion.
pub fn limit_lines(text: &str, max: usize) -> String {
    match text.split_inclusive('\n').nth(max.max(1)) {
        Some(_) => text
            .split_inclusive('\n')
            .take(max.max(1))
            .collect::<String>()
            .trim_end_matches('\n')
            .to_string(),
        None => text.to_string(),
    }
}

/// Build the provider request (no network).
pub fn request(req: &CompletionRequest, model: &str) -> Result<Request> {
    let at = offset(&req.text, req.line, req.column)?;
    let (before, after) = req.text.split_at(at);
    let prefix = if req.prefix_chars == 0 {
        PREFIX_CHARS
    } else {
        req.prefix_chars
    };
    let suffix = if req.suffix_chars == 0 {
        SUFFIX_CHARS
    } else {
        req.suffix_chars
    };
    let mut prompt = String::new();
    if let Some(p) = &req.project {
        prompt.push_str(&format!("Project: {p}\n"));
    }
    if !req.symbols.is_empty() {
        prompt.push_str("Declarations elsewhere in this file:\n");
        for s in &req.symbols {
            prompt.push_str(s);
            prompt.push('\n');
        }
        prompt.push('\n');
    }
    for (name, content) in &req.related {
        prompt.push_str(&format!(
            "Related file {name}:\n```\n{}\n```\n\n",
            head(content, 4000)
        ));
    }
    prompt.push_str(&format!(
        "Language: {}\nFile: {}\n```\n{}{CURSOR}{}\n```\n",
        req.language,
        req.path.as_deref().unwrap_or("(unsaved buffer)"),
        tail(before, prefix),
        head(after, suffix)
    ));
    Ok(Request {
        model: model.into(),
        system: Some(SYSTEM.into()),
        messages: vec![Message::user(prompt)],
        max_tokens: req.max_tokens,
        temperature: Some(0.0),
        stream: false,
    })
}

/// Turn a model reply into an insertion: strip fences, echoed context and markers.
pub fn clean(reply: &str, before: &str, after: &str) -> String {
    let mut s = reply.trim_end_matches(['\n', ' ']).to_string();
    let trimmed = s.trim_start();
    if trimmed.starts_with("```") {
        let body: Vec<&str> = trimmed.lines().skip(1).collect();
        let body = if body
            .last()
            .is_some_and(|l| l.trim_start().starts_with("```"))
        {
            &body[..body.len() - 1]
        } else {
            &body[..]
        };
        s = body.join("\n");
    }
    s = s.replace(CURSOR, "");
    // A model that repeats the current line before the cursor (or its indentation)
    if let Some(cur) = before.rsplit('\n').next().filter(|l| !l.is_empty()) {
        if let Some(rest) = s.strip_prefix(cur) {
            s = rest.to_string();
        }
    }
    // …or the next non-empty line after it (typically a closing brace)
    if let Some(next) = after.lines().map(str::trim).find(|l| !l.is_empty()) {
        if let Some(rest) = s.strip_suffix(next) {
            s = rest.trim_end().to_string();
        }
    }
    s
}

/// Answer one completion request.
pub fn complete(
    backend: &dyn Backend,
    req: &CompletionRequest,
    model: &str,
    cancel: &Cancel,
) -> Result<Completion> {
    let r = request(req, model)?;
    let resp = backend.send(&r, cancel, &mut |_| {})?;
    let at = offset(&req.text, req.line, req.column)?;
    let (before, after) = req.text.split_at(at);
    Ok(Completion {
        insertion: clean(&resp.text, before, after),
        usage: resp.usage,
    })
}

struct State {
    /// (id, request, queued at, debounce for this request)
    pending: Option<(u64, CompletionRequest, Instant, Duration)>,
    latest: u64,
    in_flight: Option<Cancel>,
    shutdown: bool,
    cache: VecDeque<(u64, String)>,
}

/// Engine settings.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// Wait after the last keystroke.
    pub debounce: Duration,
    /// Longest one request may run.
    pub timeout: Duration,
    /// Suggestions remembered by context (0: no cache).
    pub cache_entries: usize,
}

/// Key for the suggestion cache: the text around the cursor.
fn cache_key(req: &CompletionRequest) -> Option<u64> {
    use std::hash::{Hash, Hasher};
    let at = offset(&req.text, req.line, req.column).ok()?;
    let (before, after) = req.text.split_at(at);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    req.path.hash(&mut h);
    req.language.hash(&mut h);
    tail(before, 2000).hash(&mut h);
    head(after, 500).hash(&mut h);
    Some(h.finish())
}

/// Debounced, superseding completion service for editors.
pub struct Engine {
    shared: Arc<(Mutex<State>, Condvar)>,
    results: mpsc::Receiver<(u64, Result<Completion>)>,
    tx: mpsc::Sender<(u64, Result<Completion>)>,
    debounce: Duration,
    worker: Option<thread::JoinHandle<()>>,
}

impl Engine {
    /// Start a worker that asks `backend`/`model`, waiting `debounce` after the last keystroke.
    pub fn new(backend: Arc<dyn Backend>, model: String, debounce: Duration) -> Self {
        Self::with_options(
            backend,
            model,
            Options {
                debounce,
                timeout: Duration::from_secs(60),
                cache_entries: 0,
            },
        )
    }

    /// Start a worker with explicit [`Options`].
    pub fn with_options(backend: Arc<dyn Backend>, model: String, options: Options) -> Self {
        let debounce = options.debounce;
        let shared = Arc::new((
            Mutex::new(State {
                pending: None,
                latest: 0,
                in_flight: None,
                shutdown: false,
                cache: VecDeque::new(),
            }),
            Condvar::new(),
        ));
        let (tx, results) = mpsc::channel();
        let worker_tx = tx.clone();
        let s = shared.clone();
        let worker = thread::spawn(move || loop {
            let (lock, cv) = &*s;
            let mut st = lock.lock().expect("lock");
            while st.pending.is_none() && !st.shutdown {
                st = cv.wait(st).expect("lock");
            }
            if st.shutdown {
                return;
            }
            // Debounce: wait until no newer request arrived for `debounce`
            loop {
                let (_, _, at, debounce) = st.pending.as_ref().expect("pending");
                let wait = debounce.saturating_sub(at.elapsed());
                if wait.is_zero() || st.shutdown {
                    break;
                }
                st = cv.wait_timeout(st, wait).expect("lock").0;
            }
            if st.shutdown {
                return;
            }
            let (id, req, _, _) = st.pending.take().expect("pending");
            let cancel = Cancel::default();
            st.in_flight = Some(cancel.clone());
            drop(st);
            // A watchdog cancels a request that takes longer than the timeout.
            let (done_tx, done_rx) = mpsc::channel::<()>();
            let timed_out = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let watchdog = {
                let cancel = cancel.clone();
                let timed_out = timed_out.clone();
                let timeout = options.timeout;
                thread::spawn(move || {
                    if let Err(mpsc::RecvTimeoutError::Timeout) = done_rx.recv_timeout(timeout) {
                        timed_out.store(true, std::sync::atomic::Ordering::Release);
                        cancel.cancel();
                    }
                })
            };
            let mut result = complete(backend.as_ref(), &req, &model, &cancel);
            let _ = done_tx.send(());
            let _ = watchdog.join();
            let expired = timed_out.load(std::sync::atomic::Ordering::Acquire);
            if expired {
                result = Err(Error::Network(format!(
                    "no suggestion within {} s (ai.completion.timeout_secs)",
                    options.timeout.as_secs()
                )));
            }
            let mut st = lock.lock().expect("lock");
            st.in_flight = None;
            if let (Ok(c), Some(key)) = (&result, cache_key(&req)) {
                if options.cache_entries > 0 && !c.insertion.is_empty() {
                    st.cache.retain(|(k, _)| *k != key);
                    st.cache.push_back((key, c.insertion.clone()));
                    while st.cache.len() > options.cache_entries {
                        st.cache.pop_front();
                    }
                }
            }
            // Superseded or cancelled results are dropped, never shown
            if id == st.latest && (!cancel.is_cancelled() || expired) {
                let _ = worker_tx.send((id, result));
            }
        });
        Self {
            shared,
            results,
            tx,
            debounce,
            worker: Some(worker),
        }
    }

    /// Queue a request without the debounce (an explicit request).
    pub fn submit_now(&self, req: CompletionRequest) -> u64 {
        self.queue(req, Duration::ZERO)
    }

    /// Whether a request is queued or running.
    pub fn busy(&self) -> bool {
        let (lock, _) = &*self.shared;
        lock.lock()
            .map(|st| st.pending.is_some() || st.in_flight.is_some())
            .unwrap_or(false)
    }

    /// Queue a request; any older pending or in-flight request is superseded.
    pub fn submit(&self, req: CompletionRequest) -> u64 {
        self.queue(req, self.debounce)
    }

    fn queue(&self, req: CompletionRequest, debounce: Duration) -> u64 {
        let (lock, cv) = &*self.shared;
        let mut st = lock.lock().expect("lock");
        st.latest += 1;
        let id = st.latest;
        if let Some(c) = &st.in_flight {
            c.cancel();
        }
        // The same context was answered before: no request at all.
        let cached = cache_key(&req).and_then(|key| {
            st.cache
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, text)| text.clone())
        });
        if let Some(insertion) = cached {
            st.pending = None;
            let _ = self.tx.send((
                id,
                Ok(Completion {
                    insertion,
                    usage: Usage::default(),
                }),
            ));
            return id;
        }
        st.pending = Some((id, req, Instant::now(), debounce));
        cv.notify_all();
        id
    }

    /// Cancel everything (e.g. the user pressed Escape or moved away).
    pub fn cancel(&self) {
        let (lock, _) = &*self.shared;
        let mut st = lock.lock().expect("lock");
        st.latest += 1;
        st.pending = None;
        if let Some(c) = &st.in_flight {
            c.cancel();
        }
    }

    /// Next result, if any (non-blocking).
    pub fn poll(&self) -> Option<(u64, Result<Completion>)> {
        self.results.try_recv().ok()
    }

    /// Wait up to `timeout` for a result.
    pub fn wait(&self, timeout: Duration) -> Option<(u64, Result<Completion>)> {
        self.results.recv_timeout(timeout).ok()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        {
            let (lock, cv) = &*self.shared;
            let mut st = lock.lock().expect("lock");
            st.shutdown = true;
            if let Some(c) = &st.in_flight {
                c.cancel();
            }
            cv.notify_all();
        }
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ModelInfo, Response};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fake {
        calls: AtomicUsize,
        delay: Duration,
        reply: String,
    }
    impl Backend for Fake {
        fn label(&self) -> String {
            "fake".into()
        }
        fn send(
            &self,
            req: &Request,
            cancel: &Cancel,
            _: &mut dyn FnMut(&str),
        ) -> Result<Response> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            assert!(req.messages[0].content.contains(CURSOR));
            let start = Instant::now();
            while start.elapsed() < self.delay {
                if cancel.is_cancelled() {
                    return Err(Error::Cancelled);
                }
                thread::sleep(Duration::from_millis(5));
            }
            Ok(Response {
                text: self.reply.clone(),
                usage: Usage::default(),
                stop_reason: None,
                model: None,
            })
        }
        fn models(&self) -> Result<Vec<ModelInfo>> {
            Ok(Vec::new())
        }
    }

    fn req(text: &str, line: usize, column: usize) -> CompletionRequest {
        CompletionRequest {
            language: "c".into(),
            path: Some("main.c".into()),
            text: text.into(),
            line,
            column,
            related: Vec::new(),
            max_tokens: 64,
            ..Default::default()
        }
    }

    #[test]
    fn cache_answers_the_same_context_without_a_request_and_timeouts_report() {
        let fake = Arc::new(Fake {
            calls: AtomicUsize::new(0),
            delay: Duration::from_millis(5),
            reply: "1;".into(),
        });
        let e = Engine::with_options(
            fake.clone(),
            "m".into(),
            Options {
                debounce: Duration::from_millis(1),
                timeout: Duration::from_secs(5),
                cache_entries: 8,
            },
        );
        e.submit_now(req("int x = ", 1, 9));
        assert_eq!(
            e.wait(Duration::from_secs(2)).unwrap().1.unwrap().insertion,
            "1;"
        );
        let id = e.submit(req("int x = ", 1, 9));
        let (got, r) = e.wait(Duration::from_secs(2)).unwrap();
        assert_eq!((got, r.unwrap().insertion.as_str()), (id, "1;"));
        assert_eq!(
            fake.calls.load(Ordering::SeqCst),
            1,
            "second answer came from the cache"
        );
        let slow = Arc::new(Fake {
            calls: AtomicUsize::new(0),
            delay: Duration::from_secs(5),
            reply: "x".into(),
        });
        let e = Engine::with_options(
            slow,
            "m".into(),
            Options {
                debounce: Duration::ZERO,
                timeout: Duration::from_millis(100),
                cache_entries: 0,
            },
        );
        e.submit(req("a", 1, 2));
        let (_, r) = e
            .wait(Duration::from_secs(3))
            .expect("a timeout is reported");
        assert!(r.unwrap_err().to_string().contains("timeout_secs"));
    }

    #[test]
    fn symbols_reindent_and_line_limits() {
        let text = "#include <stdio.h>\nstatic int count;\nint add(int a, int b) {\n    return a + b;\n}\nvoid run(void);\n";
        let s = symbols(text, "c", 0..0, 10);
        assert_eq!(
            s,
            [
                "static int count;",
                "int add(int a, int b)",
                "void run(void);"
            ]
        );
        let at = text.find("int add").unwrap();
        assert!(!symbols(text, "c", at..text.len(), 10)
            .iter()
            .any(|l| l.contains("add")));
        let asm = "_start:\n    mov rax, 60\n.Lloop:\nloop:\n";
        assert_eq!(symbols(asm, "asm", 0..0, 10), ["_start:", "loop:"]);
        assert_eq!(reindent("{\n        x;\n    }", true, 4), "{\n\t\tx;\n\t}");
        assert_eq!(reindent("{\n\tx;\n}", false, 4), "{\n    x;\n}");
        assert_eq!(limit_lines("a\nb\nc\n", 2), "a\nb");
        assert_eq!(limit_lines("a\nb", 5), "a\nb");
        let r = request(
            &CompletionRequest {
                symbols: vec!["int add(int a, int b)".into()],
                project: Some("blink (C, cortex-m4)".into()),
                prefix_chars: 3,
                ..req("abcdefgh", 1, 9)
            },
            "m",
        )
        .unwrap();
        let p = &r.messages[0].content;
        assert!(p.contains("Project: blink"));
        assert!(p.contains("Declarations elsewhere in this file:\nint add"));
        assert!(p.contains("```\nfgh<|cursor|>"), "{p}");
    }

    #[test]
    fn offsets_are_unicode_aware() {
        let t = "int x;\n// привет\nend";
        assert_eq!(offset(t, 1, 1).unwrap(), 0);
        assert_eq!(t[offset(t, 2, 4).unwrap()..].chars().next(), Some('п'));
        assert_eq!(offset(t, 3, 4).unwrap(), t.len());
        assert!(offset(t, 2, 50).is_err());
        assert!(offset(t, 9, 1).is_err());
    }

    #[test]
    fn replies_are_cleaned() {
        assert_eq!(
            clean(
                "```c\nreturn a + b;\n```",
                "int add(int a, int b) {\n    ",
                "\n}"
            ),
            "return a + b;"
        );
        assert_eq!(
            clean("    return a + b;\n}", "    ", "\n}"),
            "return a + b;"
        );
        assert_eq!(clean("foo(<|cursor|>1)", "", ""), "foo(1)");
    }

    #[test]
    fn engine_debounces_and_keeps_only_the_latest() {
        let fake = Arc::new(Fake {
            calls: AtomicUsize::new(0),
            delay: Duration::from_millis(10),
            reply: "1;".into(),
        });
        let e = Engine::new(fake.clone(), "m".into(), Duration::from_millis(80));
        let mut last = 0;
        for _ in 0..5 {
            last = e.submit(req("int x = ", 1, 9));
            thread::sleep(Duration::from_millis(10));
        }
        let (id, r) = e.wait(Duration::from_secs(2)).expect("result");
        assert_eq!(id, last);
        assert_eq!(r.unwrap().insertion, "1;");
        assert_eq!(
            fake.calls.load(Ordering::SeqCst),
            1,
            "five keystrokes, one request"
        );
    }

    #[test]
    fn engine_cancels_in_flight_request_when_superseded() {
        let fake = Arc::new(Fake {
            calls: AtomicUsize::new(0),
            delay: Duration::from_millis(400),
            reply: "x".into(),
        });
        let e = Engine::new(fake.clone(), "m".into(), Duration::from_millis(1));
        e.submit(req("a", 1, 2));
        thread::sleep(Duration::from_millis(60));
        let second = e.submit(req("ab", 1, 3));
        let (id, _) = e.wait(Duration::from_secs(3)).expect("result");
        assert_eq!(id, second, "only the newest request is answered");
        assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
        assert!(e.poll().is_none());
    }
}
