"""Focused behavioral policy regressions for the current CI workflows."""

import os
import pathlib
import re
import subprocess
import tempfile
import textwrap
import tomllib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
ACTIVE_WORKFLOWS = [
    path
    for path in (ROOT / ".github/workflows").glob("*.yml")
    if path.name != "cache-benchmark.yml"
]


def job(workflow, name):
    text = (ROOT / ".github/workflows" / workflow).read_text()
    return re.search(rf"^  {re.escape(name)}:\n(.*?)(?=^  [\w-]+:|\Z)", text, re.M | re.S)[1]


class WorkflowPolicy(unittest.TestCase):
    def test_task_names_are_namespaced_and_developer_tasks_are_environment_scoped(self):
        shared = tomllib.loads((ROOT / "mise.toml").read_text())["tasks"]
        development = tomllib.loads((ROOT / "mise.development.toml").read_text()).get("tasks", {})
        ci = tomllib.loads((ROOT / "mise.ci.toml").read_text())["tasks"]
        for tasks in [shared, development, ci]:
            for name in tasks:
                self.assertRegex(name, r"^[a-z][a-z0-9-]*:[a-z][a-z0-9-]*$", name)
                self.assertNotIn("alias", tasks[name])
        for name in ["format:rust", "format:toml", "format:json", "format:all", "translate:sync", "dev:before-commit"]:
            self.assertIn(name, development)
            self.assertNotIn(name, shared)
            self.assertNotIn(name, ci)
        for name in ["ci:format-rust", "ci:format-toml", "ci:quality-js", "ci:lint-workflows"]:
            self.assertIn("run", ci[name])
        for environment in [development, ci]:
            effective = shared | environment
            for name, task in effective.items():
                for dependency in task.get("depends", []):
                    self.assertIn(dependency, effective, (name, dependency))
                for callee in re.findall(r"mise run ([\w:-]+)", task.get("run", "")):
                    self.assertIn(callee, effective, (name, callee))
        for workflow in ACTIVE_WORKFLOWS:
            for callee in re.findall(r"mise run ([\w:-]+)", workflow.read_text()):
                self.assertIn(callee, shared | ci, (workflow.name, callee))

    def test_ci_cargo_wrapper_reuses_existing_mbx_only_when_enabled(self):
        ci = tomllib.loads((ROOT / "mise.ci.toml").read_text())
        self.assertIn("cargo", ci.get("wrappers", {}))
        wrapper = ci["wrappers"]["cargo"]
        self.assertEqual(wrapper["command"], "env")
        self.assertIn("XILA_MBX", wrapper["args"][0])
        self.assertIn("mbx", wrapper["args"][0])
        self.assertEqual(wrapper["env"]["MBX_CARGO_SHIM_MODE"], "1")
        for task in ci["tasks"].values():
            self.assertNotIn("mr-boxington", task.get("tools", {}))

    def test_build_cache_payloads_cover_all_cargo_target_layouts(self):
        builds = {
            "ci.yml": {"host": "objects", "wasm": "target", "abi": "objects", "localization": "objects"},
            "extended.yml": {"coverage": "objects", "profiling": "target", "abi-deep": "objects"},
            "pages.yml": {"build": "objects"},
            "benchmarks.yml": {"profile": "target", "test-runner": "target"},
        }
        for workflow, names in builds.items():
            for name, mode in names.items():
                with self.subTest(workflow=workflow, job=name):
                    text = job(workflow, name)
                    self.assertEqual(text.count("jdx/mr-boxington-action@v1"), 1)
                    self.assertIn(f"github-cache-mode: {mode}", text)
                    self.assertIn('XILA_MBX: "1"', text)
                    self.assertIn("cache-generation: v1-", text)
                    self.assertNotIn("cache-key-suffix:", text)
                    self.assertNotIn("sccache", text.lower())
                    self.assertNotIn("Swatinem/rust-cache", text)
                    self.assertNotIn("RUSTC_WRAPPER:", text)
                    self.assertNotIn("RUSTC_WORKSPACE_WRAPPER:", text)

    def test_cache_restore_is_read_only_on_prs_and_matrix_modes_are_separate(self):
        for workflow in ["ci.yml", "extended.yml", "pages.yml"]:
            text = (ROOT / ".github/workflows" / workflow).read_text()
            for option in ["save-on-pull-request", "save-on-protected-branch", "save-on-workflow-dispatch"]:
                self.assertNotRegex(text, rf"{option}:\s*['\"]?true")
        for workflow, name in [("ci.yml", "abi"), ("extended.yml", "abi-deep")]:
            self.assertRegex(job(workflow, name), r"cache-generation:.*matrix.memory")

    def test_extended_and_pages_caches_can_be_seeded_by_trusted_main_dispatch(self):
        for workflow, names in {"extended.yml": ["coverage", "profiling", "abi-deep"], "pages.yml": ["build"]}.items():
            for name in names:
                self.assertIn("save-on-workflow-dispatch: ${{ github.ref == 'refs/heads/main' }}", job(workflow, name))

    def test_mbx_key_toolchain_matches_build_toolchain(self):
        for workflow, names in {
            "ci.yml": ["host", "wasm", "abi", "localization"],
            "extended.yml": ["coverage", "profiling"],
            "pages.yml": ["build"],
        }.items():
            for name in names:
                self.assertIn('toolchain: "1.99.0"', job(workflow, name))
        self.assertIn("toolchain: nightly-2026-10-12", job("extended.yml", "abi-deep"))

    def test_quality_does_not_enable_compilation_cache(self):
        quality = job("ci.yml", "quality")
        self.assertNotIn("XILA_MBX", quality)
        self.assertNotIn("mr-boxington-action", quality)

    def test_workflow_lint_includes_shellcheck_on_local_and_ci_runs(self):
        tasks = tomllib.loads((ROOT / "mise.ci.toml").read_text())["tasks"]
        self.assertIn("aqua:koalaman/shellcheck", tasks["ci:lint-workflows"]["tools"])

    def test_no_workflow_keeps_superseded_rust_caches(self):
        for workflow in ACTIVE_WORKFLOWS:
            text = workflow.read_text()
            self.assertNotIn("sccache", text.lower(), workflow.name)
            self.assertNotIn("Swatinem/rust-cache", text, workflow.name)

    def test_primary_ci_jobs_are_parallel_and_required_aggregates_results(self):
        ci = (ROOT / ".github/workflows/ci.yml").read_text()
        self.assertIn("workflow_call:", ci)
        primary = ["quality", "host", "wasm", "abi", "localization"]
        for name in primary:
            with self.subTest(job=name):
                self.assertNotRegex(job("ci.yml", name), r"(?m)^    needs:")

        required = job("ci.yml", "required")
        self.assertIn("if: always()", required)
        self.assertIn('if [[ "$result" != success ]]', required)
        for name in primary:
            with self.subTest(required_job=name):
                self.assertIn(f"needs.{name}.result", required)

    def test_primary_abi_matrix_uses_memory32_on_pr_and_both_modes_elsewhere(self):
        abi = job("ci.yml", "abi")
        self.assertIn("memory_32", abi)
        self.assertIn("memory_64", abi)
        self.assertIn("github.event_name == 'pull_request'", abi)
        self.assertIn("ci:abi", abi)

    def test_localization_job_selects_catalogs_and_runs_all_locales_for_shared_changes(self):
        localization = job("ci.yml", "localization")
        self.assertIn("CI_LOCALES", localization)
        self.assertIn("mise run ci:localization", localization)
        self.assertIn('"$EVENT_NAME" != pull_request', localization)
        self.assertIn("all_locales=true", localization)
        self.assertIn("locales=(en fr es de pt-BR it pl)", localization)
        self.assertIn("locales=(en)", localization)

    def test_locale_selection_handles_build_scripts_catalogs_and_unrelated_changes(self):
        localization = job("ci.yml", "localization")
        script = textwrap.dedent(re.search(
            r"(?ms)^      - name: Select locales\n.*?^        run: \|\n(.*?)(?=^      - )",
            localization,
        )[1])
        for changed, expected in [
            ("subsystems/graphics/build.rs", "en fr es de pt-BR it pl"),
            ("executables/clock/locales/fr.json", "en fr"),
            ("readme.md", ""),
        ]:
            with self.subTest(changed=changed), tempfile.TemporaryDirectory() as directory:
                def git(*args, **kwargs):
                    return subprocess.check_output(["git", *args], cwd=directory, text=True, **kwargs).strip()

                git("init", "--quiet")
                base = git("hash-object", "-w", "-t", "tree", "--stdin", input="")
                path = pathlib.Path(directory) / changed
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("fixture\n")
                git("add", "--", changed)
                head = git("write-tree")
                output = pathlib.Path(directory) / "github-output"
                subprocess.run(["bash", "-c", script], cwd=directory, check=True, capture_output=True, text=True, env=os.environ | {
                    "EVENT_NAME": "pull_request", "BASE_SHA": base, "HEAD_SHA": head, "GITHUB_OUTPUT": str(output),
                })
                result = dict(line.split("=", 1) for line in output.read_text().splitlines())
                self.assertEqual(result["value"], expected)
                self.assertEqual(result["run-check"], "true" if expected else "false")

    def test_extended_validation_is_reusable_and_checks_all_deep_modes(self):
        extended = (ROOT / ".github/workflows/extended.yml").read_text()
        self.assertIn("workflow_call:", extended)
        self.assertIn("schedule:", extended)
        self.assertIn("workflow_dispatch:", extended)
        for name in ["coverage", "abi-deep", "profiling", "release-gate"]:
            with self.subTest(job=name):
                self.assertRegex(extended, rf"(?m)^  {re.escape(name)}:")
        deep_task = tomllib.loads((ROOT / "mise.ci.toml").read_text())["tasks"]["ci:abi-deep"]["run"]
        self.assertIn("fail-under-lines 100", deep_task)
        self.assertIn("fail-under-functions 100", deep_task)
        self.assertIn("i686-unknown-linux-gnu", deep_task)
        self.assertIn("powerpc-unknown-linux-gnu", deep_task)
        self.assertIn("validated-sha", extended)

    def test_runtime_publication_gates_main_and_tags_but_never_prs(self):
        runtime = (ROOT / ".github/workflows/runtime.yml").read_text()
        self.assertIn('workflows: ["CI"]', runtime)
        self.assertIn("pull_request:", runtime)
        self.assertIn('tags: ["v*"]', runtime)
        self.assertIn("uses: ./.github/workflows/ci.yml", runtime)
        self.assertIn("uses: ./.github/workflows/extended.yml", runtime)
        self.assertIn("needs.deep.outputs.validated-sha == github.sha", runtime)
        self.assertIn("github.event_name == 'pull_request'", runtime)
        self.assertIn("docker push", runtime)
        self.assertNotIn("  changes:", runtime)
        self.assertIn("github.event.workflow_run.head_branch == 'main'", runtime)

    def test_runtime_image_is_smoke_tested_before_publication(self):
        runtime = (ROOT / ".github/workflows/runtime.yml").read_text()
        self.assertIn("Check image user and entrypoint", runtime)
        self.assertIn("Test read-only root, host mounts, state persistence, and exit status", runtime)
        self.assertIn("Exercise Xila CLI shell over stdin", runtime)
        self.assertLess(runtime.index("Test read-only root"), runtime.index("Publish image to GitHub Container Registry"))
        self.assertLess(runtime.index("Exercise Xila CLI shell"), runtime.index("Publish image to GitHub Container Registry"))

    def test_runtime_dockerfile_does_not_export_redundant_stable_toolchain_variable(self):
        dockerfile = (ROOT / "Dockerfile.runtime").read_text()
        self.assertNotIn("XILA_RUST_TOOLCHAIN", dockerfile)

    def test_pages_waits_for_main_ci_and_builds_without_incremental_restoration(self):
        pages = (ROOT / ".github/workflows/pages.yml").read_text()
        self.assertIn('workflows: ["CI"]', pages)
        self.assertIn("workflow_run:", pages)
        self.assertIn("mise run pages:build", pages)
        self.assertIn("actions/upload-pages-artifact", pages)
        self.assertIn("actions/deploy-pages", pages)
        for old in ["listWorkflowRunArtifacts", "download-artifact", "incremental=true"]:
            with self.subTest(old=old):
                self.assertNotIn(old, pages)

    def test_pages_task_builds_all_locale_images_deterministically(self):
        task = tomllib.loads((ROOT / "mise.ci.toml").read_text())["tasks"]["pages:build"]["run"]
        for locale in ["en", "fr", "es", "de", "pt-BR", "it", "pl"]:
            with self.subTest(locale=locale):
                self.assertIn(locale, task)
        self.assertIn("--bundle wasm", task)
        self.assertIn("--bundle native", task)
        self.assertIn("cmp ", task)
        ci_tasks = tomllib.loads((ROOT / "mise.ci.toml").read_text())["tasks"]
        self.assertIn("mise run pages:demo", task)
        self.assertIn("trunk build --release", ci_tasks["pages:demo"]["run"])

    def test_image_generation_tasks_are_self_contained_and_limit_cleanup(self):
        tasks = tomllib.loads((ROOT / "mise.toml").read_text())["tasks"]
        native = tasks["image:native"]["run"]
        wasm = tasks["image:wasm"]["run"]
        self.assertNotIn("tools/mise/prepare-", native + wasm)
        self.assertIn("rm -f target/seed/native/seed.img", native)
        self.assertIn("rm -f target/seed/wasm/seed.img", wasm)
        self.assertNotRegex(native + wasm, r"rm\s+-rf")

    def test_benchmarks_are_manual_only_and_keep_profile_and_runner_experiments(self):
        benchmarks = (ROOT / ".github/workflows/benchmarks.yml").read_text()
        self.assertIn("workflow_dispatch:", benchmarks)
        self.assertNotIn("pull_request:", benchmarks)
        self.assertNotIn("push:", benchmarks)
        self.assertRegex(benchmarks, r"(?m)^  profile:")
        self.assertRegex(benchmarks, r"(?m)^  test-runner:")
        self.assertIn("mise run ci:benchmark-profile", benchmarks)
        ci_tasks = tomllib.loads((ROOT / "mise.ci.toml").read_text())["tasks"]
        self.assertIn("cargo test --workspace --timings", ci_tasks["ci:benchmark-profile"]["run"])
        self.assertIn("cargo nextest run --workspace --timings", ci_tasks["ci:benchmark-nextest"]["run"])

    def test_ci_task_groups_and_quality_environment_are_defined(self):
        config = tomllib.loads((ROOT / "mise.ci.toml").read_text())
        shared = tomllib.loads((ROOT / "mise.toml").read_text())["tasks"]
        self.assertFalse([name for name in shared if name.startswith("ci:") or name == "pages:build"])
        self.assertNotIn("install-nightly", shared)
        self.assertNotIn("install-nightly", config["tasks"])
        for task in [
            "ci:host",
            "ci:wasm",
            "ci:abi",
            "ci:localization",
            "ci:coverage",
            "ci:abi-deep",
            "ci:profiling",
            "pages:build",
        ]:
            with self.subTest(task=task):
                self.assertIn(task, config["tasks"])
                self.assertIn("run", config["tasks"][task])

        ci = tomllib.loads((ROOT / "mise.ci.toml").read_text())
        task_tools = {name: task.get("tools", {}) for name, task in ci["tasks"].items()}
        self.assertIn("rustfmt", task_tools["ci:format-rust"]["rust"]["components"])
        self.assertEqual(task_tools["ci:format-toml"]["taplo"], "0.10.0")
        self.assertEqual(task_tools["ci:lint-workflows"]["aqua:rhysd/actionlint"], "1.7.7")
        self.assertIn("clippy", task_tools["ci:host"]["rust"]["components"])

    def test_mise_configuration_pins_project_toolchains_and_core_tasks(self):
        config = tomllib.loads((ROOT / "mise.toml").read_text())
        self.assertNotIn("tools", config)
        self.assertNotIn("XILA_RUST_TOOLCHAIN", config.get("env", {}))
        self.assertNotIn("XILA_NIGHTLY_TOOLCHAIN", config.get("env", {}))
        for task in ["check:host", "check:guest", "localization:check", "image:native", "image:wasm"]:
            with self.subTest(task=task):
                self.assertIn(task, config["tasks"])

    def test_mise_manages_nightly_components_and_toolchain_selection(self):
        ci_text = (ROOT / "mise.ci.toml").read_text()
        tasks = tomllib.loads(ci_text)["tasks"]
        self.assertNotIn("rustup toolchain install", ci_text)
        self.assertNotIn("XILA_NIGHTLY_TOOLCHAIN", ci_text)
        for name in ["ci:abi-deep"]:
            rust = tasks[name]["tools"]["rust"]
            self.assertEqual(rust["version"], "nightly-2026-10-12")
            self.assertTrue({"miri", "rust-src"} <= set(rust["components"]))
            self.assertTrue({"i686-unknown-linux-gnu", "powerpc-unknown-linux-gnu"} <= set(rust["targets"]))
        self.assertIn("llvm-tools-preview", tasks["ci:abi-deep"]["tools"]["rust"]["components"])
        self.assertNotIn("cargo +", tasks["ci:abi-deep"]["run"])
        self.assertNotIn("RUSTUP_TOOLCHAIN=", tasks["pages:build"]["run"])
        self.assertIn("mise run pages:demo", tasks["pages:build"]["run"])
        self.assertEqual(tasks["pages:demo"]["tools"]["rust"]["version"], "nightly-2026-10-12")
        self.assertIn("wasm32-unknown-unknown", tasks["pages:demo"]["tools"]["rust"]["targets"])

    def test_development_mise_environment_is_the_default_and_includes_all_tool_groups(self):
        default = tomllib.loads((ROOT / ".miserc.toml").read_text())
        self.assertEqual(default["env"], ["development"])
        development = tomllib.loads((ROOT / "mise.development.toml").read_text())
        tools = development["tools"]
        for tool in ["rust", "node", "taplo", "aqua:rhysd/actionlint", "cargo:trunk", "aqua:taiki-e/cargo-llvm-cov", "aqua:cargo-bins/cargo-binstall"]:
            with self.subTest(tool=tool):
                self.assertIn(tool, tools)
        self.assertEqual(set(tools["rust"]["targets"]), {"wasm32-unknown-unknown", "wasm32-wasip1"})
        self.assertIn("rustfmt", tools["rust"]["components"])
        self.assertIn("clippy", tools["rust"]["components"])

    def test_developer_setup_documents_full_default_and_optional_nightly_install(self):
        readme = (ROOT / "readme.md").read_text()
        self.assertIn("mise install # installs the full development environment", readme)
        self.assertNotIn("mise run install-nightly", readme)

    def test_ci_environment_keeps_tools_scoped_to_individual_tasks(self):
        ci = tomllib.loads((ROOT / "mise.ci.toml").read_text())
        self.assertNotIn("tools", ci)
        task_tools = {name: task["tools"] for name, task in ci["tasks"].items() if "tools" in task}
        self.assertIn("taplo", task_tools["ci:format-toml"])
        self.assertIn("rust", task_tools["ci:host"])
        self.assertIn("wasm32-wasip1", task_tools["ci:wasm"]["rust"]["targets"])
        self.assertIn("aqua:taiki-e/cargo-llvm-cov", task_tools["ci:coverage"])
        self.assertIn("cargo:trunk", task_tools["pages:demo"])
        self.assertEqual({"mise.ci.toml", "mise.development.toml"}, {path.name for path in ROOT.glob("mise.*.toml")})

        for workflow in ACTIVE_WORKFLOWS:
            if "MISE_ENV:" in workflow.read_text():
                self.assertRegex(workflow.read_text(), r"(?m)^\s+MISE_ENV: ci$", workflow.name)

    def test_ci_native_package_sets_are_scoped_to_build_environments(self):
        config = tomllib.loads((ROOT / "mise.ci.toml").read_text())
        self.assertIn("packages", config.get("bootstrap", {}))
        packages = config["bootstrap"]["packages"]
        for package in ["clang", "mold"]:
            self.assertEqual(packages[f"apt:{package}"]["env"], "ci")
        for package in ["pkg-config", "libasound2-dev", "libudev-dev", "libxkbcommon-dev", "libwayland-dev", "libx11-dev", "libxkbcommon-x11-dev"]:
            self.assertEqual(packages[f"apt:{package}"]["env"], "host")
        self.assertEqual(packages["apt:time"]["env"], "benchmark")
        development = tomllib.loads((ROOT / "mise.development.toml").read_text())
        self.assertEqual(development["bootstrap"]["packages"]["apt:libasound2-dev"], "latest")

    def test_ci_native_packages_are_task_scoped_not_applied_by_quality(self):
        workflows = "\n".join(path.read_text() for path in ACTIVE_WORKFLOWS)
        self.assertNotIn("apt-get", workflows)
        tasks = tomllib.loads((ROOT / "mise.ci.toml").read_text())["tasks"]
        for name, environment in {
            "ci:packages-linker": "ci",
            "ci:packages-host": "ci,host",
            "ci:packages-benchmark": "ci,host,benchmark",
        }.items():
            self.assertIn(name, tasks)
            self.assertEqual(tasks[name]["run"], f"mise -E {environment} bootstrap packages apply --manager apt --update --yes")
        for name in ["ci:host", "ci:localization", "ci:coverage", "ci:profiling", "pages:build"]:
            self.assertIn("ci:packages-host", tasks[name]["depends"])
        for name in ["ci:wasm", "ci:abi", "ci:abi-deep"]:
            self.assertIn("ci:packages-linker", tasks[name]["depends"])
        for name in ["ci:format-rust", "ci:format-toml", "ci:quality-js", "ci:lint-workflows"]:
            self.assertFalse(tasks[name].get("depends"), name)
        self.assertNotIn("bootstrap packages apply", job("ci.yml", "quality"))
        runner = job("benchmarks.yml", "test-runner")
        self.assertLess(runner.index("mise run ci:packages-benchmark"), runner.index("/usr/bin/time"))

    def test_mise_multiline_ci_tasks_run_with_bash(self):
        tasks = tomllib.loads((ROOT / "mise.ci.toml").read_text())["tasks"]
        for name in ["ci:host", "ci:wasm", "ci:localization", "ci:abi-deep", "ci:profiling", "pages:build"]:
            with self.subTest(task=name):
                self.assertEqual(tasks[name].get("shell"), "bash -c")
        ci_tasks = tomllib.loads((ROOT / "mise.ci.toml").read_text())["tasks"]
        self.assertEqual(ci_tasks["ci:benchmark-profile"].get("shell"), "bash -c")

    def test_github_actions_use_version_tags_instead_of_commit_shas(self):
        for workflow in ACTIVE_WORKFLOWS:
            for line_number, line in enumerate(workflow.read_text().splitlines(), 1):
                if "uses:" in line:
                    with self.subTest(workflow=workflow.name, line=line_number):
                        self.assertNotRegex(line, r"uses:\s+[^\s]+@[0-9a-f]{40}\b")


if __name__ == "__main__":
    unittest.main()
