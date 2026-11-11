//! Assembly dialects: detection, instruction/register/directive sets, branch
//! recognition and label discovery. Frontend-neutral; the syntax lexer, the
//! completion popup and label navigation use it.
//!
//! Dialects differ in more than spelling: NASM comments start with `;` and
//! registers are bare; GNU as on x86 uses `#` comments, `;` separates
//! statements, and AT&T syntax writes `%rax` and `$imm`; 32-bit ARM uses `@`
//! comments and `#imm`; AArch64 uses `//`; RISC-V uses `#`.
use std::path::Path;

/// An assembly dialect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dialect {
    /// NASM / YASM, Intel syntax (x86, x86-64).
    Nasm,
    /// GNU as, AT&T syntax (x86, x86-64; GNU as's default).
    GasAtt,
    /// GNU as with `.intel_syntax` (x86, x86-64).
    GasIntel,
    /// GNU as for 32-bit ARM / Thumb.
    Arm,
    /// GNU as for AArch64.
    Aarch64,
    /// GNU as for RISC-V.
    RiscV,
}

impl Dialect {
    /// All dialects.
    pub const ALL: [Dialect; 6] = [
        Self::Nasm,
        Self::GasAtt,
        Self::GasIntel,
        Self::Arm,
        Self::Aarch64,
        Self::RiscV,
    ];

    /// Configuration id.
    pub fn id(self) -> &'static str {
        match self {
            Self::Nasm => "nasm",
            Self::GasAtt => "gas-att",
            Self::GasIntel => "gas-intel",
            Self::Arm => "arm",
            Self::Aarch64 => "aarch64",
            Self::RiscV => "riscv",
        }
    }

    /// Display name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Nasm => "NASM (Intel)",
            Self::GasAtt => "GNU as, AT&T",
            Self::GasIntel => "GNU as, Intel",
            Self::Arm => "GNU as, ARM",
            Self::Aarch64 => "GNU as, AArch64",
            Self::RiscV => "GNU as, RISC-V",
        }
    }

    /// Parse a configuration id.
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.id() == s)
    }

    /// x86 / x86-64 dialects.
    pub fn is_x86(self) -> bool {
        matches!(self, Self::Nasm | Self::GasAtt | Self::GasIntel)
    }

    /// Detect from the file extension and a sample of its text: `.asm`,
    /// `.nasm` → NASM; GNU as files by `.intel_syntax`, architecture
    /// directives and register names; plain x86 GNU as defaults to AT&T.
    pub fn detect(path: Option<&Path>, sample: &str) -> Self {
        let ext = path
            .and_then(|p| p.extension())
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let text = sample.to_ascii_lowercase();
        let code: String = text
            .lines()
            .take(400)
            .map(|l| l.split(['#', ';', '@']).next().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n");
        let words: Vec<&str> = code
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '%'))
            .filter(|w| !w.is_empty())
            .collect();
        let has = |w: &str| words.contains(&w);
        let count = |pred: &dyn Fn(&str) -> bool| words.iter().filter(|w| pred(w)).count();
        let nasm_like = matches!(ext.as_str(), "asm" | "nasm" | "inc")
            || has("bits") && (has("section") || has("global"))
            || text.contains("%define")
            || text.contains("%macro");
        if nasm_like && !text.contains(".intel_syntax") && !text.contains(".globl") {
            return Self::Nasm;
        }
        if text.contains(".intel_syntax") {
            return Self::GasIntel;
        }
        let riscv = count(&|w| {
            matches!(
                w,
                "a0" | "a1"
                    | "a2"
                    | "a7"
                    | "t0"
                    | "t1"
                    | "s0"
                    | "ra"
                    | "zero"
                    | "ecall"
                    | "jal"
                    | "jalr"
                    | "li"
                    | "la"
                    | "auipc"
                    | ".option"
            )
        });
        let aarch64 = count(&|w| {
            (w.len() >= 2
                && (w.starts_with('x') || w.starts_with('w'))
                && w[1..].parse::<u8>().is_ok_and(|n| n <= 30))
                || matches!(
                    w,
                    "xzr" | "wzr" | "svc" | "adrp" | "ldp" | "stp" | "blr" | "cbz" | "cbnz"
                )
        });
        let arm = count(&|w| {
            (w.len() >= 2 && w.starts_with('r') && w[1..].parse::<u8>().is_ok_and(|n| n <= 15))
                || matches!(
                    w,
                    "bx" | "push" | "pop" | ".thumb" | ".arm" | "ldr" | "str" | "swi" | ".syntax"
                )
        });
        let x86 = count(&|w| {
            w.starts_with('%')
                || matches!(
                    w,
                    "rax"
                        | "eax"
                        | "rsp"
                        | "rbp"
                        | "rdi"
                        | "rsi"
                        | "movq"
                        | "movl"
                        | "syscall"
                        | "int"
                        | "pushq"
                        | "popq"
                )
        });
        if text.contains(".arch armv8") || text.contains(".arch aarch64") {
            return Self::Aarch64;
        }
        if text.contains(".cpu cortex")
            || text.contains(".thumb")
            || text.contains(".syntax unified")
        {
            return Self::Arm;
        }
        let best = [
            (riscv, Self::RiscV),
            (aarch64, Self::Aarch64),
            (arm, Self::Arm),
        ]
        .into_iter()
        .max_by_key(|(n, _)| *n)
        .filter(|(n, _)| *n > x86 && *n >= 2);
        match best {
            Some((_, dialect)) => dialect,
            None if matches!(ext.as_str(), "asm" | "nasm" | "inc") => Self::Nasm,
            None => Self::GasAtt,
        }
    }

    /// Instruction mnemonics offered for completion (common subset; the
    /// assembler remains the authority).
    pub fn mnemonics(self) -> &'static [&'static str] {
        match self {
            Self::Nasm | Self::GasIntel => X86,
            Self::GasAtt => X86_ATT,
            Self::Arm => ARM,
            Self::Aarch64 => AARCH64,
            Self::RiscV => RISCV,
        }
    }

    /// Assembler directives.
    pub fn directives(self) -> &'static [&'static str] {
        match self {
            Self::Nasm => NASM_DIRECTIVES,
            Self::GasAtt | Self::GasIntel | Self::Arm | Self::Aarch64 | Self::RiscV => {
                GAS_DIRECTIVES
            }
        }
    }

    /// Register names (AT&T without the `%`, which is typed before them).
    pub fn registers(self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut range = |prefix: &str, from: u32, to: u32| {
            out.extend((from..=to).map(|n| format!("{prefix}{n}")));
        };
        match self {
            Self::Nasm | Self::GasAtt | Self::GasIntel => {
                range("r", 8, 15);
                range("xmm", 0, 15);
                range("ymm", 0, 15);
                out.extend(
                    [
                        "rax", "rbx", "rcx", "rdx", "rsi", "rdi", "rbp", "rsp", "rip", "eax",
                        "ebx", "ecx", "edx", "esi", "edi", "ebp", "esp", "ax", "bx", "cx", "dx",
                        "si", "di", "bp", "sp", "al", "ah", "bl", "bh", "cl", "ch", "dl", "dh",
                        "sil", "dil", "bpl", "spl", "cs", "ds", "es", "fs", "gs", "ss", "cr0",
                        "cr2", "cr3", "cr4", "rflags",
                    ]
                    .map(String::from),
                );
                for n in 8..=15 {
                    out.extend(["d", "w", "b"].map(|s| format!("r{n}{s}")));
                }
            }
            Self::Arm => {
                range("r", 0, 15);
                range("s", 0, 31);
                range("d", 0, 31);
                out.extend(
                    ["sp", "lr", "pc", "ip", "fp", "apsr", "cpsr", "spsr"].map(String::from),
                );
            }
            Self::Aarch64 => {
                range("x", 0, 30);
                range("w", 0, 30);
                range("v", 0, 31);
                range("q", 0, 31);
                range("d", 0, 31);
                range("s", 0, 31);
                out.extend(
                    ["sp", "xzr", "wzr", "lr", "fp", "nzcv", "fpcr", "fpsr"].map(String::from),
                );
            }
            Self::RiscV => {
                range("x", 0, 31);
                range("a", 0, 7);
                range("t", 0, 6);
                range("s", 0, 11);
                range("f", 0, 31);
                range("fa", 0, 7);
                out.extend(["zero", "ra", "sp", "gp", "tp", "fp", "pc"].map(String::from));
            }
        }
        out
    }

    /// Whether `mnemonic` transfers control (jumps, calls, returns, branches).
    pub fn is_branch(self, mnemonic: &str) -> bool {
        let m = mnemonic.to_ascii_lowercase();
        let m = m.as_str();
        match self {
            Self::Nasm | Self::GasAtt | Self::GasIntel => {
                let base = m.trim_end_matches(['q', 'l', 'w']);
                m.starts_with('j')
                    || matches!(
                        m,
                        "call"
                            | "callq"
                            | "calll"
                            | "ret"
                            | "retq"
                            | "retl"
                            | "iret"
                            | "iretq"
                            | "loop"
                            | "loope"
                            | "loopne"
                            | "syscall"
                            | "sysret"
                            | "int"
                            | "int3"
                            | "leave"
                    )
                    || matches!(base, "call" | "ret")
            }
            Self::Arm => {
                matches!(
                    m,
                    "b" | "bl" | "bx" | "blx" | "cbz" | "cbnz" | "svc" | "swi" | "pop" | "ldm"
                ) || (m.len() == 3 && m.starts_with('b') && CONDITIONS.contains(&&m[1..]))
                    || (m.len() == 4 && m.starts_with("bl") && CONDITIONS.contains(&&m[2..]))
            }
            Self::Aarch64 => {
                matches!(
                    m,
                    "b" | "bl"
                        | "br"
                        | "blr"
                        | "ret"
                        | "cbz"
                        | "cbnz"
                        | "tbz"
                        | "tbnz"
                        | "svc"
                        | "eret"
                ) || m
                    .strip_prefix("b.")
                    .is_some_and(|c| CONDITIONS.contains(&c))
            }
            Self::RiscV => matches!(
                m,
                "j" | "jal"
                    | "jalr"
                    | "jr"
                    | "ret"
                    | "call"
                    | "tail"
                    | "beq"
                    | "bne"
                    | "blt"
                    | "bge"
                    | "bltu"
                    | "bgeu"
                    | "beqz"
                    | "bnez"
                    | "blez"
                    | "bgez"
                    | "bltz"
                    | "bgtz"
                    | "bgt"
                    | "ble"
                    | "bgtu"
                    | "bleu"
                    | "ecall"
                    | "ebreak"
                    | "mret"
                    | "sret"
            ),
        }
    }
}

const CONDITIONS: &[&str] = &[
    "eq", "ne", "cs", "hs", "cc", "lo", "mi", "pl", "vs", "vc", "hi", "ls", "ge", "lt", "gt", "le",
    "al",
];

const X86: &[&str] = &[
    "mov", "movzx", "movsx", "movsxd", "lea", "push", "pop", "add", "sub", "adc", "sbb", "inc",
    "dec", "neg", "mul", "imul", "div", "idiv", "and", "or", "xor", "not", "shl", "shr", "sal",
    "sar", "rol", "ror", "rcl", "rcr", "test", "cmp", "xchg", "cmpxchg", "bswap", "bt", "bts",
    "btr", "bsf", "bsr", "lzcnt", "tzcnt", "popcnt", "cbw", "cwde", "cdqe", "cwd", "cdq", "cqo",
    "jmp", "je", "jne", "jz", "jnz", "jg", "jge", "jl", "jle", "ja", "jae", "jb", "jbe", "js",
    "jns", "jo", "jno", "jc", "jnc", "jrcxz", "call", "ret", "leave", "enter", "loop", "nop",
    "hlt", "int", "int3", "syscall", "sysret", "cpuid", "rdtsc", "cli", "sti", "cld", "std",
    "pushf", "popf", "sete", "setne", "setg", "setl", "setge", "setle", "seta", "setb", "cmove",
    "cmovne", "cmovg", "cmovl", "cmova", "cmovb", "rep", "repe", "repne", "movsb", "movsq",
    "stosb", "stosq", "lodsb", "scasb", "cmpsb", "movd", "movq", "movaps", "movups", "movdqa",
    "movdqu", "movss", "movsd", "addss", "addsd", "subss", "mulss", "divss", "pxor", "xorps",
    "paddd", "pcmpeqb", "pmovmskb", "lock", "pause", "mfence", "lfence", "sfence", "ud2",
];

const X86_ATT: &[&str] = &[
    "movq", "movl", "movw", "movb", "movabsq", "movzbl", "movzwl", "movsbl", "movslq", "leaq",
    "leal", "pushq", "popq", "addq", "addl", "subq", "subl", "incq", "decq", "negq", "imulq",
    "idivq", "divq", "andq", "orq", "xorq", "xorl", "notq", "shlq", "shrq", "sarq", "testq",
    "testl", "cmpq", "cmpl", "cmpb", "xchgq", "cqto", "cltq", "jmp", "je", "jne", "jz", "jnz",
    "jg", "jge", "jl", "jle", "ja", "jae", "jb", "jbe", "js", "jns", "call", "callq", "ret",
    "retq", "leave", "leaveq", "nop", "hlt", "int", "int3", "syscall", "cpuid", "rdtsc", "sete",
    "setne", "setl", "setg", "cmove", "cmovne", "rep", "movsb", "stosb", "movd", "movaps",
    "movups", "movdqa", "movdqu", "pxor", "xorps", "lock", "pause", "mfence", "ud2",
];

const ARM: &[&str] = &[
    "mov", "mvn", "add", "adds", "adc", "sub", "subs", "sbc", "rsb", "mul", "mla", "umull",
    "smull", "sdiv", "udiv", "and", "orr", "eor", "bic", "lsl", "lsr", "asr", "ror", "cmp", "cmn",
    "tst", "teq", "ldr", "ldrb", "ldrh", "ldrd", "str", "strb", "strh", "strd", "ldm", "stm",
    "push", "pop", "b", "bl", "bx", "blx", "beq", "bne", "bgt", "blt", "bge", "ble", "bhi", "bls",
    "cbz", "cbnz", "it", "ite", "svc", "nop", "wfi", "wfe", "dmb", "dsb", "isb", "cpsid", "cpsie",
    "mrs", "msr", "movw", "movt", "adr", "uxtb", "uxth", "sxtb", "sxth", "rev", "clz",
];

const AARCH64: &[&str] = &[
    "mov", "movz", "movk", "movn", "mvn", "add", "adds", "sub", "subs", "neg", "mul", "madd",
    "msub", "sdiv", "udiv", "and", "orr", "eor", "bic", "lsl", "lsr", "asr", "ror", "cmp", "cmn",
    "tst", "csel", "cset", "csinc", "ldr", "ldrb", "ldrh", "ldrsw", "ldp", "ldur", "str", "strb",
    "strh", "stp", "stur", "adr", "adrp", "b", "bl", "br", "blr", "ret", "cbz", "cbnz", "tbz",
    "tbnz", "b.eq", "b.ne", "b.lt", "b.le", "b.gt", "b.ge", "b.hi", "b.lo", "svc", "nop", "wfi",
    "dmb", "dsb", "isb", "mrs", "msr", "fmov", "fadd", "fsub", "fmul", "fdiv", "scvtf", "fcvtzs",
];

const RISCV: &[&str] = &[
    "li", "la", "mv", "not", "neg", "add", "addi", "sub", "lui", "auipc", "and", "andi", "or",
    "ori", "xor", "xori", "sll", "slli", "srl", "srli", "sra", "srai", "slt", "slti", "sltu",
    "seqz", "snez", "mul", "mulh", "div", "divu", "rem", "remu", "lb", "lbu", "lh", "lhu", "lw",
    "lwu", "ld", "sb", "sh", "sw", "sd", "j", "jal", "jalr", "jr", "ret", "call", "tail", "beq",
    "bne", "blt", "bge", "bltu", "bgeu", "beqz", "bnez", "ecall", "ebreak", "fence", "nop", "csrr",
    "csrw", "csrrw", "csrrs", "mret", "wfi", "flw", "fsw", "fld", "fsd", "fadd.s",
];

const NASM_DIRECTIVES: &[&str] = &[
    "section",
    "segment",
    "global",
    "extern",
    "bits",
    "default",
    "align",
    "db",
    "dw",
    "dd",
    "dq",
    "dt",
    "resb",
    "resw",
    "resd",
    "resq",
    "times",
    "equ",
    "incbin",
    "%define",
    "%macro",
    "%endmacro",
    "%include",
    "%if",
    "%ifdef",
    "%else",
    "%endif",
    "%assign",
    "struc",
    "endstruc",
    "rel",
];

const GAS_DIRECTIVES: &[&str] = &[
    ".text",
    ".data",
    ".bss",
    ".rodata",
    ".section",
    ".globl",
    ".global",
    ".extern",
    ".type",
    ".size",
    ".byte",
    ".short",
    ".word",
    ".long",
    ".quad",
    ".int",
    ".ascii",
    ".asciz",
    ".string",
    ".zero",
    ".space",
    ".fill",
    ".align",
    ".balign",
    ".p2align",
    ".equ",
    ".set",
    ".macro",
    ".endm",
    ".rept",
    ".endr",
    ".if",
    ".else",
    ".endif",
    ".include",
    ".comm",
    ".lcomm",
    ".intel_syntax",
    ".att_syntax",
    ".cfi_startproc",
    ".cfi_endproc",
    ".file",
    ".loc",
    ".arch",
    ".cpu",
    ".thumb",
    ".arm",
    ".syntax",
    ".option",
    ".weak",
    ".hidden",
];

/// A label defined in the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    /// Name without the colon.
    pub name: String,
    /// 0-based line.
    pub line: usize,
}

/// Labels defined in `text`: `name:` at the start of a statement (after
/// optional indentation), outside comments; NASM local labels (`.loop:`)
/// and GNU numeric labels (`1:`) included. Bounded to 10 000 labels.
pub fn labels(text: &str, dialect: Dialect) -> Vec<Label> {
    let comment = |line: &str| -> usize {
        let marks: &[char] = match dialect {
            Dialect::Nasm => &[';'],
            Dialect::GasAtt | Dialect::GasIntel | Dialect::RiscV => &['#'],
            Dialect::Arm => &['@'],
            Dialect::Aarch64 => &[],
        };
        let mut end = line.find(marks).unwrap_or(line.len());
        if let Some(i) = line.find("//") {
            end = end.min(i);
        }
        if let Some(i) = line.find("/*") {
            end = end.min(i);
        }
        end
    };
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let code = line[..comment(line)].trim_start();
        let Some(colon) = code.find(':') else {
            continue;
        };
        let name = &code[..colon];
        let valid = !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '$' | '@'))
            && !name.starts_with('$');
        if valid {
            out.push(Label {
                name: name.to_string(),
                line: index,
            });
            if out.len() >= 10_000 {
                break;
            }
        }
    }
    out
}
