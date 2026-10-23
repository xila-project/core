"""The evidence collector must tolerate other cached Cargo configurations."""

import tempfile
import unittest
from pathlib import Path

import verify


class VerifierTests(unittest.TestCase):
    def test_lvgl_evidence_uses_current_dependency_not_historical_outputs(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            current = root / "debug/build/lvgl_rust_sys-native/out"
            historical = root / "debug/build/lvgl_rust_sys-bindgen-only/out"
            current.mkdir(parents=True)
            historical.mkdir(parents=True)
            (current / "bindings.rs").write_text("current bindings")
            (current / "liblvgl.a").write_text("current archive")
            (historical / "bindings.rs").write_text("other configuration")
            messages = [{"reason": "build-script-executed",
                         "package_id": "git+https://github.com/Xila-Project/lvgl_rust_sys#lvgl_rust_sys@0.1.0",
                         "out_dir": str(current)}]
            self.assertEqual(set(verify.lvgl_outputs(messages)),
                             {current / "bindings.rs", current / "liblvgl.a"})


if __name__ == "__main__":
    unittest.main()
