"""build.py's picolibc setup guard: the cross-file inputs are stamped only after a
successful `meson setup`, so an interrupted setup is redone with --wipe instead of being
reconfigured with the stale flags of the original cross file."""

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
            # Killed after meson wrote build.ninja but before the setup returned.
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
        # The cross file names tools and compiler flags without needing them installed.
        patches = [
            mock.patch.object(build, "find_target_tool", lambda target, role: f"fake-{role}"),
            mock.patch.object(build, "compiler_command",
                              lambda target: ["fake-clang", f"--target={target}"]),
            mock.patch.object(build, "run", FakeMeson(self.work)),
        ]
        for patch in patches:
            patch.start()
            self.addCleanup(patch.stop)
        self.meson = build.run

    def build_picolibc(self, target=TARGET):
        return build.build_picolibc(self.source, target, self.work)

    def stamp(self):
        return self.work / "picolibc-setup.stamp"

    def configured_cross(self):
        return (self.work / "picolibc-build" / ".configured-cross").read_text()

    def test_successful_setup_stamps_its_inputs_and_later_runs_reconfigure(self):
        self.build_picolibc()
        cross = self.work / "picolibc.cross"
        options = [f"-D{key}={value}" for key, value in build.PICOLIBC_OPTIONS.items()]
        self.assertEqual(self.stamp().read_text().strip(),
                         build.setup_inputs(cross, self.work / "picolibc", options))
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

    def test_changed_cross_file_redoes_the_setup(self):
        self.build_picolibc(TARGET)
        self.build_picolibc(OTHER_TARGET)
        setups = self.meson.setup_commands()
        self.assertEqual(len(setups), 2)
        self.assertIn("--wipe", setups[1])
        self.assertEqual(self.configured_cross(), (self.work / "picolibc.cross").read_text())

    def test_different_recorded_inputs_redo_the_setup(self):
        self.build_picolibc()
        self.stamp().write_text("stale inputs\n")
        self.build_picolibc()
        setups = self.meson.setup_commands()
        self.assertEqual(len(setups), 2)
        self.assertIn("--wipe", setups[1])


if __name__ == "__main__":
    unittest.main()
