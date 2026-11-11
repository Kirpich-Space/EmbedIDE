---
name: asm-review
description: Writing and reviewing assembly (x86-64 NASM/GAS, AArch64, RISC-V). Use when editing .asm/.s/.S files or inline assembly.
license: MIT
---
# Assembly

- Keep the file's syntax: NASM (`mov rax, 1`), GAS AT&T (`movq $1, %rax`) or GAS Intel (`.intel_syntax noprefix`).
- Follow the platform ABI: System V AMD64 (args rdi, rsi, rdx, rcx, r8, r9; preserve rbx, rbp, r12–r15; 16-byte
  aligned stack at `call`), AAPCS64 (x0–x7 args, preserve x19–x28, fp, lr), RISC-V (a0–a7 args, preserve s0–s11).
- Comment every non-obvious instruction with what it does to the program state, not what the mnemonic means.
- Name labels by purpose (`.Lcopy_loop`), keep local labels local.
- Assemble after each change: `nasm -f elf64 x.asm -o x.o` / `as x.s -o x.o` / the project's Makefile, then link and run.
- When changing a routine called from C, keep its prototype and calling convention exactly.
