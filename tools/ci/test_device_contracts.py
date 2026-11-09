"""Guards for the VFS device class contract crate (`devices/`, package `xila-device`)."""

import pathlib
import re
import tomllib
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[2]
KIND = ROOT / "devices/src/kind.rs"
# `device` depends on `file_system`, so its generic commands cannot use the registry.
LITERAL_KIND_ALLOWED = {"libraries/file_system"}
DEFINE_COMMAND = re.compile(r"define_command!\s*\(\s*\w+\s*,\s*(?:Read|Write)\s*,\s*([^,]+?)\s*,", re.S)
SKIPPED_PARTS = {"target", "node_modules", "tests", "docs", ".superpowers"}


def sources():
    for path in ROOT.rglob("*.rs"):
        relative = path.relative_to(ROOT)
        if SKIPPED_PARTS.intersection(relative.parts) or relative.parts[0] == "tools":
            continue
        yield relative, path.read_text(encoding="utf-8")


class DeviceContractTests(unittest.TestCase):
    def test_device_crate_stays_dependency_light(self):
        manifest = tomllib.loads((ROOT / "devices/Cargo.toml").read_text(encoding="utf-8"))
        dependencies = set(manifest.get("dependencies", {}))
        self.assertLessEqual(
            dependencies,
            {"file_system", "shared"},
            "xila-device holds contracts only; runtime dependencies create cycles",
        )

    def test_every_kind_constant_is_registered(self):
        text = KIND.read_text(encoding="utf-8")
        constants = set(re.findall(r"pub const (\w+): u8 = b'.';", text))
        listed = set(re.findall(r"\b([A-Z][A-Z0-9_]+)\b", text.split("pub const ALL", 1)[1].split("];", 1)[0]))
        self.assertTrue(constants)
        self.assertEqual(constants, listed, "every kind constant must appear in device::kind::ALL")

    def test_control_commands_use_the_kind_registry(self):
        offenders = []
        for relative, text in sources():
            if any(relative.as_posix().startswith(prefix) for prefix in LITERAL_KIND_ALLOWED):
                continue
            for match in DEFINE_COMMAND.finditer(text):
                kind = match.group(1)
                if "macro_rules" in text[max(0, match.start() - 200):match.start()] or kind.startswith("$"):
                    continue
                if "kind::" not in kind:
                    offenders.append(f"{relative}: {kind}")
        self.assertEqual(offenders, [], "use a device::kind constant instead of a literal kind byte")


if __name__ == "__main__":
    unittest.main()
