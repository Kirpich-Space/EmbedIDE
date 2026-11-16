#!/usr/bin/env python3
"""Compile the actual editor sources using cached Unicode dependencies when registry access is absent.

This checks embedide-editor and optionally its terminal frontend, not the full
workspace or its CLI/core facade.
An ephemeral Cargo manifest references the original library, tests and benches;
it does not copy sources, modify workspace membership or change Cargo.lock.
Requires Python 3.11+ and the installed Rust/Cargo/rustfmt/Clippy tools.
"""

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
EDITOR = ROOT / "crates/embedide-editor"
TERMINAL = ROOT / "crates/embedide-terminal"
# The ephemeral manifests live in a temporary directory; tests that read repository files use this.
ENV = dict(os.environ, EMBEDIDE_REPO_ROOT=str(ROOT))


def manifest(terminal=False):
    workspace = tomllib.loads((ROOT / "Cargo.toml").read_text())
    package = tomllib.loads((EDITOR / "Cargo.toml").read_text())
    if package.get("dependencies") != {"unicode-segmentation": "1.12", "unicode-width": "0.2"}:
        raise ValueError("editor dependencies changed; update the offline checker")
    for section in ("dev-dependencies", "build-dependencies", "target"):
        if package.get(section):
            raise ValueError("editor has dependencies; use the normal workspace checks")
    lines = ['[package]', 'name = "embedide-editor"']
    for field in ("version", "edition", "rust-version", "license"):
        lines.append(f'{field} = {json.dumps(workspace["workspace"]["package"][field])}')
    lines += ['[dependencies]', 'unicode-segmentation = "1.12"', 'unicode-width = "0.2"', "[workspace]"]
    if terminal:
        lines.append('members = ["terminal"]')
    lines += ["[lib]", f'path = {json.dumps(str(EDITOR / "src/lib.rs"))}']
    lines += ["[lints.rust]"]
    for name, level in workspace["workspace"]["lints"]["rust"].items():
        if not isinstance(level, str):
            raise ValueError("extended lint configuration requires updating this checker")
        lines.append(f'{name} = {json.dumps(level)}')
    lines += ["[profile.release]"]
    for name, value in workspace["profile"]["release"].items():
        lines.append(f'{name} = {json.dumps(value)}')
    for folder, kind in (("tests", "test"), ("benches", "bench")):
        for source in sorted((EDITOR / folder).glob("*.rs")):
            lines += [f"[[{kind}]]", f'name = {json.dumps(source.stem)}',
                      f'path = {json.dumps(str(source))}']
            if kind == "bench":
                lines.append("harness = false")
    return "\n".join(lines) + "\n"


def terminal_manifest():
    workspace = tomllib.loads((ROOT / "Cargo.toml").read_text())
    package = tomllib.loads((TERMINAL / "Cargo.toml").read_text())
    deps = package.get("dependencies", {})
    # Everything except the editor is optional and only compiled with the `tui` feature; the line-only
    # build below leaves it out.
    optional = {"crossterm", "unicode-segmentation", "unicode-width", "embedide-core", "serde_json",
                "portable-pty", "vt100", "embedide-build", "embedide-workspace", "embedide-toolchains",
                "embedide-ai"}
    if deps.get("embedide-editor") != {"workspace": True} or set(deps) != optional | {"embedide-editor"}:
        raise ValueError("terminal dependencies changed; update the line-only offline checker")
    if not all(deps[name].get("optional") for name in optional):
        raise ValueError("TUI dependencies must remain optional for offline line builds")
    target = package.get("target", {})
    platform = target.get("cfg(unix)", {}).get("dependencies", {}) if set(target) == {"cfg(unix)"} else {}
    if set(platform) != {"signal-hook", "nix"} or not all(platform[name].get("optional") for name in platform):
        raise ValueError("terminal platform dependencies changed; update this checker")
    if set(package.get("features", {}).get("tui", [])) != {"dep:" + name for name in optional | {"signal-hook", "nix"}}:
        raise ValueError("TUI feature dependencies changed; update this checker")
    if any(package.get(section) for section in ("dev-dependencies", "build-dependencies")):
        raise ValueError("terminal has additional dependencies; update this checker")
    lines = ['[package]', 'name = "embedide-terminal"']
    for field in ("version", "edition", "rust-version", "license"):
        lines.append(f'{field} = {json.dumps(workspace["workspace"]["package"][field])}')
    lines += ['[lib]', f'path = {json.dumps(str(TERMINAL / "src/lib.rs"))}',
              '[[bin]]', 'name = "embedide-terminal"',
              f'path = {json.dumps(str(TERMINAL / "src/main.rs"))}',
              '[features]', 'default = []', 'tui = []',
              '[dependencies.embedide-editor]', 'path = ".."', '[lints.rust]']
    for name, level in workspace["workspace"]["lints"]["rust"].items():
        if not isinstance(level, str):
            raise ValueError("extended lint configuration requires updating this checker")
        lines.append(f'{name} = {json.dumps(level)}')
    for source in sorted((TERMINAL / "tests").glob("*.rs")):
        lines += ['[[test]]', f'name = {json.dumps(source.stem)}',
                  f'path = {json.dumps(str(source))}']
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bench", action="store_true", help="also run release benchmarks")
    parser.add_argument("--terminal", action="store_true", help="also validate the terminal frontend")
    parser.add_argument("--build-terminal", type=Path, metavar="OUTPUT",
                        help="validate and build the real terminal binary, copying it to OUTPUT")
    parser.add_argument("--release", action="store_true", help="export an optimized terminal binary (requires --build-terminal)")
    args = parser.parse_args()
    if args.release and args.build_terminal is None:
        parser.error("--release requires --build-terminal")
    terminal = args.terminal or args.build_terminal is not None
    print("Scope: editor" + (" + line terminal frontend (TUI disabled)" if terminal else "")
          + "; this does NOT validate the full workspace or main CLI.", flush=True)
    with tempfile.TemporaryDirectory(prefix="embedide-editor-check-") as temporary:
        path = Path(temporary) / "Cargo.toml"
        path.write_text(manifest(terminal), encoding="utf-8")
        if terminal:
            (path.parent / "terminal").mkdir()
            (path.parent / "terminal/Cargo.toml").write_text(terminal_manifest(), encoding="utf-8")

        def run(*arguments):
            command = ["cargo", *arguments, "--manifest-path", str(path)]
            print("+", " ".join(command), flush=True)
            subprocess.run(command, cwd=ROOT, check=True, env=ENV)

        run("generate-lockfile", "--offline")
        run("check", "--workspace", "--locked", "--offline")
        run("test", "--workspace", "--locked", "--offline")
        # Options after `--` belong to Clippy's compiler rather than Cargo.
        subprocess.run(["cargo", "clippy", "--manifest-path", str(path),
                        "--workspace", "--all-targets", "--locked", "--offline", "--", "-D", "warnings"],
                       cwd=ROOT, check=True, env=ENV)
        if args.bench:
            run("bench", "--locked", "--offline", "--bench", "buffer", "--bench", "engine")
        if args.build_terminal:
            command = ["cargo", "build", "-p", "embedide-terminal", "--bin", "embedide-terminal",
                       "--manifest-path", str(path), "--locked", "--offline", "--message-format=json-render-diagnostics"]
            if args.release:
                command.append("--release")
            print("+", " ".join(command), flush=True)
            built = subprocess.run(command, cwd=ROOT, check=True, stdout=subprocess.PIPE, text=True)
            artifacts = [json.loads(line) for line in built.stdout.splitlines()]
            executables = [item["executable"] for item in artifacts
                           if item.get("reason") == "compiler-artifact" and item.get("executable")
                           and item.get("target", {}).get("name") == "embedide-terminal"]
            if len(executables) != 1:
                raise ValueError("Cargo did not report exactly one terminal executable")
            args.build_terminal.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(executables[0], args.build_terminal)
            print("Built terminal frontend:", args.build_terminal.resolve(), flush=True)


if __name__ == "__main__":
    main()
