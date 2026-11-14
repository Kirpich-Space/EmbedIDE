# video2_technical_demo

## Title

EmbedIDE technical demo: edit, build, AI agent and permissions in a Rust terminal IDE (alpha)

## Description

A real development session in EmbedIDE (`embed`), the native Rust terminal IDE for embedded and systems work. Every recorded section is the actual program, version 0.1.0-alpha.1; waiting time is sped up and labelled. Nothing is mocked.

What happens: open a C project, build with GCC, read the diagnostics and jump to the error, fix it, use the integrated terminal, then let the Codex CLI agent add a function, build and run it — with a summary, a reviewable diff and revert. Then a Claude Code request to delete a directory is stopped by a permission prompt, and the compiled program is inspected in the ELF/HEX/disassembly view.

Official Kirpich Space / EmbedIDE video. Status: alpha. Linux verified, FreeBSD compiles but not yet run, Windows/macOS not supported, native GUI planned (not available).

Repository (GPL-3.0): https://github.com/Kirpich-Space/EmbedIDE
Install (Linux x86_64): curl -fsSL https://raw.githubusercontent.com/Kirpich-Space/EmbedIDE/main/install.sh | sh
Build from source: git clone https://github.com/Kirpich-Space/EmbedIDE && cd EmbedIDE && cargo build --release --locked
Bugs and feedback: https://github.com/Kirpich-Space/EmbedIDE/issues/new/choose

The AI agents use the Claude Code / Codex CLI you are logged in with; EmbedIDE needs no API key.
Voice: Piper TTS, LibriTTS-R voice (CC BY 4.0). Music: original, synthesised for this video.

0:00 Introduction
0:17 Open a project
0:33 Build, error, jump
0:47 Fix and rebuild
0:56 Integrated terminal
1:09 Skills and policies
1:25 AI agent: the task
1:40 The agent works
1:54 Review and revert
2:06 Run the result
2:15 Permission prompts
2:38 Binary view
2:55 Summary
3:18 Install



## Tags

EmbedIDE, Rust, terminal IDE, TUI, embedded, C programming, GCC, AI coding agent, Claude Code, Codex CLI, ELF, disassembly, open source, Linux, FreeBSD
