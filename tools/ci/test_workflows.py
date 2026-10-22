"""Small policy regressions; actionlint remains the workflow syntax validator."""

import pathlib
import re
import unittest
import os
import subprocess
import tempfile
import textwrap

ROOT = pathlib.Path(__file__).resolve().parents[2]


def job(workflow, name):
    text = (ROOT / ".github/workflows" / workflow).read_text()
    return re.search(rf"^  {name}:\n(.*?)(?=^  [\w-]+:|\Z)", text, re.M | re.S)[1]


class WorkflowPolicy(unittest.TestCase):
    def test_extended_checks_override_skipped_ancestors_but_require_success(self):
        for name, dependency in [("host-coverage", "host-test"), ("wasm-extended", "wasm-translation")]:
            with self.subTest(job=name):
                condition = re.search(r"^    if: (.*)$", job("check.yml", name), re.M)[1]
                self.assertIn("!cancelled()", condition)
                self.assertIn("github.event_name != 'pull_request'", condition)
                self.assertIn(f"needs.{dependency}.result == 'success'", condition)

    def test_container_cancels_superseded_prs_only(self):
        text = (ROOT / ".github/workflows/container.yml").read_text()
        self.assertRegex(text, r"concurrency:\n  group: .*github.workflow.*github.ref")
        self.assertIn("cancel-in-progress: ${{ github.event_name == 'pull_request' }}", text)

    def test_ci_measurement_tools_are_not_runtime_image_inputs(self):
        self.assertIn("tools/ci", (ROOT / ".dockerignore").read_text().splitlines())
        workflow = (ROOT / ".github/workflows/container.yml").read_text()
        self.assertEqual(workflow.count("      - '!tools/ci/**'"), 2)

    def test_all_host_tools_sources_select_host_checks(self):
        filters = job("check.yml", "changes")
        host = filters.split("            host:\n")[1].split("            guest:\n")[0]
        # Preserve fs_image/profiling and future product tools, but CI-only
        # Python analysis has its own format-job regression suite.
        self.assertIn("'tools/!(ci)/**'", host)

    def test_abi_jobs_do_not_install_node_dependencies(self):
        for name in ["wasm-translation", "wasm-extended"]:
            with self.subTest(job=name):
                self.assertIn('node-dependencies: "false"', job("check.yml", name))

    def test_abi_setup_does_not_resolve_unrelated_workspace_lockfiles(self):
        for name in ["wasm-translation", "wasm-extended"]:
            self.assertIn("lockfile-manifests: executables/wasm/tests/translation/Cargo.toml", job("check.yml", name))
        setup = (ROOT / ".github/actions/setup/action.yml").read_text()
        self.assertIn("LOCKFILE_MANIFESTS: ${{ inputs.lockfile-manifests }}", setup)
        self.assertIn("tools/profiling/Cargo.toml", job("check.yml", "host-test"))

    def test_abi_default_test_keeps_doctests_without_running_them_twice(self):
        text = job("check.yml", "wasm-translation")
        self.assertEqual(text.count("test --locked"), 1)
        self.assertNotIn("--lib", text)
        self.assertNotIn("--tests", text)

    def test_node_setup_is_opt_out_because_fonts_need_it(self):
        text = (ROOT / ".github/actions/setup/action.yml").read_text()
        self.assertRegex(text, r"node-dependencies:\n(?:.*\n)*?    default: \"true\"")
        for name in ["npm cache", "Report npm cache", "npm dependencies"]:
            step = text.split(f"- name: Setup | {name}\n")[1].split("\n    - name:")[0]
            self.assertIn("if: inputs.node-dependencies == 'true'", step)

    def test_required_check_names_remain_stable(self):
        for name in ["Guest checks", "Format checks", "Host checks (Clippy)", "Host checks (Tests)", "Host checks (Audio, docs, build)"]:
            self.assertIn(f"    name: {name}\n", (ROOT / ".github/workflows/check.yml").read_text())

    def test_cache_experiments_are_manual_isolated_and_source_keyed(self):
        text = (ROOT / ".github/workflows/cache-benchmark.yml").read_text()
        self.assertIn("  workflow_dispatch:", text)
        self.assertNotIn("  pull_request:", text)
        self.assertNotIn("  push:", text)
        self.assertIn("cache-workspace-crates: ${{ matrix.strategy == 'workspace' }}", text)
        self.assertIn("github.sha", text)
        self.assertIn("prefix-key: experiment-", text)
        self.assertIn("SCCACHE_GHA_VERSION:", text)
        self.assertIn("max-parallel: 1", text)
        shared = re.search(r"          shared-key: (.*)", text)[1]
        self.assertIn("github.sha", shared)

    def test_python_ci_tools_select_their_own_regressions(self):
        filters = job("check.yml", "changes")
        formatting = filters.split("            format:\n")[1].split("            host:\n")[0]
        self.assertIn("'tools/ci/**'", formatting)

    def test_new_benchmark_actions_are_pinned_and_report_archive_hit(self):
        text = (ROOT / ".github/workflows/cache-benchmark.yml").read_text()
        for action in re.findall(r"uses: ([^\s]+)", text):
            if not action.startswith("./"):
                self.assertRegex(action, r"@[0-9a-f]{40}$")
        self.assertIn("steps.rust-cache.outputs.cache-hit", text)

    def test_zstd_is_available_before_rust_cache(self):
        text = (ROOT / ".github/actions/setup/action.yml").read_text()
        self.assertIn("uses: ./.github/actions/cache-tools", text)
        self.assertLess(text.index("uses: ./.github/actions/cache-tools"), text.index("id: rust-cache"))

    def test_optional_compression_install_failure_does_not_skip_validation(self):
        action = (ROOT / ".github/actions/cache-tools/action.yml").read_text()
        script = textwrap.dedent(action.split("      run: |\n")[1])
        with tempfile.TemporaryDirectory() as directory:
            apt = pathlib.Path(directory) / "apt-get"
            apt.write_text("#!/bin/sh\nexit 1\n")
            apt.chmod(0o755)
            result = subprocess.run(["/bin/bash", "-e", "-c", script], env={**os.environ, "PATH": directory}, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0)
        self.assertIn("::warning::", result.stdout)

    def test_nextest_comparison_does_not_charge_cargo_for_doctests_twice(self):
        text = job("check.yml", "nextest-benchmark")
        execution = text.split("      - name: Benchmark | cargo test execution\n")[1].split("      - name:")[0]
        self.assertIn("cargo test --workspace --tests", execution)
        self.assertIn("cargo test --workspace --doc --timings", text)

    def test_coverage_upload_remains_required_and_fail_closed(self):
        text = job("check.yml", "host-coverage")
        self.assertIn("uses: codecov/codecov-action@v7", text)
        self.assertIn("fail_ci_if_error: true", text)
        self.assertNotIn("continue-on-error: true", text)

    def test_cold_localization_can_finish_all_seven_locales(self):
        text = job("check.yml", "localization")
        self.assertGreaterEqual(int(re.search(r"timeout-minutes: (\d+)", text)[1]), 40)
        self.assertIn("locales=(en fr es de pt-BR it pl)", text)

    def test_localization_path_filters_cover_catalog_code_translation_and_each_locale(self):
        changes = job("check.yml", "changes")
        localization = changes.split("            localization:\n")[1].split("            i18n_code:\n")[0]
        for pattern in ["**/*.ftl", "**/locales/**/*.json", "**/locale/**", "**/i18n/**", "**/localization/**", "executables/wasm/src/host/translation/**", "executables/wasm/tests/translation/**"]:
            self.assertIn(pattern, localization)
        for locale in ["en", "fr", "es", "de", "pt-BR", "it", "pl"]:
            key = "pt_br" if locale == "pt-BR" else locale
            self.assertIn(f"i18n_{key}:\n              - '**/locales/{locale}.json'", changes)
        self.assertIn("EMBEDDED_I18N_STRICT: \"1\"", job("check.yml", "localization"))

    def test_localization_build_code_forces_every_locale_on_prs(self):
        changes = job("check.yml", "changes")
        i18n_code = changes.split("            i18n_code:\n")[1].split("            rust:\n")[0]
        for pattern in ["**/Cargo.{toml,lock}", "**/build.rs", "tolgee.extractor.mjs", "subsystems/graphics/fonts_generator/**"]:
            self.assertIn(pattern, i18n_code)
        workflow = (ROOT / ".github/workflows/check.yml").read_text()
        self.assertIn("I18N_CODE_CHANGED: ${{ needs.changes.outputs.i18n_code == 'true' || needs.changes.outputs.translation_code == 'true' }}", workflow)
        script = job("check.yml", "localization")
        self.assertIn('locales=(en fr es de pt-BR it pl)', script)
        self.assertIn('"$I18N_CODE_CHANGED" == "false"', script)
        self.assertIn("Checking locale(s):", script)


if __name__ == "__main__":
    unittest.main()
