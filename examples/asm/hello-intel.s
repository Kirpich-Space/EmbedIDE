# x86-64 Linux, GNU as (Intel syntax): write(1, msg, len); exit(0)
    .intel_syntax noprefix
    .section .rodata
msg:
    .ascii "Hello from GNU as (Intel)\n"
    .set len, . - msg

    .text
    .globl _start
_start:
    mov     rax, 1              # sys_write
    mov     rdi, 1
    lea     rsi, [rip + msg]
    mov     rdx, len
    syscall
    xor     edi, edi
    mov     rax, 60             # sys_exit
    syscall
