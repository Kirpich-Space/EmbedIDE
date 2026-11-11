# Assembly examples

Small Linux programs in each dialect EmbedIDE recognises. Open any file with `embed hello-nasm.asm`
(or `embed` in this directory) and press F5 to run `make`.

| File | Dialect | Build |
|---|---|---|
| `hello-nasm.asm` | NASM, Intel syntax, x86-64 | `nasm -f elf64` + `ld` |
| `hello-att.s` | GNU as, AT&T syntax, x86-64 | `as` + `ld` |
| `hello-intel.s` | GNU as, `.intel_syntax noprefix`, x86-64 | `as` + `ld` |
| `hello-aarch64.s` | GNU as, AArch64 | `make cross` assembles with `llvm-mc` (object only) |
| `hello-riscv.s` | GNU as, RISC-V 64 | `make cross` assembles with `llvm-mc` (object only) |

```sh
make          # needs nasm, GNU as and ld (binutils)
make run      # prints three greetings
make cross    # needs llvm-mc; produces AArch64 / RISC-V objects, runs nothing
objdump -d hello-nasm | less
```

Linking and running the AArch64/RISC-V programs needs a cross linker (`aarch64-linux-gnu-ld`,
`riscv64-linux-gnu-ld`) and the target machine or an emulator such as `qemu-aarch64`.
