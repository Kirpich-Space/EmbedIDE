//! Fast, bounded background syntax/compiler diagnostics for unsaved editor buffers.
//!
//! Runs read-only syntax checks (`-fsyntax-only`, `Rscript parse()`, `python3 ast.parse`,
//! `bash -n`, `zig ast-check`, standalone `rustc --emit=metadata`) against a temporary copy
//! of the buffer so errors appear dynamically while editing — without compiling artifacts
//! into the user's project or requiring a manual build.

use embedide_build::Selection;
use embedide_core::diagnostics::{self, Diagnostic};
use embedide_toolchains::Language;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

/// Maximum buffer size eligible for live syntax checking (256 KiB).
pub(crate) const MAX_BUFFER_BYTES: usize = 256 * 1024;
/// Maximum output read from a syntax check process (256 KiB).
const MAX_OUTPUT_BYTES: usize = 256 * 1024;
/// Maximum number of diagnostics kept per file.
const MAX_DIAGNOSTICS: usize = 100;
/// Hard timeout per background syntax check.
const CHECK_TIMEOUT: Duration = Duration::from_secs(3);

static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Result emitted by a finished background check.
#[derive(Debug)]
pub(crate) struct LiveResult {
    /// Generation counter at the time the check was scheduled.
    pub generation: u64,
    /// Canonical path of the checked source file.
    pub path: PathBuf,
    /// Parsed diagnostics mapped back to `path`.
    pub items: Vec<Diagnostic>,
}

/// Request snapshot handed to the worker thread.
pub(crate) struct CheckJob {
    pub generation: u64,
    pub path: PathBuf,
    pub root: PathBuf,
    pub language: String,
    pub contents: String,
    pub compilers: Vec<Selection>,
}

#[derive(Debug)]
struct Pending {
    due: Instant,
    path: PathBuf,
    language: String,
}

/// Debounced single-slot coordinator for live background syntax checks.
#[derive(Default)]
pub(crate) struct Runner {
    generation: u64,
    pending: Option<Pending>,
    rx: Option<mpsc::Receiver<LiveResult>>,
}

impl Runner {
    /// Whether a check is waiting on its debounce timer or running in the background.
    pub(crate) fn busy(&self) -> bool {
        self.pending.is_some() || self.rx.is_some()
    }

    /// Schedule a check for `path` after `delay`. Replaces any earlier pending check.
    pub(crate) fn schedule(&mut self, path: PathBuf, language: String, delay: Duration) {
        self.generation = self.generation.wrapping_add(1);
        self.pending = Some(Pending {
            due: Instant::now() + delay,
            path,
            language,
        });
    }

    /// Cancel any queued check and ignore any in-flight result.
    pub(crate) fn cancel(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.pending = None;
        self.rx = None;
    }

    /// Take the due pending check if its debounce window has elapsed.
    pub(crate) fn take_due(&mut self) -> Option<(u64, PathBuf, String)> {
        let ready = self
            .pending
            .as_ref()
            .is_some_and(|p| Instant::now() >= p.due);
        if !ready {
            return None;
        }
        let pending = self.pending.take()?;
        Some((self.generation, pending.path, pending.language))
    }

    /// Spawn a background worker for `job`, superseding any previous worker.
    pub(crate) fn spawn(&mut self, job: CheckJob) {
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let _ = thread::Builder::new()
            .name("embedide-livecheck".into())
            .spawn(move || {
                if let Some(items) = run_check(&job) {
                    let _ = tx.send(LiveResult {
                        generation: job.generation,
                        path: job.path,
                        items,
                    });
                }
            });
    }

    /// Receive a finished check result if available and still current.
    pub(crate) fn poll_result(&mut self) -> Option<LiveResult> {
        let rx = self.rx.as_ref()?;
        match rx.try_recv() {
            Ok(result) => {
                self.rx = None;
                (result.generation == self.generation).then_some(result)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.rx = None;
                None
            }
        }
    }
}

struct TempDirGuard(PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Build the command `(program, args)` for checking `tmp_file` without creating project artifacts.
fn check_command(
    job: &CheckJob,
    tmp_dir: &Path,
    tmp_file: &Path,
) -> Option<(PathBuf, Vec<String>)> {
    let ext = job
        .path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let file_dir = job.path.parent().unwrap_or(&job.root);
    let find_selected = |lang: Language| -> Option<&Selection> {
        job.compilers.iter().find(|s| s.language == lang)
    };

    match job.language.as_str() {
        "fortran" => {
            let (program, mut args) = match find_selected(Language::Fortran) {
                Some(sel) => (sel.program.clone(), sel.prefix_args.clone()),
                None => (
                    find_in_path(&["gfortran", "flang-new", "flang", "ifx"])?,
                    Vec::new(),
                ),
            };
            let name = program
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            args.push("-fsyntax-only".into());
            if name.contains("gfortran") {
                args.push("-Wall".into());
                // Keep any temporary `.mod` files strictly inside `tmp_dir`.
                args.push("-J".into());
                args.push(tmp_dir.display().to_string());
            }
            args.push("-I".into());
            args.push(file_dir.display().to_string());
            if file_dir != job.root {
                args.push("-I".into());
                args.push(job.root.display().to_string());
            }
            args.push(tmp_file.display().to_string());
            Some((program, args))
        }
        "c" if ext == "c" => {
            let (program, mut args) = match find_selected(Language::C) {
                Some(sel) => (sel.program.clone(), sel.prefix_args.clone()),
                None => (find_in_path(&["gcc", "clang", "cc"])?, Vec::new()),
            };
            args.push("-fsyntax-only".into());
            args.push("-Wall".into());
            args.push("-I".into());
            args.push(file_dir.display().to_string());
            if file_dir != job.root {
                args.push("-I".into());
                args.push(job.root.display().to_string());
            }
            args.push(tmp_file.display().to_string());
            Some((program, args))
        }
        "cpp" if matches!(ext.as_str(), "cpp" | "cc" | "cxx") => {
            let (program, mut args) = match find_selected(Language::Cpp) {
                Some(sel) => (sel.program.clone(), sel.prefix_args.clone()),
                None => (find_in_path(&["g++", "clang++", "c++"])?, Vec::new()),
            };
            args.push("-fsyntax-only".into());
            args.push("-Wall".into());
            args.push("-I".into());
            args.push(file_dir.display().to_string());
            if file_dir != job.root {
                args.push("-I".into());
                args.push(job.root.display().to_string());
            }
            args.push(tmp_file.display().to_string());
            Some((program, args))
        }
        "r" if matches!(ext.as_str(), "r") => {
            let (program, mut args) = match find_selected(Language::R) {
                Some(sel) => (sel.program.clone(), sel.prefix_args.clone()),
                None => (find_in_path(&["Rscript"])?, Vec::new()),
            };
            args.extend([
                "--vanilla".into(),
                "-e".into(),
                "invisible(parse(file=commandArgs(trailingOnly=TRUE)[1]))".into(),
                "--args".into(),
                tmp_file.display().to_string(),
            ]);
            Some((program, args))
        }
        "python" if ext == "py" => {
            let program = find_in_path(&[
                "python3",
                "python3.13",
                "python3.12",
                "python3.11",
                "python3.10",
                "python",
            ])?;
            let script = "import ast, sys\n\
try:\n\
 ast.parse(open(sys.argv[1], encoding='utf-8').read(), filename=sys.argv[1])\n\
except SyntaxError as e:\n\
 print(f'{sys.argv[1]}:{e.lineno or 1}:{e.offset or 1}: error: {e.msg}')\n\
 sys.exit(1)\n";
            Some((
                program,
                vec!["-c".into(), script.into(), tmp_file.display().to_string()],
            ))
        }
        "sh" | "bash" | "shell" if matches!(ext.as_str(), "sh" | "bash") => {
            let candidates: &[&str] = if ext == "bash" {
                &["bash"]
            } else {
                &["bash", "sh"]
            };
            let program = find_in_path(candidates)?;
            Some((program, vec!["-n".into(), tmp_file.display().to_string()]))
        }
        "zig" if ext == "zig" => {
            let program = find_selected(Language::Zig)
                .map(|s| s.program.clone())
                .or_else(|| find_in_path(&["zig"]))?;
            Some((
                program,
                vec!["ast-check".into(), tmp_file.display().to_string()],
            ))
        }
        "rust" if ext == "rs" && !job.root.join("Cargo.toml").exists() => {
            let program = find_in_path(&["rustc"])?;
            let out = tmp_dir.join("check.rmeta");
            Some((
                program,
                vec![
                    "--crate-type".into(),
                    "lib".into(),
                    "--emit=metadata".into(),
                    "-o".into(),
                    out.display().to_string(),
                    tmp_file.display().to_string(),
                ],
            ))
        }
        _ => None,
    }
}

fn find_in_path(names: &[&str]) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for name in names {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Execute the syntax check in an isolated temporary directory and map diagnostics back to `job.path`.
fn run_check(job: &CheckJob) -> Option<Vec<Diagnostic>> {
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp_dir =
        std::env::temp_dir().join(format!("embedide-livecheck-{}-{seq}", std::process::id()));
    fs::create_dir_all(&tmp_dir).ok()?;
    let _guard = TempDirGuard(tmp_dir.clone());

    let file_name = job
        .path
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty() && !n.contains('/') && !n.contains('\\'))
        .unwrap_or("buffer.txt");
    let tmp_file = tmp_dir.join(file_name);
    fs::write(&tmp_file, job.contents.as_bytes()).ok()?;

    let (program, args) = check_command(job, &tmp_dir, &tmp_file)?;

    // Redirect stdout and stderr into files inside `tmp_dir` so large outputs never block on OS pipes.
    let out_path = tmp_dir.join(".livecheck.out");
    let err_path = tmp_dir.join(".livecheck.err");
    let out_file = fs::File::create(&out_path).ok()?;
    let err_file = fs::File::create(&err_path).ok()?;

    let mut child = Command::new(&program)
        .args(&args)
        .current_dir(&job.root)
        .stdin(Stdio::null())
        .stdout(Stdio::from(out_file))
        .stderr(Stdio::from(err_file))
        .spawn()
        .ok()?;

    let deadline = Instant::now() + CHECK_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                thread::sleep(Duration::from_millis(15));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }

    let mut combined = String::new();
    for p in [&err_path, &out_path] {
        if let Ok(file) = fs::File::open(p) {
            let mut buf = Vec::new();
            let _ = file.take(MAX_OUTPUT_BYTES as u64).read_to_end(&mut buf);
            if !combined.is_empty() && !combined.ends_with('\n') {
                combined.push('\n');
            }
            combined.push_str(&String::from_utf8_lossy(&buf));
        }
    }

    Some(parse_live_output(
        &combined,
        &job.language,
        &tmp_file,
        file_name,
        &job.path,
    ))
}

/// Parse compiler/interpreter output, keeping only diagnostics that belong to `tmp_file` and
/// remapping their file path to `real_path`.
fn parse_live_output(
    output: &str,
    language: &str,
    tmp_file: &Path,
    file_name: &str,
    real_path: &Path,
) -> Vec<Diagnostic> {
    let mut parser = diagnostics::Parser::default();
    let mut items = Vec::new();
    let tmp_str = tmp_file.to_string_lossy();
    let is_shell = matches!(language, "sh" | "bash" | "shell");

    for raw in output.lines() {
        let diag = parser.feed(raw).or_else(|| {
            is_shell
                .then(|| parse_shell_syntax_error(raw, &tmp_str))
                .flatten()
        });
        let Some(mut d) = diag else {
            continue;
        };
        if !matches_temp_file(d.file.as_deref(), tmp_file, file_name) {
            continue;
        }
        d.file = Some(real_path.to_path_buf());
        d.source = "live".into();
        let duplicate = items
            .iter()
            .any(|e: &Diagnostic| e.line == d.line && e.message == d.message);
        if !duplicate {
            items.push(d);
            if items.len() >= MAX_DIAGNOSTICS {
                break;
            }
        }
    }
    items
}

fn matches_temp_file(reported: Option<&Path>, tmp_file: &Path, file_name: &str) -> bool {
    let Some(path) = reported else {
        return false;
    };
    path == tmp_file
        || path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n == file_name)
}

/// Parse `bash -n` / `sh -n` errors: `<path>: line N: <message>` or `<path>: N: <message>`.
fn parse_shell_syntax_error(line: &str, tmp_str: &str) -> Option<Diagnostic> {
    let rest = line.strip_prefix(tmp_str)?.strip_prefix(':')?;
    let rest = rest.trim_start();
    let (row_str, msg) = if let Some(after) = rest.strip_prefix("line ") {
        after.split_once(':')?
    } else {
        rest.split_once(':')?
    };
    let row = row_str.trim().parse::<usize>().ok()?;
    let message = msg.trim();
    if message.is_empty() {
        return None;
    }
    Some(Diagnostic {
        severity: "error".into(),
        message: message.into(),
        file: Some(PathBuf::from(tmp_str)),
        line: row.max(1),
        column: 1,
        code: None,
        source: "live".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remaps_temp_file_paths_and_filters_unrelated_headers() {
        let tmp = PathBuf::from("/tmp/embedide-livecheck-1-0/main.f90");
        let real = PathBuf::from("/work/proj/main.f90");
        let out = "\
/tmp/embedide-livecheck-1-0/main.f90:6:22:

    6 |     real :: result = x * y * c
      |                      1
Error: Parameter 'x' at (1) has not been declared
/usr/include/other.h:9:1: error: ignored header error
main.f90:10:19:

   10 |     print *, result
      |                   1
Error: Symbol 'result' at (1) has no IMPLICIT type
";
        let diags = parse_live_output(out, "fortran", &tmp, "main.f90", &real);
        assert_eq!(diags.len(), 2, "{diags:?}");
        assert_eq!(diags[0].file.as_deref(), Some(real.as_path()));
        assert_eq!(diags[0].line, 6);
        assert_eq!(diags[1].file.as_deref(), Some(real.as_path()));
        assert_eq!(diags[1].line, 10);
    }

    #[test]
    fn parses_shell_syntax_errors() {
        let tmp = PathBuf::from("/tmp/embedide-livecheck-1-0/run.sh");
        let real = PathBuf::from("/work/run.sh");
        let out =
            "/tmp/embedide-livecheck-1-0/run.sh: line 3: syntax error near unexpected token `fi'\n";
        let diags = parse_live_output(out, "sh", &tmp, "run.sh", &real);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].line, 3);
        assert!(diags[0].message.contains("unexpected token"));
    }

    #[test]
    fn live_check_finds_gfortran_error_without_building_artifacts() {
        if find_in_path(&["gfortran"]).is_none() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("embed-live-f90-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("main.f90");
        fs::write(&file, "program main\nend program main\n").unwrap();

        let job = CheckJob {
            generation: 1,
            path: file.clone(),
            root: dir.clone(),
            language: "fortran".into(),
            contents: "program main\n  implicit none\n  print *, missing_var\nend program main\n"
                .into(),
            compilers: Vec::new(),
        };
        let diags = run_check(&job).expect("gfortran check ran");
        assert!(
            diags.iter().any(|d| d.line == 3 && d.severity == "error"),
            "expected error on line 3: {diags:?}"
        );
        // Must never leave `.o`, `.mod`, or binaries inside the project directory.
        let entries: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name())
            .collect();
        assert_eq!(
            entries.len(),
            1,
            "unexpected files left behind: {entries:?}"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
