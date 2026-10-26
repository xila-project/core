"""Regression checks for Cargo package identity and internal dependency aliases."""

import pathlib
import json
import re
import subprocess
import tomllib
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[2]

EXPECTED_PACKAGE_NAMES = {
    "Cargo.toml": "xila",
    "drivers/core/Cargo.toml": "xila-drivers-core",
    "drivers/espressif/Cargo.toml": "xila-drivers-espressif",
    "drivers/headless/Cargo.toml": "xila-drivers-headless",
    "drivers/native/Cargo.toml": "xila-drivers-native",
    "drivers/shared/Cargo.toml": "xila-drivers-shared",
    "drivers/std/Cargo.toml": "xila-drivers-std",
    "drivers/wasm/Cargo.toml": "xila-drivers-wasm",
    "examples/native/Cargo.toml": "xila-native-example",
    "examples/wasm/Cargo.toml": "xila-wasm-example",
    "executables/activity/Cargo.toml": "xila-activity",
    "executables/calculator/Cargo.toml": "xila-calculator",
    "executables/clock/Cargo.toml": "xila-clock",
    "executables/debug/Cargo.toml": "xila-debug-application",
    "executables/file_manager/Cargo.toml": "xila-file-manager",
    "executables/pixel_painter/Cargo.toml": "xila-pixel-painter",
    "executables/settings/Cargo.toml": "xila-settings",
    "executables/shell/command_line/Cargo.toml": "xila-command-line-shell",
    "executables/shell/graphical/Cargo.toml": "xila-graphical-shell",
    "executables/synth/Cargo.toml": "xila-synth",
    "executables/terminal/Cargo.toml": "xila-terminal",
    "executables/text_editor/Cargo.toml": "xila-text-editor",
    "executables/wasm/Cargo.toml": "xila-wasm",
    "executables/wasm/tests/translation/Cargo.toml": "xila-wasm-translation-tests",
    "executables/wasm/tests/wasm_test/Cargo.toml": "xila-wasm-wasm-test",
    "executables/weather/Cargo.toml": "xila-weather",
    "libraries/abi/declarations/Cargo.toml": "xila-abi-declarations",
    "libraries/abi/definitions/Cargo.toml": "xila-abi-definitions",
    "libraries/application_metadata/Cargo.toml": "xila-application-metadata",
    "libraries/audio/Cargo.toml": "xila-audio",
    "libraries/authentication/Cargo.toml": "xila-authentication",
    "libraries/bootsplash/Cargo.toml": "xila-bootsplash",
    "libraries/debug_protocol/Cargo.toml": "xila-debug-protocol",
    "libraries/device/Cargo.toml": "xila-device",
    "libraries/executable/Cargo.toml": "xila-executable",
    "libraries/file_system/Cargo.toml": "xila-file-system",
    "libraries/firmware_update/Cargo.toml": "xila-firmware-update",
    "libraries/http/Cargo.toml": "xila_http",
    "libraries/little_fs/Cargo.toml": "xila-little-fs",
    "libraries/peripherals/Cargo.toml": "xila-peripherals",
    "libraries/shared/Cargo.toml": "xila-shared",
    "libraries/synchronization/Cargo.toml": "xila-synchronization",
    "libraries/testing/Cargo.toml": "xila-testing",
    "subsystems/graphics/Cargo.toml": "xila-graphics",
    "subsystems/graphics/bindings_generator/Cargo.toml": "xila-graphics-bindgen",
    "subsystems/graphics/tests/font_link/Cargo.toml": "xila-graphics-font-link-test",
    "subsystems/graphics/tests/font_pipeline/Cargo.toml": "xila-graphics-font-pipeline-test",
    "subsystems/log/Cargo.toml": "xila-log",
    "subsystems/memory/Cargo.toml": "xila-memory",
    "subsystems/network/Cargo.toml": "xila-network",
    "subsystems/task/Cargo.toml": "xila-task",
    "subsystems/task/task_macros/Cargo.toml": "xila-task-macros",
    "subsystems/time/Cargo.toml": "xila-time",
    "subsystems/users/Cargo.toml": "xila-users",
    "subsystems/virtual_file_system/Cargo.toml": "xila-virtual-file-system",
    "tools/fs_image/Cargo.toml": "xila-fs-image",
    "tools/profiling/Cargo.toml": "xila-profiling",
    "tools/verification_probe/Cargo.toml": "verification_probe",
}

EXPECTED_BINARY_TARGETS = {
    "libraries/firmware_update/Cargo.toml": ["xila-firmware-sim"],
    "drivers/native/Cargo.toml": ["xila-debug"],
    "executables/wasm/tests/wasm_test/Cargo.toml": ["wasm_wasm_test"],
    "executables/pixel_painter/Cargo.toml": ["pixel_painter"],
    "executables/text_editor/Cargo.toml": ["text_editor"],
    "examples/native/Cargo.toml": ["native_example", "xila-runtime"],
    "executables/calculator/Cargo.toml": ["calculator"],
    "examples/wasm/Cargo.toml": ["wasm_example"],
    "executables/weather/Cargo.toml": ["weather"],
    "executables/clock/Cargo.toml": ["clock"],
    "executables/synth/Cargo.toml": ["synth"],
    "tools/fs_image/Cargo.toml": ["fs_image"],
}

EXPECTED_LIBRARY_TARGETS = {
    "executables/activity/Cargo.toml": "activity",
    "libraries/application_metadata/Cargo.toml": "application_metadata",
    "subsystems/task/Cargo.toml": "task",
    "libraries/synchronization/Cargo.toml": "synchronization",
    "drivers/core/Cargo.toml": "drivers_core",
    "libraries/file_system/Cargo.toml": "file_system",
    "libraries/shared/Cargo.toml": "shared",
    "subsystems/log/Cargo.toml": "log",
    "subsystems/users/Cargo.toml": "users",
    "drivers/native/Cargo.toml": "drivers_native",
    "libraries/audio/Cargo.toml": "audio",
    "libraries/debug_protocol/Cargo.toml": "debug_protocol",
    "subsystems/graphics/Cargo.toml": "graphics",
    "libraries/abi/declarations/Cargo.toml": "abi_declarations",
    "subsystems/time/Cargo.toml": "time",
    "drivers/shared/Cargo.toml": "drivers_shared",
    "libraries/device/Cargo.toml": "device",
    "subsystems/network/Cargo.toml": "network",
    "subsystems/virtual_file_system/Cargo.toml": "virtual_file_system",
    "libraries/abi/definitions/Cargo.toml": "abi_definitions",
    "subsystems/memory/Cargo.toml": "memory",
    "drivers/std/Cargo.toml": "drivers_std",
    "libraries/little_fs/Cargo.toml": "little_fs",
    "libraries/testing/Cargo.toml": "testing",
    "libraries/authentication/Cargo.toml": "authentication",
    "libraries/executable/Cargo.toml": "executable",
    "libraries/peripherals/Cargo.toml": "peripherals",
    "executables/file_manager/Cargo.toml": "file_manager",
    "libraries/bootsplash/Cargo.toml": "bootsplash",
    "executables/shell/command_line/Cargo.toml": "command_line_shell",
    "executables/wasm/Cargo.toml": "wasm",
    "subsystems/graphics/bindings_generator/Cargo.toml": "graphics_bindgen",
    "executables/shell/graphical/Cargo.toml": "graphical_shell",
    "executables/terminal/Cargo.toml": "terminal",
    "executables/settings/Cargo.toml": "settings",
    "executables/debug/Cargo.toml": "debug_application",
    "executables/pixel_painter/Cargo.toml": "pixel_painter",
    "drivers/headless/Cargo.toml": "drivers_headless",
    "drivers/wasm/Cargo.toml": "drivers_wasm",
    "drivers/espressif/Cargo.toml": "drivers_espressif",
    "executables/weather/Cargo.toml": "weather",
    "executables/clock/Cargo.toml": "clock",
    "executables/synth/Cargo.toml": "synth",
}


def manifest(path):
    return tomllib.loads(path.read_text())


class CargoPackageNames(unittest.TestCase):
    def test_all_repository_packages_have_the_expected_unique_names(self):
        actual = {
            path.relative_to(ROOT).as_posix(): manifest(path)["package"]["name"]
            for path in ROOT.rglob("Cargo.toml")
            if ".git" not in path.parts
            and not path.is_relative_to(ROOT / "target")
            and ".superpowers" not in path.parts
            and "package" in manifest(path)
        }

        self.assertEqual(actual, EXPECTED_PACKAGE_NAMES)
        self.assertEqual(len(set(actual.values())), len(EXPECTED_PACKAGE_NAMES))
        self.assertEqual(actual["Cargo.toml"], "xila")
        self.assertEqual(actual["tools/profiling/Cargo.toml"], "xila-profiling")
        for path, name in actual.items():
            # These crates were added on main after the package migration.
            if path not in {
                "Cargo.toml",
                "libraries/http/Cargo.toml",
                "tools/verification_probe/Cargo.toml",
            }:
                self.assertTrue(name.startswith("xila-"), f"{path} has unprefixed name {name}")

    def test_root_workspace_dependency_keys_alias_renamed_packages(self):
        workspace = manifest(ROOT / "Cargo.toml")["workspace"]["dependencies"]

        for key, dependency in workspace.items():
            if "path" not in dependency:
                continue
            target_manifest = ROOT / dependency["path"] / "Cargo.toml"
            target_name = manifest(target_manifest)["package"]["name"]
            self.assertEqual(dependency.get("package", key), target_name, key)

        self.assertEqual(workspace["task"].get("package", "task"), "xila-task")
        self.assertEqual(workspace["graphics"].get("package", "graphics"), "xila-graphics")
        self.assertEqual(workspace["log"].get("package", "log"), "xila-log")

    def test_internal_log_alias_does_not_rename_external_log_dependency(self):
        dependencies = manifest(ROOT / "subsystems/log/Cargo.toml")["dependencies"]

        self.assertEqual(dependencies["log_external"]["package"], "log")
        self.assertEqual(dependencies["log_external"]["version"], "0.4")
        self.assertNotIn("path", dependencies["log_external"])

    def test_binary_target_names_remain_stable_after_package_rename(self):
        metadata = json.loads(
            subprocess.check_output(
                ["cargo", "metadata", "--no-deps", "--format-version", "1"],
                cwd=ROOT,
                text=True,
            )
        )
        actual = {
            pathlib.Path(package["manifest_path"]).relative_to(ROOT).as_posix(): [
                target["name"]
                for target in package["targets"]
                if "bin" in target["kind"]
            ]
            for package in metadata["packages"]
            if any("bin" in target["kind"] for target in package["targets"])
        }

        self.assertEqual(actual, EXPECTED_BINARY_TARGETS)

        native_driver = next(
            package
            for package in metadata["packages"]
            if package["name"] == "xila-drivers-native"
        )
        library_names = [
            target["name"]
            for target in native_driver["targets"]
            if "lib" in target["kind"]
        ]
        self.assertEqual(library_names, ["drivers_native"])

    def test_library_target_names_remain_stable_after_package_rename(self):
        metadata = json.loads(
            subprocess.check_output(
                ["cargo", "metadata", "--no-deps", "--format-version", "1"],
                cwd=ROOT,
                text=True,
            )
        )
        actual = {
            pathlib.Path(package["manifest_path"]).relative_to(ROOT).as_posix(): [
                target["name"]
                for target in package["targets"]
                if "lib" in target["kind"]
            ]
            for package in metadata["packages"]
            if pathlib.Path(package["manifest_path"]).relative_to(ROOT).as_posix()
            in EXPECTED_LIBRARY_TARGETS
            if any("lib" in target["kind"] for target in package["targets"])
        }

        self.assertEqual(
            actual,
            {path: [name] for path, name in EXPECTED_LIBRARY_TARGETS.items()},
        )

    def test_cargo_package_selectors_use_current_package_identities(self):
        allowed_names = set(EXPECTED_PACKAGE_NAMES.values())
        selector = re.compile(r"(?:--package(?:=|\s+)|-p\s+)([A-Za-z0-9_-]+)")
        text_suffixes = {".md", ".toml", ".yml", ".yaml", ".sh", ".py", ".rs"}

        for path in ROOT.rglob("*"):
            if (
                not path.is_file()
                or (
                    path.suffix not in text_suffixes
                    and not path.name.startswith("Dockerfile")
                )
                or ".git" in path.parts
                or ".superpowers" in path.parts
                or path.is_relative_to(ROOT / "target")
            ):
                continue
            for line_number, line in enumerate(path.read_text(errors="ignore").splitlines(), 1):
                if "cargo" not in line:
                    continue
                for match in selector.finditer(line):
                    self.assertIn(
                        match.group(1),
                        allowed_names,
                        f"stale Cargo package selector {match.group(1)!r} at {path}:{line_number}",
                    )

    def test_ci_feature_selectors_use_selected_package_names(self):
        tasks = manifest(ROOT / "mise.ci.toml")["tasks"]
        for task, features in {
            "ci:wasm-audio-check": ["xila-drivers-wasm/audio"],
            "ci:profiling-runtime-tests": [
                "xila-task/test_harness",
                "xila-drivers-std/profiling",
                "xila-drivers-std/heaptrack",
            ],
        }.items():
            with self.subTest(task=task):
                run = tasks[task]["run"]
                self.assertIn("--features " + ",".join(features), run)

if __name__ == "__main__":
    unittest.main()
