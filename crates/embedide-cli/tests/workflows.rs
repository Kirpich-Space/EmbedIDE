//! End-to-end CLI workflows with REAL installed compilers.
//!
//! Every test runs the built `embedide` binary in a fresh directory whose
//! path contains a space, with an isolated EMBEDIDE_CONFIG_DIR. A test whose
//! compiler is missing prints `SKIP: …` and returns (it does not pass silently
//! as "verified"; the message says what was not checked).
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    process::Command,
};

struct Env {
    dir: PathBuf,
    cfg: PathBuf,
}

impl Env {
    fn new(name: &str) -> Self {
        let base = std::env::temp_dir().join(format!("embedide wf {} {name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let cfg = base.join("config dir");
        Self { dir: base, cfg }
    }
    fn cli(&self, cwd: &Path, args: &[&str]) -> (Value, i32, String) {
        let out = Command::new(env!("CARGO_BIN_EXE_embedide"))
            .arg("--json")
            .args(args)
            .current_dir(cwd)
            .env("EMBEDIDE_CONFIG_DIR", &self.cfg)
            // the test harness's own target dir must not redirect project builds
            .env_remove("CARGO_TARGET_DIR")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        let v: Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
            panic!(
                "bad JSON ({e}): {stdout}\nstderr: {}",
                String::from_utf8_lossy(&out.stderr)
            )
        });
        (
            v,
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }
    fn ok(&self, cwd: &Path, args: &[&str]) -> Value {
        let (v, code, err) = self.cli(cwd, args);
        assert_eq!(code, 0, "embedide {args:?} failed: {v}\n{err}");
        v
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn have(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success() || !o.stdout.is_empty() || !o.stderr.is_empty())
}

fn skip(what: &str) {
    println!("SKIP: {what}");
}

fn run_artifact(path: &str) -> String {
    let out = Command::new(path).output().unwrap();
    assert!(out.status.success(), "artifact failed: {path}");
    String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr)
}

#[test]
fn help_version_and_doctor_work_without_a_project() {
    let e = Env::new("help");
    let out = Command::new(env!("CARGO_BIN_EXE_embedide"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("embedide "));
    let out = Command::new(env!("CARGO_BIN_EXE_embedide"))
        .arg("--help")
        .output()
        .unwrap();
    let help = String::from_utf8_lossy(&out.stdout);
    for cmd in [
        "toolchain",
        "project",
        "build",
        "clean",
        "terminal",
        "config",
        "ai",
        "doctor",
    ] {
        assert!(help.contains(cmd), "help lists {cmd}");
    }
    let v = e.ok(&e.dir, &["doctor"]);
    assert_eq!(v["data"]["hardware_probed"], false);
    assert!(v["data"]["languages"].is_object());
}

#[test]
fn c_project_with_gcc_clang_and_zig_cc_are_separate_configurations() {
    let e = Env::new("c3");
    let proj = e.dir.join("my app");
    e.ok(
        &e.dir,
        &[
            "project",
            "init",
            proj.to_str().unwrap(),
            "--language",
            "c",
            "--name",
            "hello",
        ],
    );
    let mut built = 0;
    for tc in ["gcc", "clang", "zig-cc"] {
        let available = e.ok(&e.dir, &["toolchain", "list", "--language", "c"])["data"]
            ["toolchains"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["id"] == tc && t["problem"].is_null());
        if !available {
            skip(&format!(
                "{tc} not installed: C build with {tc} not verified"
            ));
            continue;
        }
        let v = e.ok(&proj, &["build", "--toolchain", &format!("c={tc}")]);
        let sel = &v["data"]["prepared"]["toolchains"][0];
        assert_eq!(sel["id"], tc, "selected exactly the requested compiler");
        assert_eq!(sel["reason"], "command line");
        let artifact = v["data"]["outcome"]["artifact"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(run_artifact(&artifact).contains("Hello from hello"));
        built += 1;
    }
    assert!(
        built > 0,
        "at least one C compiler is required for this suite"
    );
}

#[test]
fn project_toolchain_choice_profiles_incremental_and_errors() {
    if !have("gcc") {
        return skip("gcc missing: profile/incremental/error workflow not verified");
    }
    let e = Env::new("inc");
    let proj = e.dir.join("inc proj");
    e.ok(
        &e.dir,
        &[
            "project",
            "init",
            proj.to_str().unwrap(),
            "--language",
            "c",
            "--toolchain",
            "c=gcc",
        ],
    );
    let info = e.ok(&proj, &["project", "info"]);
    assert_eq!(info["data"]["toolchains"][0]["id"], "gcc");
    assert_eq!(info["data"]["toolchains"][0]["reason"], "project");

    let v = e.ok(&proj, &["build"]);
    assert_eq!(v["data"]["outcome"]["success"], true);
    assert_eq!(
        v["data"]["prepared"]["steps"].as_array().unwrap().len(),
        2,
        "compile + link"
    );
    // nothing changed: no steps
    let v = e.ok(&proj, &["build"]);
    assert_eq!(v["data"]["prepared"]["steps"].as_array().unwrap().len(), 0);
    assert_eq!(v["data"]["prepared"]["up_to_date"], 1);
    // header dependency triggers a rebuild through the depfile
    std::fs::create_dir_all(proj.join("include")).unwrap();
    std::fs::write(proj.join("include/conf.h"), "#define GREETING \"one\"\n").unwrap();
    std::fs::write(
        proj.join("src/main.c"),
        "#include <stdio.h>\n#include \"conf.h\"\nint main(void) { puts(GREETING); return 0; }\n",
    )
    .unwrap();
    e.ok(
        &proj,
        &[
            "config",
            "set",
            "build.include",
            r#"["include"]"#,
            "--project",
            proj.to_str().unwrap(),
        ],
    );
    let v = e.ok(&proj, &["build"]);
    assert!(run_artifact(v["data"]["outcome"]["artifact"].as_str().unwrap()).contains("one"));
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(proj.join("include/conf.h"), "#define GREETING \"two\"\n").unwrap();
    let v = e.ok(&proj, &["build"]);
    assert_eq!(
        v["data"]["prepared"]["steps"].as_array().unwrap().len(),
        2,
        "header change recompiles"
    );
    assert!(run_artifact(v["data"]["outcome"]["artifact"].as_str().unwrap()).contains("two"));
    // release profile is a separate output with different flags
    let v = e.ok(&proj, &["build", "--profile", "release"]);
    assert!(v["data"]["outcome"]["artifact"]
        .as_str()
        .unwrap()
        .contains("release"));
    let args = v["data"]["prepared"]["steps"][0]["args"]
        .as_array()
        .unwrap();
    assert!(args.iter().any(|a| a == "-O2") && args.iter().any(|a| a == "-DNDEBUG"));
    // compiler error: non-zero exit, structured diagnostic, no crash
    std::fs::write(
        proj.join("src/main.c"),
        "int main(void) { return undefined_symbol; }\n",
    )
    .unwrap();
    let (v, code, _) = e.cli(&proj, &["build"]);
    assert_ne!(code, 0);
    assert_eq!(v["ok"], false);
    let d = &v["data"]["outcome"]["diagnostics"][0];
    assert_eq!(d["severity"], "error");
    assert!(d["file"].as_str().unwrap().ends_with("main.c"));
    assert_eq!(d["line"], 1);
    // unknown profile and unknown toolchain are explained
    let (v, code, _) = e.cli(&proj, &["build", "--profile", "nope"]);
    assert_eq!(code, 1);
    assert!(v["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Unknown profile"));
    let (v, _, _) = e.cli(&proj, &["build", "--toolchain", "c=tcc9000"]);
    assert!(v["error"]["message"]
        .as_str()
        .unwrap()
        .contains("not installed"));
    let (v, _, _) = e.cli(&proj, &["build", "--toolchain", "c=g++"]);
    assert!(v["error"]["message"]
        .as_str()
        .unwrap()
        .contains("compiles cpp, not c"));
}

#[test]
fn clean_only_removes_embedide_output() {
    if !have("gcc") {
        return skip("gcc missing: clean not verified");
    }
    let e = Env::new("clean");
    let proj = e.dir.join("p");
    e.ok(
        &e.dir,
        &[
            "project",
            "init",
            proj.to_str().unwrap(),
            "--language",
            "c",
            "--toolchain",
            "c=gcc",
        ],
    );
    e.ok(&proj, &["build"]);
    assert!(proj.join("build/debug").is_dir());
    e.ok(&proj, &["clean"]);
    assert!(!proj.join("build").exists());
    assert!(proj.join("src/main.c").is_file(), "sources untouched");
    // a user directory named like the output, without the marker, is refused
    std::fs::create_dir_all(proj.join("build")).unwrap();
    std::fs::write(proj.join("build/important.txt"), "keep").unwrap();
    let (v, code, _) = e.cli(&proj, &["clean"]);
    assert_eq!(code, 1);
    assert!(v["error"]["message"]
        .as_str()
        .unwrap()
        .contains("not created by EmbedIDE"));
    assert!(proj.join("build/important.txt").is_file());
}

#[test]
fn cpp_asm_nasm_and_zig_projects_build() {
    let e = Env::new("langs");
    if have("g++") || have("clang++") {
        let p = e.dir.join("cpp");
        e.ok(
            &e.dir,
            &["project", "init", p.to_str().unwrap(), "--language", "cpp"],
        );
        let v = e.ok(&p, &["build"]);
        assert!(run_artifact(v["data"]["outcome"]["artifact"].as_str().unwrap()).contains("Hello"));
    } else {
        skip("no C++ compiler: C++ build not verified");
    }
    if have("gcc") || have("clang") {
        let p = e.dir.join("asm");
        e.ok(
            &e.dir,
            &["project", "init", p.to_str().unwrap(), "--template", "asm"],
        );
        let v = e.ok(&p, &["build"]);
        assert!(
            run_artifact(v["data"]["outcome"]["artifact"].as_str().unwrap())
                .contains("2 + 40 = 42")
        );
    } else {
        skip("no C driver: GNU assembly not verified");
    }
    if have("nasm") && have("gcc") && cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        let p = e.dir.join("nasm");
        e.ok(
            &e.dir,
            &["project", "init", p.to_str().unwrap(), "--template", "asm"],
        );
        std::fs::remove_file(p.join("src/add.S")).unwrap();
        std::fs::write(p.join("src/add.asm"), "global add_numbers\nsection .text\nadd_numbers:\n    lea rax, [rdi+rsi]\n    ret\nsection .note.GNU-stack noalloc noexec nowrite progbits\n").unwrap();
        let v = e.ok(&p, &["build", "--toolchain", "asm=nasm"]);
        assert!(v["data"]["prepared"]["toolchains"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["id"] == "nasm"));
        assert!(run_artifact(v["data"]["outcome"]["artifact"].as_str().unwrap()).contains("42"));
    } else {
        skip("nasm/gcc missing or not x86_64 Linux: NASM not verified");
    }
    if have("zig") {
        let p = e.dir.join("zig");
        e.ok(
            &e.dir,
            &["project", "init", p.to_str().unwrap(), "--language", "zig"],
        );
        let v = e.ok(&p, &["build"]);
        assert!(
            run_artifact(v["data"]["outcome"]["artifact"].as_str().unwrap())
                .contains("Hello from zig")
        );
    } else {
        skip("zig missing: Zig build not verified");
    }
    for fc in ["gfortran", "flang-new", "flang", "lfortran"] {
        if have(fc) {
            let p = e.dir.join("fortran");
            e.ok(
                &e.dir,
                &[
                    "project",
                    "init",
                    p.to_str().unwrap(),
                    "--language",
                    "fortran",
                ],
            );
            let v = e.ok(&p, &["build"]);
            assert!(
                run_artifact(v["data"]["outcome"]["artifact"].as_str().unwrap()).contains("Hello")
            );
            return;
        }
    }
    skip("no Fortran compiler installed (gfortran/flang/lfortran): Fortran build not verified");
}

#[test]
fn rust_cargo_project_builds_offline() {
    if !have("cargo") {
        return skip("cargo missing: Rust build not verified");
    }
    let e = Env::new("rust");
    let p = e.dir.join("rust app");
    e.ok(
        &e.dir,
        &[
            "project",
            "init",
            p.to_str().unwrap(),
            "--language",
            "rust",
            "--name",
            "rusty",
        ],
    );
    let v = e.ok(&p, &["build"]);
    assert_eq!(v["data"]["prepared"]["system"], "cargo");
    assert!(v["data"]["prepared"]["steps"][0]["args"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a == "--offline"));
    let exe = p.join("target/debug/rusty");
    assert!(run_artifact(exe.to_str().unwrap()).contains("Hello from rusty"));
    let v = e.ok(&p, &["build", "--profile", "release"]);
    assert!(v["data"]["prepared"]["steps"][0]["args"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a == "release"));
}

#[test]
fn existing_make_project_is_adopted_and_built_with_selected_cc() {
    if !have("make") || !have("clang") {
        return skip("make/clang missing: adopted Make project not verified");
    }
    let e = Env::new("make");
    let p = e.dir.join("legacy");
    std::fs::create_dir_all(&p).unwrap();
    std::fs::write(p.join("main.c"), "int main(void){return 0;}\n").unwrap();
    let mk = "CC ?= cc\nall:\n\t$(CC) main.c -o app\n\t$(CC) --version > used.txt\n";
    std::fs::write(p.join("Makefile"), mk).unwrap();
    let v = e.ok(&e.dir, &["project", "init", p.to_str().unwrap()]);
    assert_eq!(v["data"]["adopted"], true);
    assert_eq!(
        std::fs::read_to_string(p.join("Makefile")).unwrap(),
        mk,
        "Makefile untouched"
    );
    e.ok(&p, &["build", "--toolchain", "c=clang"]);
    assert!(std::fs::read_to_string(p.join("used.txt"))
        .unwrap()
        .contains("clang"));
}

#[test]
fn firmware_template_builds_with_cross_gcc() {
    let e = Env::new("fw");
    // Prefer a cross GCC on PATH; fall back to the repository's vendor toolchain
    let vendor = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/toolchain/linux-x64/bin");
    let p = e.dir.join("fw app");
    let args = vec![
        "project",
        "init",
        p.to_str().unwrap(),
        "--template",
        "c-cortex-m",
        "--board",
        "stm32f407vg",
        "--name",
        "fw",
    ];
    let vendor_str = vendor
        .canonicalize()
        .ok()
        .map(|v| v.to_string_lossy().into_owned());
    if !have("arm-none-eabi-gcc") {
        match &vendor_str {
            Some(v) if Path::new(v).join("arm-none-eabi-gcc").is_file() => {
                e.ok(&e.dir, &["toolchain", "search-dir", v]);
            }
            _ => return skip("arm-none-eabi-gcc not found: firmware build not verified"),
        }
    }
    e.ok(&e.dir, &args);
    let v = e.ok(&p, &["build"]);
    let elf = v["data"]["outcome"]["artifact"].as_str().unwrap();
    let bytes = std::fs::read(elf).unwrap();
    assert_eq!(&bytes[..4], b"\x7fELF");
    assert_eq!(u16::from_le_bytes([bytes[18], bytes[19]]), 40, "ARM ELF");
    let args = v["data"]["prepared"]["steps"][0]["args"]
        .as_array()
        .unwrap();
    assert!(args.iter().any(|a| a == "-mcpu=cortex-m4"));
    // the host compiler is refused for a [target] project unless chosen explicitly
    let (v, code, _) = e.cli(&p, &["build", "--toolchain", "c=gcc"]);
    if have("gcc") {
        // a host compiler is refused up front with an explanation, not a late -mcpu error
        assert_eq!(code, 1, "{v}");
        assert!(
            v["error"]["message"]
                .as_str()
                .unwrap()
                .contains("host compiler"),
            "{v}"
        );
    }
}

#[test]
fn config_never_stores_secrets_and_validates() {
    let e = Env::new("cfg");
    let (v, code, _) = e.cli(
        &e.dir,
        &["config", "set", "ai.providers.openai.api_key", "sk-123"],
    );
    assert_eq!(code, 1);
    assert!(v["error"]["message"]
        .as_str()
        .unwrap()
        .contains("credential"));
    e.ok(&e.dir, &["config", "set", "ai.max_tokens", "300"]);
    assert_eq!(
        e.ok(&e.dir, &["config", "get", "ai.max_tokens"])["data"]["value"],
        "300"
    );
    let (_, code, _) = e.cli(&e.dir, &["config", "set", "ai.max_tokens", "0"]);
    assert_eq!(code, 1, "invalid value rejected");
    assert_eq!(
        e.ok(&e.dir, &["config", "get", "ai.max_tokens"])["data"]["value"],
        "300",
        "file unchanged"
    );
}

/// ELF32 section by name → (addr, file offset, size).
fn section(bytes: &[u8], name: &str) -> Option<(u32, usize, usize)> {
    let u16_at = |o: usize| u16::from_le_bytes([bytes[o], bytes[o + 1]]) as usize;
    let u32_at =
        |o: usize| u32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);
    let (shoff, shentsize, shnum, shstrndx) =
        (u32_at(32) as usize, u16_at(46), u16_at(48), u16_at(50));
    let hdr = |i: usize| shoff + i * shentsize;
    let strtab = u32_at(hdr(shstrndx) + 16) as usize;
    (0..shnum).find_map(|i| {
        let h = hdr(i);
        let start = strtab + u32_at(h) as usize;
        let end = start + bytes[start..].iter().position(|b| *b == 0)?;
        (&bytes[start..end] == name.as_bytes()).then(|| {
            (
                u32_at(h + 12),
                u32_at(h + 16) as usize,
                u32_at(h + 20) as usize,
            )
        })
    })
}

#[test]
fn firmware_template_matrix_across_cortex_m_cores() {
    let e = Env::new("fwmatrix");
    let vendor = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/toolchain/linux-x64/bin");
    if !have("arm-none-eabi-gcc") {
        match vendor.canonicalize() {
            Ok(v) if v.join("arm-none-eabi-gcc").is_file() => {
                e.ok(&e.dir, &["toolchain", "search-dir", v.to_str().unwrap()]);
            }
            _ => return skip("arm-none-eabi-gcc not found: firmware matrix not verified"),
        }
    }
    let catalog: Value = e.ok(&e.dir, &["targets"])["data"]["targets"].clone();
    let boards = [
        "stm32f030r8",
        "stm32g071rb",
        "stm32f103c8",
        "stm32f407vg",
        "stm32h743zi",
        "stm32l552ze",
        "stm32u575zi",
        "lpc55s69",
        "mimxrt1062",
        "nrf52840",
        "ra8d1",
        "same70q21",
    ];
    for id in boards {
        let b = catalog
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["id"] == id)
            .unwrap();
        let hex = |k: &str| {
            u32::from_str_radix(b[k].as_str().unwrap().trim_start_matches("0x"), 16).unwrap()
        };
        let (flash, ram) = (hex("flashOrigin"), hex("ramOrigin"));
        let (flash_end, ram_end) = (
            flash + b["flashKb"].as_u64().unwrap() as u32 * 1024,
            ram + b["ramKb"].as_u64().unwrap() as u32 * 1024,
        );
        let p = e.dir.join(format!("fw {id}"));
        e.ok(
            &e.dir,
            &[
                "project",
                "init",
                p.to_str().unwrap(),
                "--template",
                "c-cortex-m",
                "--board",
                id,
                "--name",
                id,
            ],
        );
        let v = e.ok(&p, &["build", "--profile", "release"]);
        let bytes = std::fs::read(v["data"]["outcome"]["artifact"].as_str().unwrap()).unwrap();
        assert_eq!(
            u16::from_le_bytes([bytes[18], bytes[19]]),
            40,
            "{id}: ARM ELF"
        );
        let (addr, off, size) =
            section(&bytes, ".isr_vector").unwrap_or_else(|| panic!("{id}: no .isr_vector"));
        assert!(
            size >= 8 && (flash..flash_end).contains(&addr),
            "{id}: vectors in flash"
        );
        let sp = u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap());
        let reset = u32::from_le_bytes(bytes[off + 4..off + 8].try_into().unwrap());
        assert!(sp > ram && sp <= ram_end, "{id}: SP {sp:#x} in RAM");
        assert_eq!(reset & 1, 1, "{id}: Thumb reset vector");
        assert!(
            (flash..flash_end).contains(&(reset & !1)),
            "{id}: reset in flash"
        );
        assert_eq!(
            v["data"]["outcome"]["diagnostics"]
                .as_array()
                .unwrap()
                .len(),
            0,
            "{id}: no warnings"
        );
    }
}
