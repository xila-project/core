"""Focused behavioral policy regressions for the current CI workflows."""

import os
import functools
import json
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


@functools.lru_cache
def configured_tasks(environment):
    """Resolve task inheritance and tools with Mise (scripts render at execution)."""
    result = subprocess.check_output(
        ["mise", "tasks", "ls", "--json"], cwd=ROOT, text=True,
        env=os.environ | {"MISE_ENV": environment},
    )
    tasks = {}
    for task in json.loads(result):
        task["run"] = "\n".join(task["run"])
        if not task.get("depends"):
            task.pop("depends", None)
        tasks[task["name"]] = task
    return tasks


@functools.lru_cache
def ci_config():
    config = tomllib.loads((ROOT / "mise.ci.toml").read_text())
    resolved = configured_tasks("ci")
    return config | {"tasks": {name: resolved[name] for name in config["tasks"]}}


class WorkflowPolicy(unittest.TestCase):
    def test_localization_checks_share_one_target_directory(self):
        script = ci_config()["tasks"]["ci:localization"]["run"]
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            mise = root / "mise"
            mise.write_text('#!/bin/sh\nprintf "%s|%s|%s|%s\\n" "$EMBEDDED_I18N_LOCALE" "$CARGO_TARGET_DIR" "$EMBEDDED_I18N_STRICT" "$*" >> "$CALLS"\n')
            mise.chmod(0o755)
            calls = root / "calls"
            env = os.environ | {
                "PATH": f"{directory}:{os.environ['PATH']}",
                "CALLS": str(calls), "CI_LOCALES": "en fr pl",
                "CARGO_TARGET_ROOT": str(root / "target/localization"),
            }
            subprocess.run(["bash", "-c", script], env=env, check=True, capture_output=True)
            records = [line.split('|') for line in calls.read_text().splitlines()]
            self.assertEqual([row[0] for row in records], ["en", "fr", "pl"])
            self.assertEqual({row[1] for row in records}, {str(root / "target/localization")})
            self.assertEqual({row[2] for row in records}, {"1"})
            self.assertEqual({row[3] for row in records}, {"run localization:check"})

    def test_font_regressions_have_pinned_rust_and_node_tools(self):
        for environment in ["ci", "development"]:
            with self.subTest(environment=environment):
                task = configured_tasks(environment)["test:fonts"]
                self.assertIn("rustfmt", task["tools"]["rust"]["components"])
                self.assertEqual(task["tools"]["node"], "24.22.0")
        task = ci_config()["tasks"]["ci:host-font-tests"]
        self.assertIn("mise run test:fonts", task["run"])
        self.assertIn("cargo test --manifest-path subsystems/graphics/tests/font_link/Cargo.toml", task["run"])

    def test_regular_ci_uses_headless_audio_instead_of_native_audio_specific_checks(self):
        host = job("ci.yml", "host")
        tasks = ci_config()["tasks"]
        self.assertNotIn("ci:host-audio-clippy", tasks)
        self.assertNotIn("ci:host-audio-tests", tasks)
        self.assertNotIn("host-audio", host)
        lint = tomllib.loads((ROOT / "mise.toml").read_text())["tasks"]["clippy:host"]["run"]
        self.assertIn("--package xila-audio", lint)
        self.assertIn("--package xila-drivers-headless", lint)
        self.assertNotIn("drivers_native/audio", lint)
        self.assertIn("cargo test --locked --workspace", tasks["ci:host-tests"]["run"])
        self.assertNotIn("apt:libasound2-dev", ci_config()["bootstrap"]["packages"])

    def test_tool_versions_are_central_and_templates_do_not_install_tools_globally(self):
        shared = tomllib.loads((ROOT / "mise.toml").read_text())
        self.assertIn("rust_version", shared.get("vars", {}))
        self.assertIn("nightly_version", shared["vars"])
        ci = tomllib.loads((ROOT / "mise.ci.toml").read_text())
        self.assertIn("task_templates", ci)
        self.assertNotIn("tools", shared)
        self.assertNotIn("tools", ci)
        for filename in ["mise.ci.toml", "mise.development.toml"]:
            text = (ROOT / filename).read_text()
            for version in shared["vars"].values():
                self.assertNotIn(f'"{version}"', text, filename)
        self.assertNotIn("depends", ci["task_templates"]["ci:rust-format"])
        self.assertNotIn("depends", ci["task_templates"]["ci:node"])

    def test_formatting_resolves_only_its_own_tools_in_both_environments(self):
        for environment, cases in {
            "ci": {"ci:format-rust": {"rust"}, "ci:format-toml": {"taplo"}, "ci:quality-js": {"node"}},
            "development": {"format:rust": {"rust"}, "format:toml": {"taplo"}, "format:json": {"node"}},
        }.items():
            for name, expected_tools in cases.items():
                with self.subTest(environment=environment, task=name):
                    task = configured_tasks(environment)[name]
                    self.assertEqual(set(task["tools"]), expected_tools)
                    self.assertFalse(task.get("depends"))
                    if "rust" in expected_tools:
                        self.assertEqual(task["tools"]["rust"]["components"], ["rustfmt"])
                        self.assertFalse(task["tools"]["rust"].get("targets"))
        tasks = configured_tasks("ci")
        self.assertEqual(tasks["ci:abi"]["tools"]["rust"], {"version": "1.99.0", "profile": "minimal"})
        self.assertEqual(set(tasks["ci:wasm-browser-tests"]["tools"]), {"node"})
        self.assertNotIn("tools", tomllib.loads((ROOT / "mise.development.toml").read_text()))

    def test_validation_jobs_call_the_same_ordered_leaf_tasks_as_local_wrappers(self):
        tasks = ci_config()["tasks"]
        groups = {
            ("ci.yml", "host"): ["ci:host-clippy", "ci:host-wasm-clippy", "ci:host-tests", "ci:firmware", "ci:host-profiling-tests", "ci:host-font-tests", "ci:host-build"],
            ("ci.yml", "wasm"): ["ci:wasm-clippy", "ci:wasm-audio-check", "ci:wasm-apps-clippy", "ci:wasm-image", "ci:wasm-browser-tests"],
            ("extended.yml", "profiling"): ["ci:profiling-runtime-tests", "ci:profiling-native-tests"],
            ("extended.yml", "abi-deep"): ["ci:abi-coverage", "ci:abi-miri", "ci:abi-miri-cross"],
        }
        for (workflow, group), leaves in groups.items():
            with self.subTest(group=group):
                wrapper = tasks[f"ci:{group}"]["run"]
                self.assertEqual(re.findall(r"^mise run ([\w:-]+)$", wrapper, re.M), leaves)
                self.assertNotIn("cargo ", wrapper)
                steps = re.findall(r"^      - name: .+\n        run: mise run ([\w:-]+)$", job(workflow, group), re.M)
                self.assertEqual(steps, leaves)
                for leaf in leaves:
                    task = tasks[leaf]
                    self.assertIn("run", task)
                    if leaf == "ci:wasm-browser-tests":
                        self.assertIn("node", task["tools"])
                        self.assertNotIn("depends", task)
                    else:
                        self.assertIn("rust", task["tools"])
                        self.assertIn("depends", task)

                # Exercise the wrapper shell without compiling or installing packages.
                # A failing leaf must prevent every subsequent leaf from running.
                with tempfile.TemporaryDirectory() as directory:
                    directory = pathlib.Path(directory)
                    mise = directory / "mise"
                    mise.write_text('#!/bin/sh\nprintf "%s\\n" "$2" >> "$CALLS"\n[ "$2" != "$FAIL_TASK" ]\n')
                    mise.chmod(0o755)
                    calls = directory / "calls"
                    env = os.environ | {"PATH": f"{directory}:{os.environ['PATH']}", "CALLS": str(calls)}
                    for failure in ["", leaves[1]]:
                        calls.write_text("")
                        result = subprocess.run(["bash", "-c", wrapper], env=env | {"FAIL_TASK": failure}, capture_output=True, text=True)
                        self.assertEqual(result.returncode, 1 if failure else 0, result.stderr)
                        self.assertEqual(calls.read_text().splitlines(), leaves[:2] if failure else leaves)

    def test_task_names_are_namespaced_and_developer_tasks_are_environment_scoped(self):
        shared = tomllib.loads((ROOT / "mise.toml").read_text())["tasks"]
        development = tomllib.loads((ROOT / "mise.development.toml").read_text()).get("tasks", {})
        ci = ci_config()["tasks"]
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
        ci = ci_config()
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
            "ci.yml": {"host": "target", "wasm": "target", "abi": "target", "localization": "objects"},
            "extended.yml": {"coverage": "objects", "profiling": "target", "abi-deep": "objects"},
            "pages.yml": {"build": "objects"},
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

    def test_host_target_cache_includes_standalone_test_crates(self):
        self.assertIn("CARGO_TARGET_DIR: ${{ github.workspace }}/target", job("ci.yml", "host"))

    def test_abi_target_cache_uses_translation_workspace(self):
        self.assertIn("working-directory: executables/wasm/tests/translation", job("ci.yml", "abi"))

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
                text = job(workflow, name)
                self.assertIn('toolchain: ${{ steps.toolchain.outputs.version }}', text)
                self.assertIn("run: mise run ci:toolchain", text)
        self.assertIn("run: mise run ci:toolchain-nightly", job("extended.yml", "abi-deep"))
        self.assertIn('toolchain: ${{ steps.toolchain.outputs.version }}', job("extended.yml", "abi-deep"))
        pins = tomllib.loads((ROOT / "mise.toml").read_text())["vars"]
        for task_name, pin in [("ci:toolchain", "rust_version"), ("ci:toolchain-nightly", "nightly_version")]:
            task = configured_tasks("ci")[task_name]
            self.assertEqual(task["tools"]["rust"], {"version": pins[pin], "profile": "minimal"})
            self.assertIn('echo "version={{ vars.' + pin + ' }}"', task["run"])

    def test_quality_does_not_enable_compilation_cache(self):
        quality = job("ci.yml", "quality")
        self.assertNotIn("XILA_MBX", quality)
        self.assertNotIn("mr-boxington-action", quality)

    def test_workflow_lint_includes_shellcheck_on_local_and_ci_runs(self):
        tasks = ci_config()["tasks"]
        self.assertIn("aqua:koalaman/shellcheck", tasks["ci:lint-workflows"]["tools"])

    def test_binding_build_tasks_install_rustfmt_even_with_minimal_toolchains(self):
        tasks = ci_config()["tasks"]
        for name in [
            "ci:host", "ci:wasm", "ci:localization", "ci:coverage", "ci:profiling",
            "ci:benchmark-profile", "ci:benchmark-compile", "ci:benchmark-cargo-test",
            "ci:benchmark-nextest", "ci:benchmark-doctests", "pages:build", "pages:demo",
            "clippy:host", "clippy:guest", "docs:host", "docs:guest",
            "build:applications", "image:native", "image:wasm",
            "ci:host-clippy", "ci:host-wasm-clippy",
            "ci:host-tests", "ci:host-profiling-tests", "ci:host-font-tests",
            "ci:host-build", "ci:wasm-clippy",
            "ci:wasm-audio-check", "ci:wasm-apps-clippy", "ci:wasm-image",
            "ci:profiling-runtime-tests", "ci:profiling-native-tests",
        ]:
            with self.subTest(task=name):
                rust = tasks[name]["tools"]["rust"]
                components = rust.get("components", []) if isinstance(rust, dict) else []
                self.assertIn("rustfmt", components, "wasm build scripts execute rustfmt on generated bindings")

    def test_host_tests_install_wasi_for_nested_guest_builds(self):
        rust = ci_config()["tasks"]["ci:host-tests"]["tools"]["rust"]
        self.assertIn("wasm32-wasip1", rust.get("targets", []))

    def test_font_link_fixture_can_generate_its_ignored_lockfile(self):
        task = ci_config()["tasks"]["ci:host-font-tests"]
        self.assertNotIn("--locked", task["run"])

    def test_image_tasks_do_not_use_bash_only_options_with_mise_default_shell(self):
        tasks = tomllib.loads((ROOT / "mise.toml").read_text())["tasks"]
        for name in ["image:native", "image:wasm"]:
            with self.subTest(task=name):
                self.assertNotIn("pipefail", tasks[name]["run"])

    def test_no_workflow_keeps_superseded_rust_caches(self):
        for workflow in ACTIVE_WORKFLOWS:
            text = workflow.read_text()
            self.assertNotIn("sccache", text.lower(), workflow.name)
            self.assertNotIn("Swatinem/rust-cache", text, workflow.name)

    def test_primary_ci_jobs_are_parallel_and_required_aggregates_results(self):
        ci = (ROOT / ".github/workflows/ci.yml").read_text()
        self.assertIn("workflow_call:", ci)
        primary = ["quality", "verification", "host", "wasm", "abi", "localization"]
        for name in primary:
            with self.subTest(job=name):
                self.assertNotRegex(job("ci.yml", name), r"(?m)^    needs:")

        required = job("ci.yml", "required")
        self.assertIn("if: always()", required)
        self.assertIn('if [[ "$result" != success ]]', required)
        dependencies = re.search(r"(?m)^    needs: \[(.*?)\]", required)[1]
        self.assertEqual(set(dependencies.split(", ")), set(primary))
        for name in primary:
            with self.subTest(required_job=name):
                self.assertIn(f"needs.{name}.result", required)

    def test_verification_failure_blocks_required_check(self):
        required = job("ci.yml", "required")
        script = textwrap.dedent(required.split("        run: |\n", 1)[1])
        bindings = re.findall(r"([A-Z_]+): \$\{\{ needs\.([\w-]+)\.result \}\}", required)
        for status in ["success", "failure", "cancelled", "skipped"]:
            with self.subTest(verification_result=status):
                env = os.environ | {name: "success" for name, _ in bindings}
                env["VERIFICATION_RESULT"] = status
                result = subprocess.run(["bash", "-c", script], env=env, capture_output=True, text=True)
                self.assertEqual(result.returncode, 0 if status == "success" else 1, result.stdout + result.stderr)

    def test_primary_abi_matrix_checks_both_memory_modes_on_every_event(self):
        abi = job("ci.yml", "abi")
        self.assertIn("memory_32", abi)
        self.assertIn("memory_64", abi)
        self.assertNotIn("github.event_name == 'pull_request'", abi)
        self.assertIn("ci:abi", abi)

    def test_localization_job_selects_catalogs_and_runs_all_locales_for_shared_changes(self):
        localization = job("ci.yml", "localization")
        self.assertIn("CI_LOCALES", localization)
        self.assertIn("mise run ci:localization", localization)
        self.assertIn('"$EVENT_NAME" != pull_request', localization)
        self.assertIn("all_locales=true", localization)
        self.assertIn("locales=(en fr es de pt-BR it pl zh-CN)", localization)
        self.assertIn("locales=(en)", localization)

    def test_locale_selection_handles_build_scripts_catalogs_and_unrelated_changes(self):
        localization = job("ci.yml", "localization")
        script = textwrap.dedent(re.search(
            r"(?ms)^      - name: Select locales\n.*?^        run: \|\n(.*?)(?=^      - )",
            localization,
        )[1])
        for changed, expected in [
            ("subsystems/graphics/build.rs", "en fr es de pt-BR it pl zh-CN"),
            ("subsystems/graphics/build/catalogs.rs", "en fr es de pt-BR it pl zh-CN"),
            ("executables/clock/locales/fr.json", "en fr"),
            ("executables/clock/locales/zh-CN.json", "en zh-CN"),
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
        tasks = ci_config()["tasks"]
        deep_task = tasks["ci:abi-coverage"]["run"] + tasks["ci:abi-miri-cross"]["run"]
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

    def test_runtime_pr_build_only_and_main_release_validate_image(self):
        runtime = (ROOT / ".github/workflows/runtime.yml").read_text()
        self.assertIn("load: ${{ github.event_name != 'pull_request' }}", runtime)
        self.assertIn("outputs: ${{ github.event_name == 'pull_request' && 'type=cacheonly' || '' }}", runtime)
        self.assertIn(
            "cache-to: ${{ github.event_name != 'pull_request' && "
            "'type=gha,mode=max,scope=xila-runtime-linux-amd64,ignore-error=true' || '' }}",
            runtime,
        )
        for step in [
            "Prepare image metadata",
            "Check image user and entrypoint",
            "Build runtime smoke-test guest",
            "Test read-only root, host mounts, state persistence, and exit status",
            "Exercise Xila CLI shell over stdin",
        ]:
            with self.subTest(step=step):
                self.assertIn(
                    f"- name: {step}\n        if: github.event_name != 'pull_request'",
                    runtime,
                )
        self.assertIn("uses: docker/build-push-action@v7", runtime)
        self.assertIn("cache-from: type=gha,scope=xila-runtime-linux-amd64", runtime)
        self.assertIn("if: github.event_name == 'workflow_run' || startsWith(github.ref, 'refs/tags/v')", runtime)
        self.assertNotIn("          load: true", runtime)

    def test_runtime_dockerfile_does_not_export_redundant_stable_toolchain_variable(self):
        dockerfile = (ROOT / "Dockerfile.runtime").read_text()
        self.assertNotIn("XILA_RUST_TOOLCHAIN", dockerfile)

    def test_pages_waits_for_main_ci_and_builds_without_incremental_restoration(self):
        pages = (ROOT / ".github/workflows/pages.yml").read_text()
        self.assertIn('workflows: ["CI"]', pages)
        self.assertIn("workflow_run:", pages)
        self.assertIn("xila-project/platforms/.github/workflows/demo.yml@", pages)
        self.assertIn("name: platform-demonstrations", pages)
        self.assertIn("path: pages/demonstrations/", pages)
        self.assertIn("mise run docs:host", pages)
        self.assertIn("mise run docs:guest", pages)
        self.assertIn("actions/upload-pages-artifact", pages)
        self.assertIn("actions/deploy-pages", pages)
        for old in ["listWorkflowRunArtifacts", "incremental=true"]:
            with self.subTest(old=old):
                self.assertNotIn(old, pages)

    def test_pages_task_builds_all_locale_images_deterministically(self):
        task = ci_config()["tasks"]["pages:build"]["run"]
        for locale in ["en", "fr", "es", "de", "pt-BR", "it", "pl"]:
            with self.subTest(locale=locale):
                self.assertIn(locale, task)
        self.assertIn("--bundle wasm", task)
        self.assertIn("--bundle native", task)
        self.assertIn("cmp ", task)
        ci_tasks = ci_config()["tasks"]
        self.assertIn("mise run pages:demo", task)
        self.assertIn("trunk build --release", ci_tasks["pages:demo"]["run"])

    def test_pages_images_and_browser_seed_follow_each_locale_target(self):
        # Run the real task shells; replace only compilation and browser tooling.
        shared = tomllib.loads((ROOT / "mise.toml").read_text())["tasks"]
        pages = tomllib.loads((ROOT / "mise.ci.toml").read_text())["tasks"]["pages:build"]["run"]
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            bin_dir = root / "bin"
            bin_dir.mkdir()
            image_script = root / "image.sh"
            image_script.write_text(shared["image:wasm"]["run"])
            cargo = bin_dir / "cargo"
            cargo.write_text(textwrap.dedent('''\
                #!/usr/bin/env python3
                import os, pathlib, sys
                args = sys.argv[1:]
                target = pathlib.Path(os.environ.get("CARGO_TARGET_DIR", "target"))
                locale = os.environ["EMBEDDED_I18N_LOCALE"]
                if args[0] == "build":
                    apps = target / "wasm32-wasip1/release"
                    apps.mkdir(parents=True, exist_ok=True)
                    for name in ["calculator", "weather", "clock", "pixel_painter", "text_editor", "synth"]:
                        (apps / (name + ".wasm")).write_text(locale)
                elif args[0] == "run":
                    output = pathlib.Path(args[args.index("--") + 1])
                    apps = pathlib.Path(args[args.index("--applications-dir") + 1])
                    assert (apps / "calculator.wasm").read_text() == locale
                    bundle = args[args.index("--bundle") + 1]
                    with output.open("x") as image:
                        image.write(locale + ":" + bundle)
                else:
                    raise AssertionError(args)
                '''))
            cargo.chmod(0o755)
            mise = bin_dir / "mise"
            mise.write_text(textwrap.dedent('''\
                #!/bin/sh
                set -eu
                case "$2" in
                  docs:host|docs:guest) ;;
                  image:wasm) sh "$IMAGE_SCRIPT" ;;
                  pages:demo)
                    test "$(cat target/seed/wasm/seed.img)" = "$EMBEDDED_I18N_LOCALE:wasm"
                    mkdir -p examples/wasm/dist
                    cp target/seed/wasm/seed.img examples/wasm/dist/seed.img
                    ;;
                  *) exit 1 ;;
                esac
                '''))
            mise.chmod(0o755)
            for target in ["x86_64-unknown-linux-gnu", "wasm32-unknown-unknown"]:
                docs = root / "target" / target / "doc"
                docs.mkdir(parents=True)
                (docs / "index.html").write_text("docs")
            env = os.environ | {
                "PATH": f"{bin_dir}:{os.environ['PATH']}",
                "IMAGE_SCRIPT": str(image_script),
                "CARGO_TARGET_ROOT": str(root / "locale targets"),
            }
            result = subprocess.run(["bash", "-c", pages], cwd=root, env=env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            for locale in ["en", "fr", "es", "de", "pt-BR", "it", "pl"]:
                for bundle in ["wasm", "native"]:
                    image = root / "target/reference-filesystem-images" / locale / f"{bundle}.img"
                    self.assertEqual(image.read_text(), f"{locale}:{bundle}")
                seed = root / "pages/demonstrations/wasm" / locale / "seed.img"
                self.assertEqual(seed.read_text(), f"{locale}:wasm")
            # Normal local builds still use target/ and replace only the seed image.
            env.pop("CARGO_TARGET_DIR", None)
            env["EMBEDDED_I18N_LOCALE"] = "en"
            result = subprocess.run(["sh", str(image_script)], cwd=root, env=env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertEqual((root / "target/seed/wasm/seed.img").read_text(), "en:wasm")

    def test_image_generation_tasks_are_self_contained_and_limit_cleanup(self):
        tasks = tomllib.loads((ROOT / "mise.toml").read_text())["tasks"]
        native = tasks["image:native"]["run"]
        wasm = tasks["image:wasm"]["run"]
        self.assertNotIn("tools/mise/prepare-", native + wasm)
        self.assertIn("rm -f target/seed/native/seed.img", native)
        self.assertIn('rm -f "${CARGO_TARGET_DIR:-target}/seed/wasm/seed.img"', wasm)
        self.assertNotRegex(native + wasm, r"rm\s+-rf")

    def test_benchmark_workflows_are_not_present(self):
        self.assertFalse((ROOT / ".github/workflows/benchmarks.yml").exists())
        self.assertFalse((ROOT / ".github/workflows/cache-benchmark.yml").exists())

    def test_ci_task_groups_and_quality_environment_are_defined(self):
        config = ci_config()
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

        ci = ci_config()
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
        tasks = ci_config()["tasks"]
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

    def test_development_mise_environment_is_default_with_task_scoped_mbx(self):
        default = tomllib.loads((ROOT / ".miserc.toml").read_text())
        self.assertEqual(default["env"], ["development"])
        development = tomllib.loads((ROOT / "mise.development.toml").read_text())
        self.assertNotIn("tools", development)
        tasks = configured_tasks("development")
        for name in ["check:host", "check:guest", "clippy:host", "clippy:guest", "build:cargo", "image:wasm"]:
            with self.subTest(task=name):
                self.assertTrue(tasks[name]["tools"]["rust"]["mr_boxington"])
                self.assertIn("mr-boxington", tasks[name]["tools"])
        self.assertIn("aqua:taiki-e/cargo-llvm-cov", tasks["coverage:host"]["tools"])

    def test_developer_setup_documents_optional_task_tool_preinstall(self):
        readme = (ROOT / "readme.md").read_text()
        self.assertIn("mise install --include-task-tools", readme)
        self.assertNotIn("mise run install-nightly", readme)

    def test_ci_environment_keeps_tools_scoped_to_individual_tasks(self):
        ci = ci_config()
        self.assertNotIn("tools", ci)
        task_tools = {name: task["tools"] for name, task in ci["tasks"].items() if "tools" in task}
        self.assertIn("taplo", task_tools["ci:format-toml"])
        self.assertIn("rust", task_tools["ci:host"])
        self.assertIn("wasm32-wasip1", task_tools["ci:wasm"]["rust"]["targets"])
        self.assertIn("aqua:taiki-e/cargo-llvm-cov", task_tools["ci:coverage"])
        self.assertEqual(task_tools["pages:demo"]["github:trunk-rs/trunk"], "0.21.14")
        self.assertNotIn("cargo:trunk", task_tools["pages:demo"])
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
        for package in ["pkg-config", "libudev-dev", "libxkbcommon-dev", "libwayland-dev", "libx11-dev", "libxkbcommon-x11-dev"]:
            self.assertEqual(packages[f"apt:{package}"]["env"], "host")
        self.assertEqual(packages["apt:time"]["env"], "benchmark")
        development = tomllib.loads((ROOT / "mise.development.toml").read_text())
        self.assertEqual(development["bootstrap"]["packages"]["apt:libasound2-dev"], "latest")

    def test_ci_native_packages_are_task_scoped_not_applied_by_quality(self):
        workflows = "\n".join(path.read_text() for path in ACTIVE_WORKFLOWS)
        self.assertNotIn("apt-get", workflows)
        tasks = ci_config()["tasks"]
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

    def test_mise_multiline_ci_tasks_run_with_bash(self):
        tasks = ci_config()["tasks"]
        for name in ["ci:host", "ci:wasm", "ci:localization", "ci:abi-deep", "ci:profiling", "pages:build"]:
            with self.subTest(task=name):
                self.assertEqual(tasks[name].get("shell"), "bash -c")
        ci_tasks = ci_config()["tasks"]
        self.assertEqual(ci_tasks["ci:benchmark-profile"].get("shell"), "bash -c")

    def test_github_actions_use_version_tags_instead_of_commit_shas(self):
        for workflow in ACTIVE_WORKFLOWS:
            for line_number, line in enumerate(workflow.read_text().splitlines(), 1):
                if "uses:" in line and "xila-project/platforms/.github/workflows/" not in line:
                    with self.subTest(workflow=workflow.name, line=line_number):
                        self.assertNotRegex(line, r"uses:\s+[^\s]+@[0-9a-f]{40}\b")


if __name__ == "__main__":
    unittest.main()
