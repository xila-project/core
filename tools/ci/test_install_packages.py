"""Tests for tools/ci/install_packages.py; the system commands are recorded, never run."""

from __future__ import annotations

import contextlib
import io
import json
import os
import pathlib
import subprocess
import tempfile
import unittest
from unittest import mock

import install_packages

RUNNER_MIRRORS = (
    "http://azure.archive.ubuntu.com/ubuntu/\tpriority:1\n"
    "https://archive.ubuntu.com/ubuntu/\tpriority:2\n"
    "https://security.ubuntu.com/ubuntu/\tpriority:3\n"
)


def status_output(**states: str) -> str:
    packages = [
        {"package": name.replace("_", "-"), "desired_state": "present", "state": state}
        for name, state in states.items()
    ]
    return json.dumps({"apt": {"available": True, "packages": packages}})


class FakeSystem:
    """Stands in for subprocess.run: answers mise status and records every other command."""

    def __init__(self, status: str, download_results: list[int] | None = None):
        self.status = status
        self.download_results = list(download_results or [])
        self.commands: list[list[str]] = []
        self.inputs: list[str | None] = []

    def __call__(self, command, **options):
        if command[3:6] == ["bootstrap", "packages", "status"]:
            return subprocess.CompletedProcess(command, 0, stdout=self.status, stderr="")
        self.commands.append(command)
        self.inputs.append(options.get("input"))
        result = 0
        if "--download-only" in command and self.download_results:
            result = self.download_results.pop(0)
        return subprocess.CompletedProcess(command, result)

    def apt_commands(self, action: str) -> list[list[str]]:
        return [command for command in self.commands if "apt-get" in command and action in command]

    def mise_applies(self) -> list[list[str]]:
        return [command for command in self.commands if command[:1] == ["mise"]]


class InstallPackagesTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.runner_temporary = pathlib.Path(self.temporary.name) / "runner"
        self.runner_temporary.mkdir()
        self.mirror_list = pathlib.Path(self.temporary.name) / "apt-mirrors.txt"
        self.mirror_list.write_text(RUNNER_MIRRORS, encoding="utf-8")
        patches = [
            mock.patch.object(install_packages, "MIRROR_LIST", self.mirror_list),
            mock.patch.object(install_packages.os, "geteuid", return_value=1001),
        ]
        for patch in patches:
            patch.start()
            self.addCleanup(patch.stop)

    def main(self, system: FakeSystem, arguments: list[str], hosted: bool) -> int:
        environment = {"GITHUB_ACTIONS": "true", "RUNNER_TEMP": str(self.runner_temporary)} if hosted else {}
        with (
            mock.patch.dict(os.environ, environment, clear=True),
            mock.patch.object(install_packages.subprocess, "run", system),
            contextlib.redirect_stdout(io.StringIO()),
        ):
            return install_packages.main(arguments)

    def test_installed_packages_need_no_apt_command(self):
        for hosted in [False, True]:
            with self.subTest(hosted=hosted):
                system = FakeSystem(status_output(clang="installed", mold="installed"))
                self.assertEqual(self.main(system, ["ci"], hosted), 0)
                self.assertEqual(system.commands, [])

    def test_developer_machine_runs_the_unchanged_mise_command(self):
        system = FakeSystem(status_output(clang="installed", lld="missing"))
        self.assertEqual(self.main(system, ["ci,sdk"], hosted=False), 0)
        self.assertEqual(
            system.commands,
            [["mise", "-E", "ci,sdk", "bootstrap", "packages", "apply", "--manager", "apt", "--yes", "--update"]],
        )
        self.assertEqual(self.mirror_list.read_text(encoding="utf-8"), RUNNER_MIRRORS)

    def test_hosted_runner_downloads_under_limits_then_installs_from_the_cache(self):
        system = FakeSystem(status_output(clang="installed", lld="missing", cmake="missing"))
        self.assertEqual(self.main(system, ["ci,sdk"], hosted=True), 0)
        update, download, apply = system.commands
        for command, limit in [(update, "60"), (download, "180")]:
            self.assertEqual(command[:5], ["sudo", "-n", "timeout", "--kill-after=10", limit])
            for option in ["Acquire::Retries=3", "Acquire::http::Timeout=30", "Acquire::https::Timeout=30"]:
                self.assertIn(option, command)
        self.assertEqual(update[-1], "update")
        self.assertEqual(download[-6:], ["install", "--download-only", "--yes", "--", "lld", "cmake"])
        self.assertEqual(apply, ["mise", "-E", "ci,sdk", "bootstrap", "packages", "apply", "--manager", "apt", "--yes"])
        self.assertTrue((self.runner_temporary / install_packages.LISTS_REFRESHED_MARKER).exists())

    def test_package_lists_refresh_once_per_job(self):
        first = FakeSystem(status_output(lld="missing"))
        self.main(first, ["ci,sdk"], hosted=True)
        second = FakeSystem(status_output(gcc_arm_none_eabi="missing"))
        self.assertEqual(self.main(second, ["ci,sdk-arm"], hosted=True), 0)
        self.assertEqual(len(first.apt_commands("update")), 1)
        self.assertEqual(second.apt_commands("update"), [])
        self.assertEqual(len(second.apt_commands("--download-only")), 1)

    def test_slow_mirror_is_stopped_and_retried_with_the_next_mirror_first(self):
        system = FakeSystem(status_output(gcc_riscv64_unknown_elf="missing"), download_results=[124, 0])
        self.assertEqual(self.main(system, ["ci,sdk,sdk-riscv"], hosted=True), 0)
        self.assertEqual(len(system.apt_commands("--download-only")), 2)
        self.assertEqual(len(system.apt_commands("update")), 1)
        tee = system.commands.index(["sudo", "-n", "tee", str(self.mirror_list)])
        self.assertTrue(system.inputs[tee].startswith("https://archive.ubuntu.com/ubuntu/\tpriority:1\n"))
        self.assertEqual(len(system.mise_applies()), 1)

    def test_failed_downloads_stop_before_installing(self):
        system = FakeSystem(status_output(lld="missing"), download_results=[124, 100, 124])
        self.assertEqual(self.main(system, ["ci,sdk"], hosted=True), 1)
        self.assertEqual(len(system.apt_commands("--download-only")), install_packages.ATTEMPTS)
        self.assertEqual(system.mise_applies(), [])

    def test_hosted_dry_run_changes_nothing_and_asks_mise_for_its_plan(self):
        system = FakeSystem(status_output(lld="missing"))
        self.assertEqual(self.main(system, ["--dry-run", "ci,sdk"], hosted=True), 0)
        self.assertEqual(system.commands, [system.mise_applies()[0]])
        self.assertEqual(system.commands[0][-1], "--dry-run")
        self.assertFalse((self.runner_temporary / install_packages.LISTS_REFRESHED_MARKER).exists())

    def test_rotated_mirror_list_moves_the_first_mirror_last(self):
        self.assertEqual(
            install_packages.rotated_mirror_list(RUNNER_MIRRORS),
            "https://archive.ubuntu.com/ubuntu/\tpriority:1\n"
            "https://security.ubuntu.com/ubuntu/\tpriority:2\n"
            "http://azure.archive.ubuntu.com/ubuntu/\tpriority:3\n",
        )
        self.assertIsNone(install_packages.rotated_mirror_list("# comment\nhttp://one.example/ubuntu/\n"))


if __name__ == "__main__":
    unittest.main()
