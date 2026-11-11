//! Service contracts without physical hardware or global PATH mutation.
use embedide_core::{
    config::{NativeConfig, SerialConfig, Target},
    debug::parse_mi,
    diagnostics, explorer, hardware,
    process::{ProcessEvent, ProcessTask},
    BuildPlan, BuildSystem, ProcessSpec, Project,
};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "embedide-services-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
    fn write(&self, path: &str, text: &str) -> PathBuf {
        let path = self.0.join(path);
        fs::write(&path, text).unwrap();
        path
    }
    #[cfg(unix)]
    fn executable(&self, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = self.write(name, body);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    }
    fn plan(&self, program: &str, args: &[&str]) -> BuildPlan {
        BuildPlan {
            directory: self.0.clone(),
            system: BuildSystem::Make,
            steps: vec![ProcessSpec {
                program: program.into(),
                args: args.iter().map(|a| (*a).into()).collect(),
            }],
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn finish(task: &ProcessTask) -> (Vec<String>, Option<i32>) {
    let start = Instant::now();
    let mut output = Vec::new();
    loop {
        for event in task.drain() {
            match event {
                ProcessEvent::Output(text, _) => output.push(text),
                ProcessEvent::Finished(code, _) => return (output, code),
                _ => {}
            }
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "task never finished"
        );
        thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn configuration_preserves_unknown_project_fields_and_layers_local_settings() {
    let f = Fixture::new();
    let original = r#"{"version":1,"future":{"keep":true},"native":{"version":1,"theme":"dark","targets":[{"name":"My Pico","chip":"RP2040","compiler_target":"thumbv6m-none-eabi"}]}}"#;
    f.write("embedide.json", original);
    f.write(
        "embedide.local.json",
        r#"{"theme":"light","serial":{"baud":9600},"tools":{"cargo":"/path with spaces/cargo"}}"#,
    );
    let config = NativeConfig::load(&f.0).unwrap();
    assert_eq!(config.theme, "light");
    assert_eq!(config.serial.baud, 9600);
    assert_eq!(config.targets[0].chip.as_deref(), Some("RP2040"));
    assert_eq!(
        fs::read_to_string(f.0.join("embedide.json")).unwrap(),
        original
    );
    f.write("embedide.local.json", r#"{"unknown":true}"#);
    assert!(NativeConfig::load(&f.0).is_err());
    f.write("embedide.local.json", r#"{"version":2}"#);
    assert!(NativeConfig::load(&f.0).is_err());
    f.write("embedide.local.json", r#"{"tab_width":0}"#);
    assert!(NativeConfig::load(&f.0).is_err());
}
#[test]
fn missing_configuration_uses_defaults_and_key_bindings_are_validated() {
    let f = Fixture::new();
    let config = NativeConfig::load(&f.0).unwrap();
    assert_eq!((config.theme.as_str(), config.tab_width), ("mono", 4));
    assert!(!config.mouse && config.keys.is_empty());
    f.write(
        "embedide.local.json",
        r#"{"mouse":true,"keys":{"hide_terminal":"alt+6","build":"F12","palette":"ctrl+alt+7","quit":"Ctrl+Q"}}"#,
    );
    let config = NativeConfig::load(&f.0).unwrap();
    assert!(config.mouse);
    assert_eq!(config.keys["build"], "F12");
    for bad in [
        r#"{"keys":{"build":"f13"}}"#,
        r#"{"keys":{"build":"ctrl+1"}}"#,
        r#"{"keys":{"build":"alt+x"}}"#,
        r#"{"keys":{"build":"shift+f1"}}"#,
        r#"{"keys":{"build":"f5","check":"F5"}}"#,
        r#"{"mouse":"yes"}"#,
    ] {
        f.write("embedide.local.json", bad);
        let error = NativeConfig::load(&f.0).err().map(|e| e.to_string());
        assert!(error.is_some(), "{bad} was accepted");
    }
    f.write(
        "embedide.local.json",
        r#"{"keys":{"build":"f5","check":"F5"}}"#,
    );
    let error = NativeConfig::load(&f.0).err().unwrap().to_string();
    assert!(error.contains("unique"), "{error}");
}
#[test]
fn indentation_and_completion_settings_have_defaults_and_are_validated() {
    let f = Fixture::new();
    let config = NativeConfig::load(&f.0).unwrap();
    assert!(config.indent.auto && config.indent.width == 4 && config.indent.style == "detect");
    assert!(config.completion.auto && config.completion.lsp && config.completion.min_chars == 2);
    f.write(
        "embedide.local.json",
        r#"{"indent":{"width":2,"style":"tabs"},"completion":{"min_chars":1,"servers":{"zig":{"command":""}}},"keys":{"suggest":"ctrl+space"}}"#,
    );
    let config = NativeConfig::load(&f.0).unwrap();
    assert_eq!(
        (config.indent.width, config.indent.style.as_str()),
        (2, "tabs")
    );
    assert_eq!(config.completion.servers["zig"].command, "");
    for bad in [
        r#"{"indent":{"width":0}}"#,
        r#"{"indent":{"style":"smart"}}"#,
        r#"{"indent":{"tabs":true}}"#,
        r#"{"completion":{"min_chars":9}}"#,
        r#"{"completion":{"servers":{"python":{"command":"pylsp"}}}}"#,
        r#"{"completion":{"servers":{"rust":{"cmd":"ra"}}}}"#,
        r#"{"asm":{"syntax":"masm"}}"#,
    ] {
        f.write("embedide.local.json", bad);
        assert!(NativeConfig::load(&f.0).is_err(), "{bad} was accepted");
    }
}
#[test]
fn cargo_and_cmake_profiles_keep_arguments_separate_and_do_not_execute() {
    let f = Fixture::new();
    f.write("Cargo.toml", "[package]\nname='test'\nversion='0.1.0'\n");
    let mut config = NativeConfig::default();
    config.targets.push(Target {
        name: "Board".into(),
        compiler_target: Some("thumbv6m-none-eabi".into()),
        ..Target::default()
    });
    config
        .profiles
        .get_mut("release")
        .unwrap()
        .features
        .push("usb".into());
    config.tools.insert("cargo".into(), "/a path/cargo".into());
    let plan = config
        .build_plan(&Project::open(&f.0).unwrap(), "release", Some(0), true)
        .unwrap();
    assert_eq!(plan.steps[0].program, "/a path/cargo");
    assert_eq!(
        plan.steps[0].args,
        [
            "check",
            "--jobs",
            "2",
            "--offline",
            "--message-format=json",
            "--profile",
            "release",
            "--features",
            "usb",
            "--target",
            "thumbv6m-none-eabi"
        ]
    );
    fs::remove_file(f.0.join("Cargo.toml")).unwrap();
    f.write("CMakeLists.txt", "project(Test)");
    config.build_directory = "build with spaces".into();
    config.cmake_generator = Some("Ninja".into());
    config.cmake_toolchain = Some("board file.cmake".into());
    let plan = config
        .build_plan(&Project::open(&f.0).unwrap(), "release", None, false)
        .unwrap();
    assert_eq!(plan.steps.len(), 2);
    assert!(plan.steps[0]
        .args
        .contains(&"-DCMAKE_TOOLCHAIN_FILE=board file.cmake".into()));
    assert_eq!(plan.steps[1].args[1], "build with spaces");
    assert!(!f.0.join("build with spaces").exists());
}
#[test]
fn diagnostics_accept_cargo_and_windows_paths_but_reject_ambiguous_locations() {
    let gcc = diagnostics::parse("C:\\space dir\\main.c:12:9: warning: unused variable").unwrap();
    assert_eq!(gcc.line, 12);
    assert_eq!(gcc.column, 9);
    assert_eq!(gcc.file.unwrap(), PathBuf::from("C:\\space dir\\main.c"));
    let rust = diagnostics::parse(r#"{"reason":"compiler-message","message":{"level":"error","message":"bad type","code":{"code":"E0308"},"spans":[{"is_primary":true,"file_name":"src/main.rs","line_start":3,"column_start":7}]}}"#).unwrap();
    assert_eq!(rust.code.as_deref(), Some("E0308"));
    assert_eq!(rust.line, 3);
    assert!(diagnostics::parse("something error: oops").is_none());
    assert!(diagnostics::parse("CMake Error at CMakeLists.txt:3 (message):").is_some());
}
#[test]
fn assembler_diagnostics_without_columns_point_at_the_line() {
    // Exact output of NASM 3.02 and GNU as 2.47 for a bad instruction.
    let nasm =
        diagnostics::parse("/tmp/x y/a.asm:4: error: instruction expected, found `movv eax'")
            .unwrap();
    assert_eq!(
        (nasm.file.unwrap(), nasm.line, nasm.column),
        (PathBuf::from("/tmp/x y/a.asm"), 4, 1)
    );
    assert_eq!(nasm.severity, "error");
    let gas = diagnostics::parse("b.s:5: Error: no such instruction: `movv %rdi,%rdi'").unwrap();
    assert_eq!((gas.file.unwrap(), gas.line), (PathBuf::from("b.s"), 5));
    let warning = diagnostics::parse("b.s:7: Warning: end of file not at end of a line").unwrap();
    assert_eq!(warning.severity, "warning");
    let mc = diagnostics::parse("t.s:3:9: error: invalid operand for instruction").unwrap();
    assert_eq!((mc.line, mc.column), (3, 9));
    assert!(diagnostics::parse("b.s: Assembler messages:").is_none());
}
#[test]
fn flash_requires_unambiguous_hardware_and_matching_elf_architecture() {
    let f = Fixture::new();
    let mut elf = [0; 64];
    elf[..4].copy_from_slice(b"\x7fELF");
    elf[5] = 1;
    elf[18] = 40;
    fs::write(f.0.join("firm ware.elf"), elf).unwrap();
    let mut target = Target {
        name: "Custom".into(),
        compiler_target: Some("thumbv6m-none-eabi".into()),
        chip: Some("RP2040".into()),
        probe: None,
        artifact: Some("firm ware.elf".into()),
    };
    assert!(hardware::flash_plan(&f.0, &target, "probe-rs").is_err());
    target.probe = Some("1234:5678".into());
    assert!(hardware::flash_plan(&f.0, &target, "probe-rs").is_err());
    target.probe = Some("1234:5678:UNIQUE".into());
    let plan = hardware::flash_plan(&f.0, &target, "probe-rs").unwrap();
    assert!(plan.steps[0].args.contains(&"--verify".into()));
    assert!(!plan.steps[0].args.iter().any(|s| s.contains("erase")));
    assert!(plan.steps[0]
        .args
        .last()
        .unwrap()
        .ends_with("firm ware.elf"));
    target.compiler_target = Some("riscv32imac-unknown-none-elf".into());
    assert!(hardware::flash_plan(&f.0, &target, "probe-rs").is_err());
}
#[test]
fn serial_framing_validation_rejects_invalid_values() {
    let mut config = SerialConfig::default();
    assert!(config.validate().is_err());
    config.port = "explicit-device".into();
    assert!(config.validate().is_ok());
    config.data_bits = 9;
    assert!(config.validate().is_err());
    config.data_bits = 8;
    config.parity = "guess".into();
    assert!(config.validate().is_err());
}
#[test]
fn mi_parser_handles_nested_stack_registers_escapes_and_invalid_records() {
    let record = parse_mi(r#"42^done,stack=[frame={level="0",fullname="/path space/main.c",line="12",func="m\"ain"},frame={level="1",func="boot"}]"#).unwrap();
    assert_eq!(record.token, Some(42));
    assert_eq!(record.data["stack"][0]["frame"]["line"], "12");
    assert_eq!(record.data["stack"][0]["frame"]["func"], "m\"ain");
    let stopped =
        parse_mi(r#"*stopped,reason="breakpoint-hit",frame={fullname="main.c",line="7"}"#).unwrap();
    assert_eq!(stopped.class, "stopped");
    let octal = parse_mi(r#"1^error,msg="a\n\143at""#).unwrap();
    assert_eq!(octal.data["msg"], "a\ncat");
    for invalid in [
        "(gdb)",
        "~\"console\"",
        "^done,x={",
        "^done,a=oops",
        "^done,stack=[frame={line=\"1\"}",
        "^done trailing",
    ] {
        assert!(parse_mi(invalid).is_none(), "{invalid}");
    }
}
#[test]
fn explorer_is_lazy_exclusive_and_rejects_escaping_paths() {
    let f = Fixture::new();
    fs::create_dir(f.0.join("target")).unwrap();
    fs::create_dir(f.0.join("src")).unwrap();
    f.write("memory.x", "RAM");
    assert_eq!(explorer::read_directory(&f.0, &f.0).unwrap().len(), 2);
    for invalid in ["../escape", "/outside", "", "src/../file"] {
        assert!(explorer::destination(&f.0, invalid).is_err());
    }
    let file = explorer::create(&f.0, "src/new.c", false).unwrap();
    assert!(explorer::create(&f.0, "src/new.c", false).is_err());
    explorer::rename(&f.0, &file, "src/renamed.c").unwrap();
    assert!(!file.exists());
    f.write("src/existing.c", "keep");
    assert!(explorer::rename(&f.0, &f.0.join("src/renamed.c"), "src/existing.c").is_err());
    assert!(explorer::delete(&f.0, &f.0.join("src")).is_err());
    assert_eq!(
        fs::read_to_string(f.0.join("src/existing.c")).unwrap(),
        "keep"
    );
}
#[cfg(unix)]
#[test]
fn explorer_never_traverses_symlink_parents() {
    let f = Fixture::new();
    let outside = Fixture::new();
    std::os::unix::fs::symlink(&outside.0, f.0.join("link")).unwrap();
    assert!(explorer::create(&f.0, "link/file", false).is_err());
    assert!(explorer::read_directory(&f.0, &f.0.join("link")).is_err());
    assert!(explorer::delete(&f.0, &f.0.join("link")).is_err());
}
#[test]
fn missing_executable_reports_failure_and_final_state() {
    let f = Fixture::new();
    let task = ProcessTask::start(
        f.plan("embedide-deliberately-nonexistent-tool-88384", &[]),
        BTreeMap::new(),
    )
    .unwrap();
    let (output, code) = finish(&task);
    assert!(code.is_none());
    assert!(output.concat().contains("Cannot start"));
}
#[cfg(unix)]
#[test]
fn process_streams_output_before_exit_and_stops_following_steps_on_failure() {
    let f = Fixture::new();
    let tool = f.executable("fake compiler 猫", "#!/bin/sh\nprintf 'src/main.c:3:2: warning: test\\n'\nsleep 0.2\nprintf 'failed\\n' >&2\nexit 7\n");
    let mut plan = f.plan(tool.to_str().unwrap(), &[]);
    plan.steps.push(ProcessSpec {
        program: "/bin/sh".into(),
        args: vec!["-c".into(), "touch must-not-run".into()],
    });
    let task = ProcessTask::start(plan, BTreeMap::new()).unwrap();
    let start = Instant::now();
    let mut streamed = false;
    while start.elapsed() < Duration::from_secs(2) && !streamed {
        for event in task.drain() {
            if let ProcessEvent::Output(text, _) = event {
                streamed |= text.contains("warning");
            }
        }
        thread::sleep(Duration::from_millis(5));
    }
    assert!(streamed);
    let (_, code) = finish(&task);
    assert_eq!(code, Some(7));
    assert!(!f.0.join("must-not-run").exists());
}
#[cfg(unix)]
#[test]
fn cancellation_kills_descendants_and_retains_completion_during_output_overflow() {
    let f = Fixture::new();
    let tool = f.executable("noisy compiler", "#!/bin/sh\n(sleep 30) &\nprintf '%s' $! > descendant.pid\nwhile :; do printf '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\\n'; done\n");
    let task = ProcessTask::start(f.plan(tool.to_str().unwrap(), &[]), BTreeMap::new()).unwrap();
    let start = Instant::now();
    while (!f.0.join("descendant.pid").exists() || task.dropped_bytes() == 0)
        && start.elapsed() < Duration::from_secs(4)
    {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(task.dropped_bytes() > 0);
    task.cancel();
    let (_, code) = finish(&task);
    assert!(code.is_none());
    assert!(start.elapsed() < Duration::from_secs(5));
}
#[cfg(unix)]
#[test]
fn debugger_initializes_mi_and_reports_unexpected_termination() {
    use embedide_core::{
        config::DebugConfig,
        debug::{DebugCommand, DebugEvent, DebugSession},
    };
    let f = Fixture::new();
    f.write("firmware.elf", "symbols fixture");
    let gdb = f.executable(
        "fake gdb",
        r##"#!/usr/bin/env python3
import re, sys
for line in sys.stdin:
    token, command = re.match(r'(\d+)(.*)', line.strip()).groups()
    if command == '-data-list-register-values x':
        print(token + '^done,register-values=[{number="0",value="0x1234"}]', flush=True)
        sys.exit(19)
    print(token + ('^connected' if command.startswith('-target-select') else '^done'), flush=True)
"##,
    );
    let config = DebugConfig {
        gdb: gdb.to_string_lossy().into_owned(),
        elf: "firmware.elf".into(),
        ..DebugConfig::default()
    };
    let session = DebugSession::start(&f.0, &config).unwrap();
    let start = Instant::now();
    let mut connected = false;
    let mut failed = false;
    while start.elapsed() < Duration::from_secs(5) && !failed {
        for event in session.drain() {
            match event {
                DebugEvent::Connected => {
                    connected = true;
                    session.command(DebugCommand::Registers).unwrap();
                }
                DebugEvent::Error(error) => {
                    assert!(error.contains("unexpectedly"));
                    failed = true;
                }
                _ => {}
            }
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(connected && failed);
}

#[cfg(unix)]
#[test]
fn serial_monitor_exchanges_real_bytes_through_a_pseudoterminal() {
    use embedide_core::hardware::{SerialEvent, SerialMonitor};
    use std::{
        io::{BufRead, BufReader, Write},
        process::{Command, Stdio},
    };
    struct ChildGuard(std::process::Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut peer = ChildGuard(
        Command::new("python3")
            .args([
                "-u",
                "-c",
                r#"
import os, pty, select, sys, time, tty
master, slave = pty.openpty()
tty.setraw(slave)
print(os.ttyname(slave), flush=True)
ready, _, _ = select.select([sys.stdin], [], [], 5)
if not ready: sys.exit(4)
sys.stdin.readline()
os.write(master, b'boot\xff\xe7\x8c\xab\n')
ready, _, _ = select.select([master], [], [], 5)
if not ready: sys.exit(5)
print(os.read(master, 100).hex(), flush=True)
time.sleep(1)
"#,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut output = BufReader::new(peer.0.stdout.take().unwrap());
    let mut port = String::new();
    output.read_line(&mut port).unwrap();
    let config = SerialConfig {
        port: port.trim().into(),
        receive_only: false,
        ..SerialConfig::default()
    };
    let monitor = SerialMonitor::connect(&config).unwrap();
    peer.0
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"ready\n")
        .unwrap();
    let start = Instant::now();
    let mut received = Vec::new();
    while !received.ends_with(b"\n") && start.elapsed() < Duration::from_secs(3) {
        for event in monitor.drain() {
            if let SerialEvent::Data(bytes) = event {
                received.extend(bytes);
            }
        }
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(received, b"boot\xff\xe7\x8c\xab\n");
    monitor.send("hello").unwrap();
    let mut outgoing = String::new();
    output.read_line(&mut outgoing).unwrap();
    assert_eq!(outgoing.trim(), "68656c6c6f0a");
    drop(monitor);
    let config = SerialConfig {
        port: config.port,
        receive_only: true,
        ..SerialConfig::default()
    };
    let monitor = SerialMonitor::connect(&config).unwrap();
    assert!(monitor.send("not allowed").is_err());
}

#[cfg(unix)]
#[test]
fn serial_open_failures_explain_permissions_and_missing_devices() {
    use embedide_core::hardware::SerialMonitor;
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("embed-serial-perm-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let missing = SerialConfig {
        port: dir.join("ttyNOPE").display().to_string(),
        ..SerialConfig::default()
    };
    let error = SerialMonitor::connect(&missing).err().unwrap().to_string();
    assert!(error.contains("no such device"), "{error}");
    // A device node the user may not open (skipped as root, who bypasses modes).
    let locked = dir.join("ttyLOCKED");
    std::fs::write(&locked, "").unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::File::open(&locked).is_err() {
        let config = SerialConfig {
            port: locked.display().to_string(),
            ..SerialConfig::default()
        };
        let error = SerialMonitor::connect(&config).err().unwrap().to_string();
        assert!(
            error.contains("permission denied") && error.contains("ls -l"),
            "{error}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
