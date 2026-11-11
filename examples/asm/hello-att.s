# x86-64 Linux, GNU as (AT&T syntax): write(1, msg, len); exit(0)
    .section .rodata
msg:
    .ascii "Hello from GNU as (AT&T)\n"
    .set len, . - msg

    .text
    .globl _start
    .type _start, @function
_start:
    movq    $1, %rax            # sys_write
    movq    $1, %rdi            # stdout
    leaq    msg(%rip), %rsi
    movq    $len, %rdx
    syscall
    xorl    %edi, %edi          # status 0
    movq    $60, %rax           # sys_exit
    syscall
