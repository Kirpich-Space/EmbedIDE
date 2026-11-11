# Assembly examples for EmbedIDE. Run `make` (x86-64 Linux: nasm, GNU as, ld)
# and `make cross` (AArch64 and RISC-V objects with llvm-mc; nothing is run).

X86 := hello-nasm hello-att hello-intel

all: $(X86)

hello-nasm: hello-nasm.asm
	nasm -f elf64 -g -F dwarf -o hello-nasm.o hello-nasm.asm
	ld -o $@ hello-nasm.o

hello-att: hello-att.s
	as -g -o hello-att.o hello-att.s
	ld -o $@ hello-att.o

hello-intel: hello-intel.s
	as -g -o hello-intel.o hello-intel.s
	ld -o $@ hello-intel.o

cross: hello-aarch64.o hello-riscv.o

hello-aarch64.o: hello-aarch64.s
	llvm-mc -triple=aarch64-linux-gnu -filetype=obj -o $@ hello-aarch64.s

hello-riscv.o: hello-riscv.s
	llvm-mc -triple=riscv64-linux-gnu -filetype=obj -o $@ hello-riscv.s

run: all
	./hello-nasm && ./hello-att && ./hello-intel

clean:
	rm -f *.o $(X86)

.PHONY: all cross run clean
