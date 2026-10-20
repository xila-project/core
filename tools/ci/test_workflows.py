"""Small policy regressions; actionlint remains the workflow syntax validator."""

import pathlib
import re
import unittest

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

    def test_all_host_tools_sources_select_host_checks(self):
        filters = job("check.yml", "changes")
        host = filters.split("            host:\n")[1].split("            guest:\n")[0]
        self.assertIn("'tools/**'", host)

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

    def test_zstd_is_available_before_rust_cache(self):
        text = (ROOT / ".github/actions/setup/action.yml").read_text()
        self.assertIn("uses: ./.github/actions/cache-tools", text)
        self.assertLess(text.index("uses: ./.github/actions/cache-tools"), text.index("id: rust-cache"))


if __name__ == "__main__":
    unittest.main()
