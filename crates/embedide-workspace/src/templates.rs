//! Project templates. Files are created with exclusive opens, so an existing
//! file is never replaced. An existing non-empty directory is adopted: only
//! `embedide.toml` is written, describing the build system already there.
use crate::{manifest::BuildKind, Error, Manifest, Result, MANIFEST_FILE};
use embedide_toolchains::Language;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
};

/// One MCU from the preserved catalog (hardware support is not implied).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Board {
    /// Catalog id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Family.
    pub family: String,
    /// GCC -mcpu value.
    pub cpu: String,
    /// GCC -mfpu value, if any.
    pub fpu: Option<String>,
    /// soft, softfp or hard.
    pub float_abi: Option<String>,
    /// Flash origin (hex).
    pub flash_origin: String,
    /// RAM origin (hex).
    pub ram_origin: String,
    /// Flash size, KiB.
    pub flash_kb: u32,
    /// RAM size, KiB.
    pub ram_kb: u32,
    /// Vendor header define.
    pub c_define: String,
    /// Rust target triple.
    pub rust_target: String,
    /// OpenOCD target cfg name.
    pub openocd_target: String,
    /// Default OpenOCD interface.
    pub default_adapter: String,
}

/// The MCU catalog (67 entries, migrated from the desktop app).
pub fn boards() -> Vec<Board> {
    serde_json::from_str(include_str!("../../embedide-core/data/legacy-targets.json"))
        .expect("valid catalog")
}

/// Template ids with a one-line description.
pub const TEMPLATES: &[(&str, &str)] = &[
    ("c", "Host C program (native build)"),
    ("cpp", "Host C++ program (native build)"),
    ("rust", "Rust program (Cargo)"),
    ("zig", "Zig program (native build with zig build-exe)"),
    (
        "asm",
        "Host C program calling a GNU assembly function (x86_64/aarch64)",
    ),
    ("fortran", "Fortran program (native build)"),
    (
        "c-cortex-m",
        "Bare-metal C firmware for an Arm Cortex-M board (needs --board, cross GCC)",
    ),
];

/// Options for [`create`].
#[derive(Debug, Default, Clone)]
pub struct CreateOptions {
    /// Template id; defaults to the language's host template.
    pub template: Option<String>,
    /// Primary language (selects the default template).
    pub language: Option<Language>,
    /// Toolchain choices to record (language id → toolchain id).
    pub toolchain: BTreeMap<String, String>,
    /// Board id for firmware templates.
    pub board: Option<String>,
}

/// What [`create`] did.
#[derive(Debug, serde::Serialize)]
pub struct Created {
    /// Project root.
    pub root: PathBuf,
    /// Files written (relative paths).
    pub files: Vec<String>,
    /// True when an existing directory was adopted without adding sources.
    pub adopted: bool,
    /// Build system recorded in the manifest.
    pub system: BuildKind,
}

fn write_new(root: &Path, rel: &str, content: &str, files: &mut Vec<String>) -> Result<()> {
    let path = root.join(rel);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| {
            Error(format!(
                "Cannot create {} (existing files are never overwritten): {e}",
                path.display()
            ))
        })?;
    f.write_all(content.as_bytes())?;
    files.push(rel.to_string());
    Ok(())
}

fn mem(kb: u32) -> String {
    if kb >= 1024 && kb % 1024 == 0 {
        format!("{}M", kb / 1024)
    } else {
        format!("{kb}K")
    }
}

/// GCC flags for a board's core (same rules as the desktop templates).
pub fn cpu_flags(b: &Board) -> Vec<String> {
    let mut v = vec![format!("-mcpu={}", b.cpu), "-mthumb".into()];
    if let Some(fpu) = &b.fpu {
        v.push(format!("-mfpu={fpu}"));
        v.push(format!(
            "-mfloat-abi={}",
            b.float_abi.as_deref().unwrap_or("hard")
        ));
    }
    v
}

fn linker_script(b: &Board) -> String {
    format!(
        "MEMORY\n{{\n    FLASH (rx)  : ORIGIN = {}, LENGTH = {}\n    RAM (xrw)   : ORIGIN = {}, LENGTH = {}\n}}\n\n_estack = ORIGIN(RAM) + LENGTH(RAM);\n\nSECTIONS\n{{\n    .isr_vector : {{ KEEP(*(.isr_vector)) }} > FLASH\n    .text : {{ *(.text*) }} > FLASH\n    .rodata : {{ *(.rodata*) }} > FLASH\n    .data : {{ *(.data*) }} > RAM AT > FLASH\n    .bss : {{ *(.bss*) *(COMMON) }} > RAM\n}}\n",
        b.flash_origin, mem(b.flash_kb), b.ram_origin, mem(b.ram_kb)
    )
}

fn startup(b: &Board) -> String {
    // binutils 2.43 rejects `.cpu cortex-m85`; -mcpu on the command line selects it
    let cpu = if b.cpu == "cortex-m85" {
        format!("@ CPU selected by -mcpu={} on the command line", b.cpu)
    } else {
        format!(".cpu {}", b.cpu)
    };
    let vectors = "    .word Default_Handler\n".repeat(14);
    format!(
        ".syntax unified\n{cpu}\n.thumb\n\n.global g_pfnVectors\n.global Default_Handler\n.global Reset_Handler\n\n.section .text.Reset_Handler\n.thumb_func\n.weak Reset_Handler\n.type Reset_Handler, %function\nReset_Handler:\n    ldr r0, =_estack\n    mov sp, r0\n    bl SystemInit\n    bl main\n1:  b 1b\n.size Reset_Handler, .-Reset_Handler\n\n.section .text.Default_Handler,\"ax\",%progbits\n.thumb_func\nDefault_Handler:\n    b Default_Handler\n\n.section .isr_vector,\"a\",%progbits\n.type g_pfnVectors, %object\ng_pfnVectors:\n    .word _estack\n    .word Reset_Handler\n{vectors}.size g_pfnVectors, .-g_pfnVectors\n"
    )
}

/// Detect an existing build system in a directory (for adoption).
fn existing_system(root: &Path) -> Option<BuildKind> {
    let has = |n: &str| root.join(n).is_file();
    if has("Cargo.toml") {
        Some(BuildKind::Cargo)
    } else if has("CMakeLists.txt") {
        Some(BuildKind::Cmake)
    } else if has("build.zig") {
        Some(BuildKind::Zig)
    } else if has("Makefile") || has("makefile") || has("GNUmakefile") {
        Some(BuildKind::Make)
    } else {
        None
    }
}

/// Create a project at `root` (which may not exist yet).
pub fn create(root: &Path, name: &str, opts: &CreateOptions) -> Result<Created> {
    let template =
        opts.template
            .clone()
            .unwrap_or_else(|| match opts.language.unwrap_or(Language::C) {
                Language::C => "c".into(),
                Language::Cpp => "cpp".into(),
                Language::Rust => "rust".into(),
                Language::Zig => "zig".into(),
                Language::Asm => "asm".into(),
                Language::Fortran => "fortran".into(),
            });
    if !TEMPLATES.iter().any(|(t, _)| *t == template) {
        let ids: Vec<_> = TEMPLATES.iter().map(|(t, _)| *t).collect();
        return Err(Error(format!(
            "Unknown template '{template}'; available: {}",
            ids.join(", ")
        )));
    }
    if root.join(MANIFEST_FILE).exists() {
        return Err(Error(format!(
            "{} already exists; nothing was changed",
            root.join(MANIFEST_FILE).display()
        )));
    }
    let non_empty = root.is_dir() && std::fs::read_dir(root)?.next().is_some();
    std::fs::create_dir_all(root)?;
    let mut files = Vec::new();

    if non_empty {
        // Adopt: describe what is there, add nothing else
        let system = existing_system(root).unwrap_or(BuildKind::Native);
        let lang = opts.language.unwrap_or(match system {
            BuildKind::Cargo => Language::Rust,
            BuildKind::Zig => Language::Zig,
            _ => Language::C,
        });
        let mut m = Manifest::new(name, lang, system);
        if system == BuildKind::Native && !root.join("src").is_dir() {
            m.build.sources = vec![".".into()];
        }
        m.toolchain = opts.toolchain.clone();
        m.validate()?;
        write_new(root, MANIFEST_FILE, &m.to_toml()?, &mut files)?;
        return Ok(Created {
            root: root.to_path_buf(),
            files,
            adopted: true,
            system,
        });
    }

    let (lang, system) = match template.as_str() {
        "c" | "c-cortex-m" => (Language::C, BuildKind::Native),
        "cpp" => (Language::Cpp, BuildKind::Native),
        "rust" => (Language::Rust, BuildKind::Cargo),
        "zig" => (Language::Zig, BuildKind::Native),
        "asm" => (Language::Asm, BuildKind::Native),
        _ => (Language::Fortran, BuildKind::Native),
    };
    let mut m = Manifest::new(name, lang, system);
    m.toolchain = opts.toolchain.clone();
    match template.as_str() {
        "c" => {
            write_new(root, "src/main.c", &format!("#include <stdio.h>\n\nint main(void) {{\n    printf(\"Hello from {name}\\n\");\n    return 0;\n}}\n"), &mut files)?;
            m.build.flags = vec!["-Wall".into(), "-Wextra".into()];
        }
        "cpp" => {
            write_new(root, "src/main.cpp", &format!("#include <iostream>\n\nint main() {{\n    std::cout << \"Hello from {name}\\n\";\n    return 0;\n}}\n"), &mut files)?;
            m.build.flags = vec!["-Wall".into(), "-Wextra".into()];
        }
        "rust" => {
            write_new(root, "Cargo.toml", &format!("[package]\nname = \"{}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n", name.replace('.', "-")), &mut files)?;
            write_new(
                root,
                "src/main.rs",
                &format!("fn main() {{\n    println!(\"Hello from {name}\");\n}}\n"),
                &mut files,
            )?;
            write_new(root, ".gitignore", "/target\n/build\n", &mut files)?;
        }
        "zig" => {
            // std.debug.print is stable across Zig 0.13–0.17 (stdout APIs changed in 0.15)
            write_new(root, "src/main.zig", &format!("const std = @import(\"std\");\n\npub fn main() void {{\n    std.debug.print(\"Hello from {name}\\n\", .{{}});\n}}\n"), &mut files)?;
        }
        "asm" => {
            write_new(root, "src/main.c", "#include <stdio.h>\n\nlong add_numbers(long a, long b);\n\nint main(void) {\n    printf(\"2 + 40 = %ld\\n\", add_numbers(2, 40));\n    return 0;\n}\n", &mut files)?;
            write_new(root, "src/add.S", "/* add_numbers(a, b) = a + b, System V / AAPCS64 */\n    .text\n    .globl add_numbers\n#if defined(__APPLE__)\n    .globl _add_numbers\n_add_numbers:\n#endif\nadd_numbers:\n#if defined(__x86_64__)\n    lea (%rdi,%rsi), %rax\n    ret\n#elif defined(__aarch64__)\n    add x0, x0, x1\n    ret\n#else\n#error \"add.S supports x86_64 and aarch64\"\n#endif\n#if defined(__linux__) && defined(__ELF__)\n    .section .note.GNU-stack,\"\",%progbits\n#endif\n", &mut files)?;
            let c_choice = m
                .toolchain
                .get("c")
                .cloned()
                .unwrap_or_else(|| "gcc".into());
            m.toolchain.entry("asm".into()).or_insert(c_choice);
        }
        "fortran" => {
            write_new(
                root,
                "src/main.f90",
                &format!(
                    "program {}\n  implicit none\n  print *, 'Hello from {name}'\nend program\n",
                    name.replace(['-', '.'], "_")
                ),
                &mut files,
            )?;
        }
        "c-cortex-m" => {
            let id = opts.board.clone().ok_or_else(|| {
                Error("c-cortex-m needs --board <id> (see `embedide targets`)".into())
            })?;
            let b = boards()
                .into_iter()
                .find(|b| b.id == id)
                .ok_or_else(|| Error(format!("Unknown board '{id}'; see `embedide targets`")))?;
            write_new(root, "src/main.c", &format!("#include <stdint.h>\n\n/* Target: {} ({}) */\n\nint main(void) {{\n    while (1) {{\n        /* Application loop */\n    }}\n}}\n\nvoid SystemInit(void) {{}}\n", b.name, b.family), &mut files)?;
            write_new(root, "src/startup.S", &startup(&b), &mut files)?;
            write_new(root, "linker.ld", &linker_script(&b), &mut files)?;
            let cross = opts
                .toolchain
                .get("c")
                .cloned()
                .unwrap_or_else(|| "arm-none-eabi-gcc".into());
            m.toolchain.entry("c".into()).or_insert(cross.clone());
            m.toolchain.entry("asm".into()).or_insert(cross);
            m.target.flags = cpu_flags(&b);
            m.target.board = Some(b.id.clone());
            m.build.defines = vec![b.c_define.clone()];
            m.build.flags = vec![
                "-Wall".into(),
                "-ffunction-sections".into(),
                "-fdata-sections".into(),
            ];
            m.build.link_flags = vec!["-nostartfiles".into(), "-Wl,--gc-sections".into()];
            m.build.linker_script = Some("linker.ld".into());
            m.build.artifact = Some(format!("{name}.elf"));
            // keep -g in release so a debugger can still map the image
            if let Some(r) = m.profiles.get_mut("release") {
                r.flags.push("-g".into());
            }
            let mut dbg = toml::Table::new();
            dbg.insert("openocd_interface".into(), b.default_adapter.clone().into());
            dbg.insert("openocd_target".into(), b.openocd_target.clone().into());
            dbg.insert("gdb".into(), "arm-none-eabi-gdb".into());
            m.debug = Some(dbg);
        }
        _ => unreachable!(),
    }
    if template != "rust" {
        write_new(root, ".gitignore", "/build\n", &mut files)?;
    }
    m.validate()?;
    write_new(root, MANIFEST_FILE, &m.to_toml()?, &mut files)?;
    Ok(Created {
        root: root.to_path_buf(),
        files,
        adopted: false,
        system,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("embedide-tpl-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn every_template_validates() {
        for (t, _) in TEMPLATES {
            let d = tmp(t);
            let c = create(
                &d,
                "demo",
                &CreateOptions {
                    template: Some((*t).into()),
                    board: Some("stm32f407vg".into()),
                    ..Default::default()
                },
            )
            .unwrap();
            assert!(c.files.contains(&MANIFEST_FILE.to_string()), "{t}");
            assert!(Manifest::load(&d).unwrap().is_some(), "{t}");
            std::fs::remove_dir_all(&d).unwrap();
        }
    }

    #[test]
    fn existing_directory_is_adopted_without_touching_files() {
        let d = tmp("adopt");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("Makefile"), "all:\n\techo hi\n").unwrap();
        let c = create(&d, "legacy", &CreateOptions::default()).unwrap();
        assert!(c.adopted);
        assert_eq!(c.system, BuildKind::Make);
        assert_eq!(c.files, vec![MANIFEST_FILE.to_string()]);
        assert_eq!(
            std::fs::read_to_string(d.join("Makefile")).unwrap(),
            "all:\n\techo hi\n"
        );
        assert!(create(&d, "legacy", &CreateOptions::default())
            .unwrap_err()
            .0
            .contains("nothing was changed"));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn firmware_template_uses_catalog_cpu_flags() {
        let d = tmp("fw");
        create(
            &d,
            "fw",
            &CreateOptions {
                template: Some("c-cortex-m".into()),
                board: Some("ra8d1".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let m = Manifest::load(&d).unwrap().unwrap();
        assert!(m.target.flags.contains(&"-mcpu=cortex-m85".to_string()));
        let s = std::fs::read_to_string(d.join("src/startup.S")).unwrap();
        assert!(!s.contains(".cpu cortex-m85"));
        assert_eq!(m.toolchain["c"], "arm-none-eabi-gcc");
        std::fs::remove_dir_all(&d).unwrap();
        assert!(create(
            &tmp("fw2"),
            "fw",
            &CreateOptions {
                template: Some("c-cortex-m".into()),
                ..Default::default()
            }
        )
        .is_err());
    }

    #[test]
    fn catalog_has_67_boards() {
        assert_eq!(boards().len(), 67);
    }
}
