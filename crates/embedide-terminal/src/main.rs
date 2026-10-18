//! Independently buildable terminal frontend; the main CLI delegates to this library.
use std::{
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
};

fn main() -> ExitCode {
    let mut path = None;
    let mut batch = false;
    let mut line = false;
    let mut positional = false;
    for argument in std::env::args_os().skip(1) {
        if !positional && argument == "--" {
            positional = true;
        } else if !positional && argument == "--batch" {
            batch = true;
        } else if !positional && argument == "--line" {
            line = true;
        } else if !positional && (argument == "--help" || argument == "-h") {
            println!("Usage: embedide-terminal [--batch | --line] [--] FILE\n\nInteractive mode uses the full-screen TUI when compiled with the tui feature.\n--line preserves cooked key-line input; --batch reads the same protocol without prompts.\nEOF never saves automatically. Batch mode stops on the first error.");
            return ExitCode::SUCCESS;
        } else if path.is_some() || (!positional && argument.to_string_lossy().starts_with('-')) {
            eprintln!("Usage: embedide-terminal [--batch | --line] [--] FILE");
            return ExitCode::from(2);
        } else {
            path = Some(PathBuf::from(argument));
        }
    }
    let Some(path) = path else {
        eprintln!("Usage: embedide-terminal [--batch | --line] [--] FILE");
        return ExitCode::from(2);
    };
    if batch && line {
        eprintln!("--batch and --line are mutually exclusive");
        return ExitCode::from(2);
    }
    // OsStr/PathBuf retain non-Unicode paths; no shell or shell quoting is used.
    let result = embedide_terminal::open(&path)
        .map_err(embedide_terminal::FrontendError::from)
        .and_then(|editor| {
            if line {
                embedide_terminal::run_line_stdio(editor, false)
            } else {
                embedide_terminal::run_stdio(editor, batch)
            }
        });
    match result {
        Ok(report) => {
            let output = writeln!(
                io::stdout().lock(),
                "closed={} modified={} bytes={} cursor={}:{}",
                report.closed,
                report.modified,
                report.bytes,
                report.position.line + 1,
                report.position.scalar_column + 1
            );
            match output {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("embedide-terminal: {error}");
                    ExitCode::from(1)
                }
            }
        }
        Err(error) => {
            eprintln!("embedide-terminal: {error}");
            ExitCode::from(1)
        }
    }
}
