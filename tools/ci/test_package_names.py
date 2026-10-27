"""Regression checks for Cargo package identity and internal dependency aliases."""

import pathlib
import json
import re
import subprocess
import tomllib
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[2]

EXPECTED_PACKAGE_NAMES = {
    "core/Cargo.toml": "xila",
    "drivers/core/Cargo.toml": "xila-drivers-core",
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
    "tools/verification_probe/Cargo.toml": "verification_probe",
}

EXPECTED_BINARY_TARGETS = {
}

EXPECTED_LIBRARY_TARGETS = {
    "libraries/application_metadata/Cargo.toml": "application_metadata",
    "subsystems/task/Cargo.toml": "task",
    "libraries/synchronization/Cargo.toml": "synchronization",
    "drivers/core/Cargo.toml": "drivers_core",
    "libraries/file_system/Cargo.toml": "file_system",
    "libraries/shared/Cargo.toml": "shared",
    "subsystems/log/Cargo.toml": "log",
    "subsystems/users/Cargo.toml": "users",
    "libraries/audio/Cargo.toml": "audio",
    "libraries/debug_protocol/Cargo.toml": "debug_protocol",
    "subsystems/graphics/Cargo.toml": "graphics",
    "libraries/abi/declarations/Cargo.toml": "abi_declarations",
    "subsystems/time/Cargo.toml": "time",
    "libraries/device/Cargo.toml": "device",
    "subsystems/network/Cargo.toml": "network",
    "subsystems/virtual_file_system/Cargo.toml": "virtual_file_system",
    "libraries/abi/definitions/Cargo.toml": "abi_definitions",
    "subsystems/memory/Cargo.toml": "memory",
    "libraries/little_fs/Cargo.toml": "little_fs",
    "libraries/authentication/Cargo.toml": "authentication",
    "libraries/executable/Cargo.toml": "executable",
    "libraries/peripherals/Cargo.toml": "peripherals",
    "libraries/bootsplash/Cargo.toml": "bootsplash",
    "subsystems/graphics/bindings_generator/Cargo.toml": "graphics_bindgen",
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
            and ".application-src" not in path.parts
            and "node_modules" not in path.parts
            and "package" in manifest(path)
        }

        self.assertEqual(actual, EXPECTED_PACKAGE_NAMES)
        self.assertEqual(len(set(actual.values())), len(EXPECTED_PACKAGE_NAMES))
        self.assertEqual(actual["core/Cargo.toml"], "xila")
        for path, name in actual.items():
            # These crates were added on main after the package migration.
            if path not in {
                "core/Cargo.toml",
                "libraries/http/Cargo.toml",
                "tools/verification_probe/Cargo.toml",
            }:
                self.assertTrue(name.startswith("xila-"), f"{path} has unprefixed name {name}")

    def test_virtual_workspace_defaults_to_xila_facade(self):
        root = manifest(ROOT / "Cargo.toml")
        self.assertNotIn("package", root)
        metadata = json.loads(subprocess.check_output(
            ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"],
            cwd=ROOT, text=True,
        ))
        packages = [p for p in metadata["packages"] if p["name"] == "xila"]
        self.assertEqual(len(packages), 1)
        self.assertEqual(pathlib.Path(packages[0]["manifest_path"]), ROOT / "core/Cargo.toml")
        self.assertEqual(metadata["workspace_default_members"], [packages[0]["id"]])

    def test_facade_and_workspace_libraries_share_version(self):
        root = manifest(ROOT / "Cargo.toml")
        self.assertIn("version", root["workspace"]["package"])
        metadata = json.loads(subprocess.check_output(
            ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"],
            cwd=ROOT, text=True,
        ))
        for package in metadata["packages"]:
            path = pathlib.Path(package["manifest_path"]).relative_to(ROOT)
            if path.parts[0] in {"core", "libraries", "subsystems"}:
                self.assertEqual(manifest(ROOT / path)["package"]["version"], {"workspace": True}, str(path))

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
        allowed_names = set(EXPECTED_PACKAGE_NAMES.values()) | {"xila-wasm"}  # Applications workspace selector
        selector = re.compile(r"(?:--package(?:=|\s+)|-p\s+)([A-Za-z0-9_-]+)")
        text_suffixes = {".md", ".toml", ".yml", ".yaml", ".sh", ".py", ".rs"}

        for path in ROOT.rglob("*"):
            if ".application-src" in path.parts or "node_modules" in path.parts:
                continue
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

    def test_core_production_graph_does_not_depend_on_other_repositories(self):
        tree = subprocess.check_output(
            ["cargo", "tree", "--locked", "-p", "xila", "-e", "normal"], cwd=ROOT, text=True
        )
        self.assertNotIn("xila-project/applications", tree.lower())
        self.assertNotIn("xila-project/platforms", tree.lower())

    def test_xila_packages_have_one_resolved_identity(self):
        metadata = json.loads(subprocess.check_output(
            ["cargo", "metadata", "--locked", "--format-version", "1"], cwd=ROOT, text=True
        ))
        ids = {}
        for package in metadata["packages"]:
            name = package["name"]
            if name == "xila" or name.startswith("xila-"):
                ids.setdefault(name, set()).add(package["id"])
        self.assertFalse({name: entries for name, entries in ids.items() if len(entries) != 1})

if __name__ == "__main__":
    unittest.main()
