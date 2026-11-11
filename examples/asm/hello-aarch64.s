// AArch64 Linux, GNU as syntax: write(1, msg, len); exit(0)
    .arch armv8-a
    .text
    .global _start
_start:
    mov     x0, #1              // stdout
    adr     x1, msg
    mov     x2, #len
    mov     x8, #64             // sys_write
    svc     #0
    mov     x0, #0
    mov     x8, #93             // sys_exit
    svc     #0
msg:
    .ascii "Hello from AArch64\n"
    .set len, . - msg
