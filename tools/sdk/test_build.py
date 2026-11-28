"""build.py's picolibc setup guard: the setup inputs are stamped only after a successful
`meson setup`, so an interrupted setup is redone with --wipe instead of being reconfigured
with the stale flags of the original cross file."""

import os
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import build

TARGET = "x86_64-unknown-linux-gnu"
OTHER_TARGET = "riscv32imac-unknown-none-elf"


class FakeMeson:
    """Stands in for build.run. Records every command and mimics meson's configure
    behavior: a plain setup or --wipe configures with the cross file as it is now
    (build.ninja plus a copy of that cross file), while --reconfigure keeps the cross
    file of the original configure, which is exactly the stale-flag case."""

    def __init__(self, work, interrupt_setups=()):
        self.work = Path(work)
        self.commands = []
        self.interrupt_setups = set(interrupt_setups)
        self.setups = 0

    def __call__(self, command, **kwargs):
        command = [str(part) for part in command]
        self.commands.append(command)
        if command[:2] != ["meson", "setup"]:
            return
        self.setups += 1
        build_dir = self.work / "picolibc-build"
        if self.setups in self.interrupt_setups:
            # The setup fails after meson wrote build.ninja, as a run interrupted there.
            build_dir.mkdir(parents=True, exist_ok=True)
            (build_dir / "build.ninja").write_text("# interrupted setup\n")
            raise subprocess.CalledProcessError(1, command)
        if "--reconfigure" not in command:
            build_dir.mkdir(parents=True, exist_ok=True)
            (build_dir / "build.ninja").write_text("# configured\n")
            (build_dir / ".configured-cross").write_text(
                (self.work / "picolibc.cross").read_text())

    def setup_commands(self):
        return [command for command in self.commands if command[:2] == ["meson", "setup"]]


class SetupGuardTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="xila-test-build-picolibc-")
        self.addCleanup(self.temporary.cleanup)
        self.work = Path(self.temporary.name) / "work"
        self.work.mkdir(parents=True)
        self.source = Path(self.temporary.name) / "picolibc"
        self.source.mkdir()
        (self.source / "meson.build").write_text("# fake picolibc\n")
        # The cross file and the stamp name tools and a compiler without needing them.
        self.real_compiler_identity = build.compiler_identity
        patches = [
            mock.patch.object(build, "find_target_tool", lambda target, role: f"fake-{role}"),
            mock.patch.object(build, "compiler_command",
                              lambda target: ["fake-clang", f"--target={target}"]),
            mock.patch.object(build, "compiler_identity",
                              lambda target: f"fake-clang {target} version 1"),
            mock.patch.object(build, "run", FakeMeson(self.work)),
        ]
        for patch in patches:
            patch.start()
            self.addCleanup(patch.stop)
        self.meson = build.run

    def build_picolibc(self, target=TARGET, source=None):
        return build.build_picolibc(source or self.source, target, self.work)

    def stamp(self):
        return self.work / "picolibc-setup.stamp"

    def configured_cross(self):
        return (self.work / "picolibc-build" / ".configured-cross").read_text()

    def inputs(self, install=None, source=None):
        options = [f"-D{key}={value}" for key, value in build.PICOLIBC_OPTIONS.items()]
        return build.setup_inputs(source or self.source, TARGET, self.work / "picolibc.cross",
                                  install or self.work / "picolibc", options)

    def test_successful_setup_stamps_its_inputs_and_later_runs_reconfigure(self):
        self.build_picolibc()
        self.assertEqual(self.stamp().read_text().strip(), self.inputs())
        self.assertFalse(self.stamp().with_name(self.stamp().name + ".tmp").exists())
        self.build_picolibc()
        setups = self.meson.setup_commands()
        self.assertEqual(len(setups), 2)
        self.assertNotIn("--wipe", setups[0])
        self.assertNotIn("--reconfigure", setups[0])
        self.assertIn("--reconfigure", setups[1])
        self.assertNotIn("--wipe", setups[1])

    def test_interrupted_setup_leaves_no_stamp_and_is_redone_with_wipe(self):
        self.meson.interrupt_setups.add(1)
        with self.assertRaises(subprocess.CalledProcessError):
            self.build_picolibc()
        # The setup failed, so no inputs were recorded, but meson had written build.ninja.
        self.assertFalse(self.stamp().exists())
        self.assertTrue((self.work / "picolibc-build" / "build.ninja").exists())
        self.build_picolibc()
        setups = self.meson.setup_commands()
        self.assertEqual(len(setups), 2)
        self.assertNotIn("--reconfigure", setups[1])
        self.assertIn("--wipe", setups[1])
        # The redo configured with the cross file as it is now, not a stale one.
        self.assertEqual(self.configured_cross(), (self.work / "picolibc.cross").read_text())
        self.assertTrue(self.stamp().exists())

    def test_interrupted_redo_of_new_inputs_keeps_no_stale_stamp(self):
        # Configure with cross file A, start a redo for B and interrupt it after meson
        # wrote build.ninja, then revert to A: the stamp must not survive the redo, so
        # the reverted run wipes instead of reconfiguring with B's flags.
        self.build_picolibc(TARGET)
        self.meson.interrupt_setups.add(2)
        with self.assertRaises(subprocess.CalledProcessError):
            self.build_picolibc(OTHER_TARGET)
        self.assertFalse(self.stamp().exists())
        self.build_picolibc(TARGET)
        setups = self.meson.setup_commands()
        self.assertEqual(len(setups), 3)
        self.assertIn("--wipe", setups[1])
        self.assertIn("--wipe", setups[2])
        self.assertNotIn("--reconfigure", setups[2])
        self.assertEqual(self.configured_cross(), (self.work / "picolibc.cross").read_text())
        self.assertEqual(self.stamp().read_text().strip(), self.inputs())

    def test_changed_cross_file_redoes_the_setup(self):
        self.build_picolibc(TARGET)
        self.build_picolibc(OTHER_TARGET)
        setups = self.meson.setup_commands()
        self.assertEqual(len(setups), 2)
        self.assertIn("--wipe", setups[1])
        self.assertEqual(self.configured_cross(), (self.work / "picolibc.cross").read_text())

    def test_changed_compiler_flag_in_the_cross_file_redoes_the_setup(self):
        # Same target, same mocked compiler identity: only the cross file changes, so this
        # exercises the cross-file part of the hashed inputs.
        self.build_picolibc(TARGET)
        with mock.patch.object(build, "compiler_command",
                               lambda target: ["fake-clang", f"--target={target}",
                                               "-march=rv32imac"]):
            self.build_picolibc(TARGET)
        setups = self.meson.setup_commands()
        self.assertEqual(len(setups), 2)
        self.assertIn("--wipe", setups[1])
        self.assertNotIn("--reconfigure", setups[1])
        self.assertIn("-march=rv32imac", (self.work / "picolibc.cross").read_text())
        self.assertEqual(self.configured_cross(), (self.work / "picolibc.cross").read_text())

    def test_unusable_compiler_exits_with_an_error(self):
        for failure in (FileNotFoundError("fake-clang"),
                        subprocess.CalledProcessError(1, ["fake-clang"])):
            with mock.patch.object(build.subprocess, "run", side_effect=failure):
                with self.assertRaises(SystemExit) as raised:
                    self.real_compiler_identity(TARGET)
            self.assertIn("does not report a version", str(raised.exception))

    def test_different_recorded_inputs_redo_the_setup(self):
        self.build_picolibc()
        self.stamp().write_text("stale inputs\n")
        self.build_picolibc()
        setups = self.meson.setup_commands()
        self.assertEqual(len(setups), 2)
        self.assertIn("--wipe", setups[1])

    def test_changed_source_directory_redoes_the_setup(self):
        self.build_picolibc()
        other_source = self.source.parent / "picolibc-copy"
        other_source.mkdir()
        (other_source / "meson.build").write_text("# fake picolibc copy\n")
        self.build_picolibc(source=other_source)
        setups = self.meson.setup_commands()
        self.assertEqual(len(setups), 2)
        self.assertIn("--wipe", setups[1])

    def test_changed_picolibc_option_redoes_the_setup(self):
        self.build_picolibc()
        with mock.patch.dict(build.PICOLIBC_OPTIONS, {"multilib": "true"}):
            self.build_picolibc()
        setups = self.meson.setup_commands()
        self.assertEqual(len(setups), 2)
        self.assertIn("--wipe", setups[1])

    def test_changed_prefix_redoes_the_setup(self):
        self.build_picolibc()
        self.stamp().write_text(self.inputs(install=self.work / "other-prefix") + "\n")
        self.build_picolibc()
        setups = self.meson.setup_commands()
        self.assertEqual(len(setups), 2)
        self.assertIn("--wipe", setups[1])

    def test_changed_compiler_identity_redoes_the_setup(self):
        self.build_picolibc()
        with mock.patch.object(build, "compiler_identity", lambda target: "fake-clang 2.0"):
            self.build_picolibc()
        setups = self.meson.setup_commands()
        self.assertEqual(len(setups), 2)
        self.assertIn("--wipe", setups[1])

    def test_changed_meson_environment_redoes_the_setup(self):
        self.build_picolibc()
        with mock.patch.dict(os.environ, {"CFLAGS": "-O0"}):
            self.build_picolibc()
        setups = self.meson.setup_commands()
        self.assertEqual(len(setups), 2)
        self.assertIn("--wipe", setups[1])


if __name__ == "__main__":
    unittest.main()
