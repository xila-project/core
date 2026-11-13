"""The evidence collector must tolerate other cached Cargo configurations."""

import tempfile
import unittest
from pathlib import Path

import verify


class VerifierTests(unittest.TestCase):
    def test_fallback_probe_restores_translation_metadata_after_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            manifest = Path(directory) / "Cargo.toml"
            source = '[package.metadata.i18n]\nfallback = ["en"]\nstrict = true\n'
            manifest.write_text(source)
            with self.assertRaises(RuntimeError):
                with verify.restore_manifest(manifest):
                    verify.set_fallback(manifest, "fr")
                    self.assertIn('fallback = ["fr"]', manifest.read_text())
                    raise RuntimeError("probe failed")
            self.assertEqual(manifest.read_text(), source)

    def test_lvgl_evidence_uses_current_dependency_not_historical_outputs(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            current = root / "debug/build/lvgl-rust-sys-native/out"
            historical = root / "debug/build/lvgl-rust-sys-bindgen-only/out"
            current.mkdir(parents=True)
            historical.mkdir(parents=True)
            (current / "bindings.rs").write_text("current bindings")
            (current / "liblvgl.a").write_text("current archive")
            (historical / "bindings.rs").write_text("other configuration")
            messages = [{"reason": "build-script-executed",
                         "package_id": "git+https://github.com/AlixANNERAUD/lvgl_rust?branch=feat%2Fxila-integration#lvgl-rust-sys@0.1.2+lvgl.9.6.0",
                         "out_dir": str(current)}]
            self.assertEqual(set(verify.lvgl_outputs(messages)),
                             {current / "bindings.rs", current / "liblvgl.a"})


if __name__ == "__main__":
    unittest.main()
