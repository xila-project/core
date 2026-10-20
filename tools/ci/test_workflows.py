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

    def test_node_setup_is_opt_out_because_fonts_need_it(self):
        text = (ROOT / ".github/actions/setup/action.yml").read_text()
        self.assertRegex(text, r"node-dependencies:\n(?:.*\n)*?    default: \"true\"")
        for name in ["npm cache", "Report npm cache", "npm dependencies"]:
            step = text.split(f"- name: Setup | {name}\n")[1].split("\n    - name:")[0]
            self.assertIn("if: inputs.node-dependencies == 'true'", step)

    def test_required_check_names_remain_stable(self):
        for name in ["Guest checks", "Format checks", "Host checks (Clippy)", "Host checks (Tests)", "Host checks (Audio, docs, build)"]:
            self.assertIn(f"    name: {name}\n", (ROOT / ".github/workflows/check.yml").read_text())


if __name__ == "__main__":
    unittest.main()
