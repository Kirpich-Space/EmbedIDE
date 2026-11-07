//! Integration checks for the actual headless executable and shared services.
use embedide_core::{self as core, BuildOptions, BuildSystem, Project};
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "embedide-test-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, contents: &str) {
        fs::write(self.0.join(name), contents).unwrap();
    }
    fn cli(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_embed"))
            .args(args)
            .current_dir(&self.0)
            .output()
            .unwrap()
    }
    fn edit(&self, name: &str, keys: &str) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_embed"))
            .args(["--json", "edit", "--batch", "--", name])
            .current_dir(&self.0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(keys.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn json_output(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&output.stdout)))
}
fn options(system: Option<BuildSystem>) -> BuildOptions {
    BuildOptions {
        system,
        release: false,
        jobs: 2,
        online: false,
    }
}

#[test]
fn init_refuses_overwrite_and_preserves_sources() {
    let f = Fixture::new();
    f.write("main.c", "/* user content */");
    let first = f.cli(&["init", "--json"]);
    assert!(first.status.success());
    assert_eq!(json_output(&first)["ok"], true);
    let original = fs::read(f.0.join("embedide.json")).unwrap();
    let second = f.cli(&["--json", "init"]);
    assert_eq!(second.status.code(), Some(1));
    assert_eq!(json_output(&second)["ok"], false);
    assert_eq!(fs::read(f.0.join("embedide.json")).unwrap(), original);
    assert_eq!(
        fs::read_to_string(f.0.join("main.c")).unwrap(),
        "/* user content */"
    );
}

#[test]
fn legacy_metadata_and_unknown_fields_are_preserved() {
    let f = Fixture::new();
    let metadata =
        r#"{"version":1,"name":"legacy","type":"c","boardId":"stm32f407vg","custom":{"a":3}}"#;
    f.write("embedide.json", metadata);
    f.write("Makefile", "all:\n\t@echo built\n");
    let out = f.cli(&["open", ".", "--json"]);
    assert!(out.status.success());
    assert_eq!(
        json_output(&out)["data"]["metadata"],
        serde_json::from_str::<Value>(metadata).unwrap()
    );
    assert_eq!(
        fs::read_to_string(f.0.join("embedide.json")).unwrap(),
        metadata
    );
    assert_eq!(
        Project::open(&f.0).unwrap().build_system(None).unwrap(),
        BuildSystem::Make
    );
}

#[test]
fn bad_metadata_is_reported_without_modification() {
    for invalid in [
        "{bad",
        "[]",
        r#"{"version":2}"#,
        r#"{"buildSystem":false}"#,
        r#"{"buildSystem":"shell"}"#,
    ] {
        let f = Fixture::new();
        f.write("embedide.json", invalid);
        f.write("Cargo.toml", "");
        let out = f.cli(&["--json", "open"]);
        assert_eq!(out.status.code(), Some(1), "{invalid}");
        assert_eq!(json_output(&out)["error"]["kind"], "operation");
        assert_eq!(
            fs::read_to_string(f.0.join("embedide.json")).unwrap(),
            invalid
        );
    }
}

#[test]
fn ambiguous_manifests_require_explicit_choice() {
    let f = Fixture::new();
    f.write("Cargo.toml", "");
    f.write("Makefile", "");
    let project = Project::open(&f.0).unwrap();
    assert!(project.build_system(None).is_err());
    assert_eq!(
        project.build_system(Some(BuildSystem::Cargo)).unwrap(),
        BuildSystem::Cargo
    );
    f.write("embedide.json", r#"{"version":1,"buildSystem":"make"}"#);
    assert_eq!(
        Project::open(&f.0).unwrap().build_system(None).unwrap(),
        BuildSystem::Make
    );
}

#[test]
fn dry_run_never_executes_build_scripts_and_preserves_argument_boundaries() {
    let f = Fixture::new();
    f.write("Makefile", "all:\n\t@touch SHOULD_NOT_EXIST\n");
    let out = f.cli(&["build", "--dry-run", "--json"]);
    assert!(out.status.success());
    let value = json_output(&out);
    assert_eq!(value["data"]["executed"], false);
    assert_eq!(
        value["data"]["plan"]["steps"][0]["args"],
        json!(["-j", "2"])
    );
    assert!(!f.0.join("SHOULD_NOT_EXIST").exists());
    let odd_path = f.0.join("space ; $() Unicode-тест");
    fs::create_dir(&odd_path).unwrap();
    fs::write(odd_path.join("Cargo.toml"), "").unwrap();
    let plan = core::plan_build(&Project::open(&odd_path).unwrap(), &options(None)).unwrap();
    assert_eq!(plan.directory, odd_path.canonicalize().unwrap());
    assert_eq!(plan.steps[0].args, ["build", "--jobs", "2", "--offline"]);
}

#[test]
fn adapters_validate_options_and_build_order() {
    for (manifest, system, program, steps) in [
        ("Cargo.toml", BuildSystem::Cargo, "cargo", 1),
        ("GNUmakefile", BuildSystem::Make, "make", 1),
        ("CMakeLists.txt", BuildSystem::Cmake, "cmake", 2),
        ("build.zig", BuildSystem::Zig, "zig", 1),
    ] {
        let f = Fixture::new();
        f.write(manifest, "");
        let project = Project::open(&f.0).unwrap();
        let plan = core::plan_build(&project, &options(None)).unwrap();
        assert_eq!(plan.system, system);
        assert_eq!(plan.steps.len(), steps);
        assert_eq!(plan.steps[0].program, program);
        let mut invalid = options(None);
        invalid.jobs = 0;
        assert!(core::plan_build(&project, &invalid).is_err());
        assert!(project
            .build_system(Some(if system == BuildSystem::Make {
                BuildSystem::Cargo
            } else {
                BuildSystem::Make
            }))
            .is_err());
    }
}

#[test]
fn missing_projects_and_bad_cli_arguments_have_distinct_exit_codes() {
    let f = Fixture::new();
    let missing = f.cli(&["--json", "open"]);
    assert_eq!(missing.status.code(), Some(1));
    assert_eq!(json_output(&missing)["error"]["kind"], "operation");
    for args in [
        vec!["--json", "build", "--jobs", "0"],
        vec!["--json", "flash"],
        vec!["--json", "init", "--build-system", "shell"],
    ] {
        let out = f.cli(&args);
        assert_eq!(out.status.code(), Some(2));
        assert_eq!(json_output(&out)["error"]["kind"], "usage");
    }
}

#[test]
fn inventories_work_without_project_or_display() {
    let f = Fixture::new();
    for command in ["doctor", "toolchains", "targets"] {
        let out = Command::new(env!("CARGO_BIN_EXE_embed"))
            .args([command, "--json"])
            .current_dir(&f.0)
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .output()
            .unwrap();
        assert!(out.status.success());
        assert_eq!(json_output(&out)["ok"], true);
    }
}

#[test]
fn runner_propagates_exit_code_and_stops_after_failure() {
    let f = Fixture::new();
    let plan = core::BuildPlan {
        directory: f.0.clone(),
        system: BuildSystem::Make,
        steps: vec![
            core::ProcessSpec {
                program: env!("CARGO_BIN_EXE_embed").into(),
                args: vec!["unknown-command".into()],
            },
            core::ProcessSpec {
                program: env!("CARGO_BIN_EXE_embed").into(),
                args: vec!["init".into()],
            },
        ],
    };
    assert_eq!(core::run_build(&plan).unwrap(), 2);
    assert!(!f.0.join("embedide.json").exists());
    let missing = core::BuildPlan {
        steps: vec![core::ProcessSpec {
            program: "embedide-nonexistent-test-executable".into(),
            args: vec![],
        }],
        ..plan
    };
    assert!(core::run_build(&missing).is_err());
}

#[test]
fn edit_batch_uses_shared_engine_saves_and_reopens_unicode_file() {
    let f = Fixture::new();
    f.write("space ; 猫.c", "old value\r\n");
    let output = f.edit("space ; 猫.c", "ciwновое<Esc>\n:wq\n");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(json_output(&output)["data"]["closed"], true);
    assert_eq!(json_output(&output)["data"]["modified"], false);
    let path = f.0.join("space ; 猫.c");
    assert_eq!(
        core::Editor::open(&path)
            .unwrap()
            .state()
            .buffer()
            .contents(),
        "новое value\r\n"
    );
    assert!(f.edit("space ; 猫.c", "A!<Esc>\n:wq\n").status.success());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "новое value!\r\n");
}

#[test]
fn edit_batch_creation_dirty_quit_and_force_quit_have_correct_codes() {
    let f = Fixture::new();
    assert_eq!(
        f.edit("new.rs", "iunsaved<Esc>\n:q\n").status.code(),
        Some(1)
    );
    assert!(!f.0.join("new.rs").exists());
    let output = f.edit("new.rs", "iunsaved<Esc>\n:q!\n");
    assert!(output.status.success());
    assert_eq!(json_output(&output)["data"]["modified"], true);
    assert!(!f.0.join("new.rs").exists());
    assert!(f.edit("new.rs", "inew<Esc>\n:wq\n").status.success());
    assert_eq!(std::fs::read_to_string(f.0.join("new.rs")).unwrap(), "new");
}

#[test]
fn interactive_edit_rejects_json_without_batch_before_showing_a_prompt() {
    let f = Fixture::new();
    let output = f.cli(&["--json", "edit", "new.rs"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(json_output(&output)["error"]["kind"], "usage");
    assert!(!f.0.join("new.rs").exists());
}

#[test]
fn edit_line_and_batch_modes_have_explicit_conflicts_and_pipe_guards() {
    let f = Fixture::new();
    for args in [
        vec!["--json", "edit", "--line", "--batch", "new.rs"],
        vec!["--json", "edit", "--line", "new.rs"],
    ] {
        let output = f.cli(&args);
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(json_output(&output)["error"]["kind"], "usage");
    }
    let output = f.cli(&["edit", "--line", "new.rs"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("--batch"));
    assert!(!f.0.join("new.rs").exists());
}

#[test]
fn directory_shorthand_preserves_named_commands_when_matching_directories_exist() {
    let f = Fixture::new();
    fs::create_dir(f.0.join("toolchains")).unwrap();
    let out = f.cli(&["toolchains"]);
    assert!(out.status.success());
    assert!(json_output(&out)["data"]["tools"].is_array());
}

#[test]
fn directory_shorthand_inspects_project_without_terminal_or_execution() {
    let f = Fixture::new();
    f.write("Cargo.toml", "[package]\nname='fixture'\nversion='0.1.0'\n");
    let out = f.cli(&["."]);
    assert!(out.status.success());
    assert_eq!(
        json_output(&out)["data"]["detected_build_systems"],
        json!(["cargo"])
    );
}
