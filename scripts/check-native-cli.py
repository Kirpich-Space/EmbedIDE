#!/usr/bin/env python3
"""Black-box contracts for a supplied native binary; no Cargo or GUI required.

Runs only disposable, trusted fixtures. It never builds the user's project.
The executable's SHA-256 identifies which artifact was actually exercised;
passing these tests does not prove the current sources compile.
"""

import argparse
import hashlib
import json
import os
import platform
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
BINARY = None


class NativeCliContracts(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="embedide-contract-")
        self.addCleanup(self.temporary.cleanup)
        self.project = Path(self.temporary.name) / "space ; $() Unicode-тест"
        self.project.mkdir()
        self.environment = os.environ.copy()
        self.environment.pop("DISPLAY", None)
        self.environment.pop("WAYLAND_DISPLAY", None)

    def write(self, name, content):
        (self.project / name).write_text(content, encoding="utf-8")

    def cli(self, *args, code=0, environment=None):
        result = subprocess.run(
            [str(BINARY), "--json", *args],
            cwd=self.project,
            env=self.environment if environment is None else environment,
            capture_output=True,
            timeout=30,
            check=False,
        )
        self.assertEqual(result.returncode, code, result.stderr.decode(errors="replace"))
        value = json.loads(result.stdout)
        self.assertEqual(value["ok"], code == 0)
        return value, result

    def test_init_never_overwrites_metadata_or_sources(self):
        self.write("main.c", "/* preserved */")
        self.cli("init")
        original = (self.project / "embedide.json").read_bytes()
        self.cli("init", code=1)
        self.assertEqual((self.project / "embedide.json").read_bytes(), original)
        self.assertEqual((self.project / "main.c").read_text(), "/* preserved */")

    def test_open_preserves_unknown_legacy_fields(self):
        metadata = {"version": 1, "type": "c", "boardId": "stm32f407vg", "future": [42]}
        self.write("embedide.json", json.dumps(metadata))
        before = (self.project / "embedide.json").read_bytes()
        value, _ = self.cli("open", ".")
        self.assertEqual(value["data"]["metadata"], metadata)
        self.assertEqual((self.project / "embedide.json").read_bytes(), before)

    def test_ambiguous_build_requires_choice_without_executing(self):
        self.write("Cargo.toml", "")
        self.write("Makefile", "all:\n\t@touch SHOULD_NOT_EXIST\n")
        self.cli("build", "--dry-run", code=1)
        value, _ = self.cli("build", "--dry-run", "--build-system", "make")
        # GNU make is `gmake` on the BSDs.
        bsd = platform.system() in ("FreeBSD", "DragonFly", "NetBSD", "OpenBSD") and shutil.which("gmake")
        self.assertEqual(value["data"]["plan"]["steps"][0]["program"], "gmake" if bsd else "make")
        self.assertFalse((self.project / "SHOULD_NOT_EXIST").exists())

    def test_invalid_metadata_remains_unchanged(self):
        for invalid in ["{bad", "[]", '{"version":2}', '{"buildSystem":false}']:
            with self.subTest(metadata=invalid):
                self.write("embedide.json", invalid)
                value, _ = self.cli("open", code=1)
                self.assertEqual(value["error"]["kind"], "operation")
                self.assertEqual((self.project / "embedide.json").read_text(), invalid)

    def test_cargo_is_offline_by_default(self):
        self.write("Cargo.toml", "")
        value, _ = self.cli("build", "--dry-run")
        self.assertIn("--offline", value["data"]["plan"]["steps"][0]["args"])
        value, _ = self.cli("build", "--dry-run", "--online")
        self.assertNotIn("--offline", value["data"]["plan"]["steps"][0]["args"])

    def test_usage_errors_have_distinct_status(self):
        for args in [("build", "--jobs", "0"), ("unknown",), ("init", "--build-system", "shell")]:
            with self.subTest(args=args):
                value, _ = self.cli(*args, code=2)
                self.assertEqual(value["error"]["kind"], "usage")

    def test_inventory_and_catalog_need_no_project_or_display(self):
        for command in ["doctor", "toolchains", "targets"]:
            with self.subTest(command=command):
                self.cli(command)
        value, _ = self.cli("targets")
        self.assertFalse(value["data"]["hardware_verified"])
        self.assertEqual(len(value["data"]["targets"]), 67)

    def test_missing_build_tool_is_an_operation_error(self):
        self.write("Makefile", "all:\n\t@echo unused\n")
        environment = dict(self.environment, PATH="")
        value, _ = self.cli("build", code=1, environment=environment)
        self.assertEqual(value["error"]["kind"], "operation")

    @unittest.skipUnless(shutil.which("gcc") and shutil.which("make"), "GCC and Make required")
    def test_actual_native_c_build_and_json_stream_separation(self):
        self.write("main.c", '#include <stdio.h>\nint main(void) { puts("native-ok"); return 0; }\n')
        self.write("Makefile", "all:\n\t@echo BUILD_OUTPUT_SENTINEL\n\tgcc -Wall -Werror main.c -o firmware.exe\n")
        source = (self.project / "main.c").read_bytes()
        value, result = self.cli("build")
        self.assertTrue(value["data"]["executed"])
        self.assertEqual(value["data"]["exit_code"], 0)
        self.assertIn(b"BUILD_OUTPUT_SENTINEL", result.stderr)
        self.assertNotIn(b"BUILD_OUTPUT_SENTINEL", result.stdout)
        executable = subprocess.run(
            [str(self.project / "firmware.exe")], capture_output=True, timeout=10, check=True
        )
        self.assertEqual(executable.stdout.strip(), b"native-ok")
        self.assertEqual((self.project / "main.c").read_bytes(), source)

    @unittest.skipUnless(shutil.which("gcc") and shutil.which("make"), "GCC and Make required")
    def test_actual_compiler_failure_has_nonzero_build_status(self):
        self.write("main.c", "#error EXPECTED_COMPILER_FAILURE\n")
        self.write("Makefile", "all:\n\tgcc -c main.c -o main.o\n")
        value, result = self.cli("build", code=2)
        self.assertEqual(value["data"]["exit_code"], 2)
        self.assertIn(b"EXPECTED_COMPILER_FAILURE", result.stderr)
        self.assertFalse((self.project / "main.o").exists())


def main():
    global BINARY
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    options = parser.parse_args()
    BINARY = options.binary.resolve(strict=True)
    print(f"Artifact: {BINARY}", flush=True)
    print(f"SHA-256: {hashlib.sha256(BINARY.read_bytes()).hexdigest()}", flush=True)
    result = unittest.TextTestRunner(verbosity=2).run(
        unittest.defaultTestLoader.loadTestsFromTestCase(NativeCliContracts)
    )
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    raise SystemExit(main())
