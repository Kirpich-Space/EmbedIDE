; x86-64 Linux, NASM (Intel syntax): write(1, msg, len); exit(0)
bits 64
default rel

section .rodata
msg:    db "Hello from NASM", 10
len     equ $ - msg

section .text
global _start
_start:
    mov     eax, 1              ; sys_write
    mov     edi, 1              ; stdout
    lea     rsi, [msg]
    mov     edx, len
    syscall
    test    rax, rax
    js      .fail
    xor     edi, edi            ; status 0
    jmp     .exit
.fail:
    mov     edi, 1
.exit:
    mov     eax, 60             ; sys_exit
    syscall
