#!/usr/bin/env python3
"""Install the apt packages that mise environments declare, with bounded downloads on CI.

Usage: python3 tools/ci/install_packages.py [--dry-run] ENVIRONMENTS (for example ci,host,sdk)

Every `ci:packages-*` task runs this script, so each rule below lives in one place.

- When mise reports every declared package as installed, the script stops: no
  `apt-get update` and no network access. Build tasks can therefore keep their package
  dependencies for developers who run them directly, while a job's explicit setup step
  has already installed the packages.
- On a developer machine (`GITHUB_ACTIONS` is not `true`) it runs the same command the
  tasks always ran: `mise bootstrap packages apply --manager apt --update --yes`. It
  changes no apt configuration and no mirror list.
- On a GitHub Actions runner, the network part runs first and under limits:
  `apt-get update` once per job, then `apt-get install --download-only`, both with apt's
  own retries and read timeouts and with a wall-clock limit per attempt. A mirror that
  stalls, or that serves the archives at a trickle (core#792: 95 MB at 67 kB/s for 24
  minutes), is stopped and retried with the next mirror of the runner's mirror list
  first; apt keeps the archives it finished and resumes partial ones. mise then installs
  the packages from the local archive cache, so the step that runs dpkg is never
  interrupted. apt verifies every archive against the signed package lists, as usual.
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import subprocess

ATTEMPTS = 3
UPDATE_LIMIT_SECONDS = 60
DOWNLOAD_LIMIT_SECONDS = 180
# Retries and read timeouts of each apt request. They catch a dead connection, not a slow one,
# hence the wall-clock limits above.
APT_NETWORK_OPTIONS = [
    "-o",
    "Acquire::Retries=3",
    "-o",
    "Acquire::http::Timeout=30",
    "-o",
    "Acquire::https::Timeout=30",
]
# The `mirror+file:` source list of GitHub-hosted Ubuntu runners.
MIRROR_LIST = pathlib.Path("/etc/apt/apt-mirrors.txt")
# Marks, in the runner's per-job temporary directory, that this job already refreshed the lists.
LISTS_REFRESHED_MARKER = "xila-apt-package-lists-refreshed"
TIMEOUT_EXIT_STATUS = 124


def missing_packages(environments: str) -> list[str]:
    """Return the apt packages that mise reports as not installed for these environments."""
    status = subprocess.run(
        ["mise", "-E", environments, "bootstrap", "packages", "status", "--json"],
        check=True,
        capture_output=True,
        text=True,
    )
    packages = json.loads(status.stdout).get("apt", {}).get("packages", [])
    return [
        package["package"]
        for package in packages
        if package.get("desired_state") == "present" and package.get("state") != "installed"
    ]


def as_root(command: list[str]) -> list[str]:
    return command if os.geteuid() == 0 else ["sudo", "-n", *command]


def run(command: list[str], dry_run: bool, **options) -> int:
    print("+ " + " ".join(command), flush=True)
    if dry_run:
        return 0
    return subprocess.run(command, check=False, **options).returncode


def bounded_apt(arguments: list[str], limit_seconds: int, dry_run: bool) -> bool:
    """Run apt-get as root under a wall-clock limit; `timeout` runs as root so it can stop apt."""
    command = as_root(
        [
            "timeout",
            "--kill-after=10",
            str(limit_seconds),
            "env",
            "DEBIAN_FRONTEND=noninteractive",
            "apt-get",
            *APT_NETWORK_OPTIONS,
            *arguments,
        ]
    )
    status = run(command, dry_run)
    if status == TIMEOUT_EXIT_STATUS:
        print(f"apt-get {arguments[0]} exceeded its {limit_seconds} second limit.", flush=True)
    elif status != 0:
        print(f"apt-get {arguments[0]} failed with exit status {status}.", flush=True)
    return status == 0


def rotated_mirror_list(text: str) -> str | None:
    """Move the first mirror to the end and renumber the priorities; None with fewer than two."""
    entries = [line.split() for line in text.splitlines() if line.strip() and not line.lstrip().startswith("#")]
    if len(entries) < 2:
        return None
    entries = entries[1:] + entries[:1]
    lines = []
    for priority, (mirror, *fields) in enumerate(entries, start=1):
        fields = [field for field in fields if not field.startswith("priority:")]
        lines.append("\t".join([mirror, *fields, f"priority:{priority}"]))
    return "\n".join(lines) + "\n"


def prefer_next_mirror(dry_run: bool) -> None:
    if not MIRROR_LIST.is_file():
        return
    rotated = rotated_mirror_list(MIRROR_LIST.read_text(encoding="utf-8"))
    if rotated is None:
        return
    print(f"Trying {rotated.split()[0]} first.", flush=True)
    run(as_root(["tee", str(MIRROR_LIST)]), dry_run, input=rotated, text=True, stdout=subprocess.DEVNULL)


def download(packages: list[str], dry_run: bool) -> bool:
    """Fetch the archives of the packages into apt's cache, in bounded and retried attempts."""
    runner_temporary = os.environ.get("RUNNER_TEMP")
    marker = pathlib.Path(runner_temporary) / LISTS_REFRESHED_MARKER if runner_temporary else None
    for attempt in range(1, ATTEMPTS + 1):
        if attempt > 1:
            prefer_next_mirror(dry_run)
        lists_ready = marker is not None and marker.exists()
        if not lists_ready:
            lists_ready = bounded_apt(["update"], UPDATE_LIMIT_SECONDS, dry_run)
            if lists_ready and marker is not None and not dry_run:
                marker.touch()
        arguments = ["install", "--download-only", "--yes", "--", *packages]
        if lists_ready and bounded_apt(arguments, DOWNLOAD_LIMIT_SECONDS, dry_run):
            return True
        print(f"::warning::Package download attempt {attempt} of {ATTEMPTS} did not finish.", flush=True)
    print(f"::error::Could not download {', '.join(packages)} in {ATTEMPTS} attempts.", flush=True)
    return False


def main(arguments: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("environments", help="comma-separated mise environments, for example ci,host,sdk")
    parser.add_argument("--dry-run", action="store_true", help="print the commands without changing the system")
    options = parser.parse_args(arguments)

    missing = missing_packages(options.environments)
    if not missing:
        print(f"Every apt package of {options.environments} is installed.", flush=True)
        return 0

    apply = ["mise", "-E", options.environments, "bootstrap", "packages", "apply", "--manager", "apt", "--yes"]
    if os.environ.get("GITHUB_ACTIONS") == "true":
        if not download(missing, options.dry_run):
            return 1
    else:
        apply.append("--update")
    if options.dry_run:
        apply.append("--dry-run")
    # mise's --dry-run prints its plan without installing, so the command always runs.
    return run(apply, dry_run=False)


if __name__ == "__main__":
    raise SystemExit(main())
