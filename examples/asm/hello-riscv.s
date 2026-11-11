# RISC-V 64 Linux, GNU as syntax: write(1, msg, len); exit(0)
    .text
    .globl _start
_start:
    li      a0, 1               # stdout
    la      a1, msg
    li      a2, 20
    li      a7, 64              # sys_write
    ecall
    li      a0, 0
    li      a7, 93              # sys_exit
    ecall
    .section .rodata
msg:
    .ascii "Hello from RISC-V!!\n"
