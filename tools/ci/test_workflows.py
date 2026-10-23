"""Focused behavioral policy regressions for the current CI workflows."""

import pathlib
import re
import tomllib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]


def job(workflow, name):
    text = (ROOT / ".github/workflows" / workflow).read_text()
    return re.search(rf"^  {re.escape(name)}:\n(.*?)(?=^  [\w-]+:|\Z)", text, re.M | re.S)[1]


class WorkflowPolicy(unittest.TestCase):
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

    def test_extended_validation_is_reusable_and_checks_all_deep_modes(self):
        extended = (ROOT / ".github/workflows/extended.yml").read_text()
        self.assertIn("workflow_call:", extended)
        self.assertIn("schedule:", extended)
        self.assertIn("workflow_dispatch:", extended)
        for name in ["coverage", "abi-deep", "profiling", "release-gate"]:
            with self.subTest(job=name):
                self.assertRegex(extended, rf"(?m)^  {re.escape(name)}:")
        deep_task = tomllib.loads((ROOT / "mise.toml").read_text())["tasks"]["ci:abi-deep"]["run"]
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
        task = tomllib.loads((ROOT / "mise.toml").read_text())["tasks"]["pages:build"]["run"]
        for locale in ["en", "fr", "es", "de", "pt-BR", "it", "pl"]:
            with self.subTest(locale=locale):
                self.assertIn(locale, task)
        self.assertIn("--bundle wasm", task)
        self.assertIn("--bundle native", task)
        self.assertIn("cmp ", task)
        self.assertIn("trunk build --release", task)

    def test_image_generation_tasks_are_self_contained_and_limit_cleanup(self):
        tasks = tomllib.loads((ROOT / "mise.toml").read_text())["tasks"]
        native = tasks["prepare-native-image"]["run"]
        wasm = tasks["prepare-wasm-image"]["run"]
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
        config = tomllib.loads((ROOT / "mise.toml").read_text())
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

        ci = tomllib.loads((ROOT / "mise.ci.toml").read_text())
        task_tools = {name: task.get("tools", {}) for name, task in ci["tasks"].items()}
        self.assertIn("rustfmt", task_tools["format-rust"]["rust"]["components"])
        self.assertEqual(task_tools["format-toml"]["taplo"], "0.10.0")
        self.assertEqual(task_tools["workflow-lint"]["aqua:rhysd/actionlint"], "1.7.7")
        self.assertIn("clippy", task_tools["ci:host"]["rust"]["components"])

    def test_mise_configuration_pins_project_toolchains_and_core_tasks(self):
        config = tomllib.loads((ROOT / "mise.toml").read_text())
        self.assertNotIn("tools", config)
        self.assertNotIn("XILA_RUST_TOOLCHAIN", config["env"])
        self.assertEqual(config["env"]["XILA_NIGHTLY_TOOLCHAIN"], "nightly-2026-10-12")
        for task in ["host-check", "guest-check", "localization-check", "prepare-native-image", "prepare-wasm-image"]:
            with self.subTest(task=task):
                self.assertIn(task, config["tasks"])

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
        self.assertIn("mise run install-nightly", readme)

    def test_ci_environment_keeps_tools_scoped_to_individual_tasks(self):
        ci = tomllib.loads((ROOT / "mise.ci.toml").read_text())
        self.assertNotIn("tools", ci)
        task_tools = {name: task["tools"] for name, task in ci["tasks"].items() if "tools" in task}
        self.assertIn("taplo", task_tools["format-toml"])
        self.assertIn("rust", task_tools["ci:host"])
        self.assertIn("wasm32-wasip1", task_tools["ci:wasm"]["rust"]["targets"])
        self.assertIn("aqua:taiki-e/cargo-llvm-cov", task_tools["ci:coverage"])
        self.assertIn("cargo:trunk", task_tools["pages:build"])
        self.assertEqual({"mise.ci.toml", "mise.development.toml"}, {path.name for path in ROOT.glob("mise.*.toml")})

        workflows = list((ROOT / ".github/workflows").glob("*.yml"))
        for workflow in workflows:
            if "MISE_ENV:" in workflow.read_text():
                self.assertRegex(workflow.read_text(), r"(?m)^\s+MISE_ENV: ci$", workflow.name)

    def test_ci_native_packages_are_scoped_to_workflows_not_global_bootstrap(self):
        ci = tomllib.loads((ROOT / "mise.ci.toml").read_text())
        self.assertNotIn("bootstrap", ci)
        workflows = "\n".join(path.read_text() for path in (ROOT / ".github/workflows").glob("*.yml"))
        self.assertNotIn("mise bootstrap packages apply", workflows)
        self.assertIn("libasound2-dev", job("ci.yml", "host"))
        self.assertNotIn("libasound2-dev", job("ci.yml", "wasm"))
        self.assertIn("libasound2-dev", job("ci.yml", "localization"))
        self.assertIn("libasound2-dev", job("extended.yml", "coverage"))
        self.assertNotIn("libasound2-dev", job("extended.yml", "abi-deep"))
        self.assertIn("libasound2-dev", job("pages.yml", "build"))
        self.assertRegex(job("ci.yml", "abi"), r"apt-get install[^\n]*clang mold")
        self.assertRegex(job("extended.yml", "abi-deep"), r"apt-get install[^\n]*clang mold")

    def test_github_actions_use_version_tags_instead_of_commit_shas(self):
        for workflow in (ROOT / ".github/workflows").glob("*.yml"):
            for line_number, line in enumerate(workflow.read_text().splitlines(), 1):
                if "uses:" in line:
                    with self.subTest(workflow=workflow.name, line=line_number):
                        self.assertNotRegex(line, r"uses:\s+[^\s]+@[0-9a-f]{40}\b")


if __name__ == "__main__":
    unittest.main()
