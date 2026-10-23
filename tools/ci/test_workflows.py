"""Small policy regressions; actionlint remains the workflow syntax validator."""

import pathlib
import re
import tomllib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]


def job(workflow, name):
    text = (ROOT / ".github/workflows" / workflow).read_text()
    return re.search(rf"^  {name}:\n(.*?)(?=^  [\w-]+:|\Z)", text, re.M | re.S)[1]


class WorkflowPolicy(unittest.TestCase):
    def test_mise_configuration_pins_project_toolchains_and_core_tasks(self):
        config = tomllib.loads((ROOT / "mise.toml").read_text())
        self.assertEqual(config["tools"]["rust"]["version"], "1.99.0")
        self.assertEqual(config["env"]["XILA_NIGHTLY_TOOLCHAIN"], "nightly-2026-10-12")
        self.assertEqual(config["tools"]["node"], "24.22.0")
        self.assertEqual(config["tools"]["aqua:cargo-bins/cargo-binstall"], "1.15.7")
        for task in ["host-check", "guest-check", "localization-check", "prepare-native-image", "prepare-wasm-image"]:
            with self.subTest(task=task):
                self.assertIn(task, config["tasks"])

    def test_setup_action_installs_mise_tools_and_can_add_nightly_components(self):
        setup = (ROOT / ".github/actions/setup/action.yml").read_text()
        self.assertIn("uses: jdx/mise-action@8418fedb1110381a0d3ba51a7a415d7c1b884550", setup)
        self.assertIn("nightly-toolchain:", setup)
        self.assertIn("mise run install-nightly", setup)
        self.assertNotIn("source development/toolchains.env", setup)

    def test_mise_bootstraps_runner_system_packages(self):
        config = tomllib.loads((ROOT / "mise.toml").read_text())
        packages = config["bootstrap"]["packages"]
        for package in ["clang", "mold", "pkg-config", "libasound2-dev", "libudev-dev", "libxkbcommon-dev", "libwayland-dev", "libx11-dev", "libxkbcommon-x11-dev"]:
            with self.subTest(package=package):
                self.assertEqual(packages[f"apt:{package}"], "latest")
        setup = (ROOT / ".github/actions/setup/action.yml").read_text()
        self.assertIn("mise bootstrap packages apply --manager apt --yes", setup)
        self.assertNotIn("sudo apt-get install", setup)

    def test_check_workflow_uses_mise_without_the_development_container(self):
        check = (ROOT / ".github/workflows/check.yml").read_text()
        self.assertNotIn("container:", check)
        self.assertNotIn("cargo make", check)
        self.assertIn("'mise.toml'", check)
        self.assertIn("'tools/mise/**'", check)
        self.assertNotIn("'**/Makefile.toml'", check)
        self.assertIn("            mise:", check)
        self.assertNotIn("            makefile:", check)
        self.assertIn("mise run install-nightly", (ROOT / ".github/actions/setup/action.yml").read_text())

    def test_deploy_and_cache_benchmark_use_hosted_mise_setup(self):
        deploy = (ROOT / ".github/workflows/deploy.yml").read_text()
        benchmark = (ROOT / ".github/workflows/cache-benchmark.yml").read_text()
        self.assertNotIn("container:", deploy)
        self.assertNotIn("cargo make", deploy)
        self.assertIn("mise run host-docs", deploy)
        self.assertIn("mise run guest-docs", deploy)
        self.assertIn("mise run prepare-wasm-image", deploy)
        self.assertIn('nightly-toolchain: "true"', deploy)
        self.assertNotIn("container:", benchmark)
        self.assertIn("jdx/mise-action@", benchmark)
        self.assertIn("mise bootstrap packages apply --manager apt --yes", benchmark)
        self.assertNotIn("sudo apt-get install", benchmark)

    def test_local_developer_docs_use_mise_and_no_development_container(self):
        readme = (ROOT / "readme.md").read_text()
        agents = (ROOT / "AGENTS.md").read_text()
        for document in [readme, agents]:
            self.assertNotIn("cargo make", document)
            self.assertNotIn("development-environment", document)
            self.assertIn("mise install", document)
        runtime = (ROOT / ".github/workflows/container.yml").read_text()
        self.assertIn("'mise.toml'", runtime)
        self.assertIn("'tools/mise/**'", runtime)
        self.assertNotIn("'Makefile.toml'", runtime)

    def test_seed_image_tasks_limit_removal_to_generated_seed_images(self):
        native = (ROOT / "tools/mise/prepare-native-image.sh").read_text()
        wasm = (ROOT / "tools/mise/prepare-wasm-image.sh").read_text()
        self.assertIn("rm -f target/seed/native/seed.img", native)
        self.assertIn("--bundle native", native)
        self.assertIn("rm -f target/seed/wasm/seed.img", wasm)
        self.assertIn("--bundle wasm", wasm)
        self.assertNotRegex(native + wasm, r"rm\s+-rf\s+(?!target/seed)")

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
        shared = re.search(r"          shared-key: (.*)", text)[1]
        self.assertIn("github.sha", shared)

    def test_python_ci_tools_select_their_own_regressions(self):
        filters = job("check.yml", "changes")
        formatting = filters.split("            format:\n")[1].split("            host:\n")[0]
        self.assertIn("'tools/ci/**'", formatting)

    def test_zstd_is_available_before_rust_cache(self):
        text = (ROOT / ".github/actions/setup/action.yml").read_text()
        self.assertIn("uses: ./.github/actions/cache-tools", text)
        self.assertLess(text.index("uses: ./.github/actions/cache-tools"), text.index("id: rust-cache"))
        cache_tools = (ROOT / ".github/actions/cache-tools/action.yml").read_text()
        self.assertIn("sudo apt-get install", cache_tools)


if __name__ == "__main__":
    unittest.main()
