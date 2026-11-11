---
name: rust-crate
description: Conventions for Rust crates and workspaces. Use when editing Rust code, Cargo manifests or tests.
license: MIT
---
# Rust crate

- Keep `cargo fmt --all -- --check` and `cargo clippy --all-targets -- -D warnings` clean.
- Run `cargo test` for the affected crate (`-p name`) after a change; add a regression test for every fixed bug.
- Library code returns `Result` with a typed error; no `unwrap()`/`expect()` on fallible input outside tests.
- Document public items (`///`); keep `unsafe` out unless the crate already allows it, and explain every `unsafe` block.
- Do not add dependencies or change `Cargo.lock` unless the task needs it; prefer the standard library.
- `no_std` crates stay `no_std`: no `std::` paths, use `core::`/`alloc::`.
- Use `--locked` (and `--offline` when the registry cache is all there is) for builds and tests.
