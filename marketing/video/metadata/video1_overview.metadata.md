# video1_overview

## Title

EmbedIDE — a native Rust IDE for embedded and systems development (alpha overview)

## Description

EmbedIDE (`embed`) is a native Rust IDE for embedded and systems development that runs in your terminal: a Vim-style editor, explorer, integrated terminal, builds with jump-to-source diagnostics, an ELF/HEX/disassembly view for the programs you build, and optional AI agents that work in your project through the Claude Code or Codex CLI you already use — with a permission policy you control and diffs you can review and revert.

Official Kirpich Space / EmbedIDE video. Recorded sections are the real program (0.1.0-alpha.1); the architecture slide is an animation and says so. Waiting time is sped up and labelled.

Status: alpha. Linux verified, FreeBSD compiles but has not been run, Windows/macOS not supported. The native GUI is planned, not started.

Repository (GPL-3.0): https://github.com/Kirpich-Space/EmbedIDE
Install (Linux x86_64, static build, checksum-verified):
curl -fsSL https://raw.githubusercontent.com/Kirpich-Space/EmbedIDE/main/install.sh | sh
Build from source: git clone https://github.com/Kirpich-Space/EmbedIDE && cd EmbedIDE && cargo build --release --locked
Found a bug? https://github.com/Kirpich-Space/EmbedIDE/issues/new/choose

Voice: Piper TTS, LibriTTS-R voice (CC BY 4.0). Music: original, synthesised for this video.

0:00 What is EmbedIDE
0:19 Architecture
0:41 Editor and builds
0:52 Integrated terminal
1:03 Binary view
1:16 Claude and Codex
1:33 AI agent edits files
2:12 Status
2:23 Install
2:41 Build from source



## Tags

EmbedIDE, Rust, embedded, IDE, terminal, TUI, firmware, systems programming, open source, Claude Code, Codex CLI, FreeBSD, Linux
