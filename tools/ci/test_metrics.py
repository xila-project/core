import unittest
import subprocess
from unittest.mock import patch

import metrics


class MetricsTests(unittest.TestCase):
    def test_reruns_use_attempt_specific_jobs_logs_and_wall_origin(self):
        import json
        run = {"id": 42, "run_attempt": 2, "head_sha": "abc", "event": "push", "conclusion": "success", "created_at": "2026-10-19T00:00:00Z", "html_url": "https://example/run"}
        attempt = {"run_started_at": "2026-10-20T00:00:00Z"}
        jobs = [{"jobs": [{"name": "test", "conclusion": "success", "started_at": "2026-10-20T00:00:00Z", "completed_at": "2026-10-20T00:01:00Z", "steps": []}]}]
        with patch.object(metrics, "gh", side_effect=[json.dumps(run), json.dumps(attempt), json.dumps(jobs), ""]) as gh:
            result = metrics.collect("owner/repo", 42)
        self.assertEqual(result["wall_minutes"], 1)
        self.assertEqual(result["runner_minutes"], 1)
        self.assertIn("repos/owner/repo/actions/runs/42/attempts/2/jobs?per_page=100", gh.call_args_list[2].args)
        self.assertIn("--attempt", gh.call_args_list[3].args)

    def test_archive_hit_is_preserved_separately_from_compiler_hits(self):
        for value, expected in [("true", True), ("false", False)]:
            result = metrics.log_metrics(f"test\tUNKNOWN STEP\t2026-10-20T00:00:00Z Rust cache exact hit: {value}")
            self.assertIs(result["test"]["rust_cache_exact_hit"], expected)

    def test_failed_workflow_is_not_a_successful_sample_even_if_jobs_succeeded(self):
        import json
        run = {"id": 42, "run_attempt": 1, "head_sha": "abc", "event": "push", "conclusion": "cancelled", "created_at": "2026-10-20T00:00:00Z", "html_url": "https://example/run"}
        attempt = {"run_started_at": "2026-10-20T00:00:00Z"}
        jobs = [{"jobs": [{"name": "test", "conclusion": "success", "started_at": "2026-10-20T00:00:00Z", "completed_at": "2026-10-20T00:01:00Z", "steps": []}]}]
        with patch.object(metrics, "gh", side_effect=[json.dumps(run), json.dumps(attempt), json.dumps(jobs), ""]):
            result = metrics.collect("owner/repo", 42)
        self.assertFalse(result["successful_sample"])

    def test_missing_cancelled_job_logs_do_not_discard_runner_cost(self):
        import json
        run = {"id": 42, "run_attempt": 1, "head_sha": "abc", "event": "push", "conclusion": "cancelled", "created_at": "2026-10-20T00:00:00Z", "html_url": "https://example/run"}
        attempt = {"run_started_at": "2026-10-20T00:00:00Z"}
        jobs = [{"jobs": [{"name": "test", "conclusion": "cancelled", "started_at": "2026-10-20T00:00:00Z", "completed_at": "2026-10-20T00:01:00Z", "steps": []}]}]
        with patch.object(metrics, "gh", side_effect=[json.dumps(run), json.dumps(attempt), json.dumps(jobs), subprocess.CalledProcessError(1, "gh")]):
            result = metrics.collect("owner/repo", 42)
        self.assertEqual(result["runner_minutes"], 1)
        self.assertFalse(result["logs_available"])
        self.assertFalse(result["successful_sample"])

    def test_runner_minutes_include_post_steps_not_queue_or_skipped_jobs(self):
        jobs = [
            {"name": "test", "conclusion": "success", "started_at": "2026-10-20T00:01:00Z", "completed_at": "2026-10-20T00:03:00Z", "steps": [{"name": "Post cache", "started_at": "2026-10-20T00:02:00Z", "completed_at": "2026-10-20T00:03:00Z", "conclusion": "success"}]},
            {"name": "lint", "conclusion": "success", "started_at": "2026-10-20T00:01:00Z", "completed_at": "2026-10-20T00:02:00Z", "steps": []},
            {"name": "skip", "conclusion": "skipped", "started_at": "2026-10-20T00:01:00Z", "completed_at": "2026-10-20T00:00:59Z", "steps": []},
        ]
        result = metrics.job_metrics(jobs)
        self.assertEqual(result["runner_minutes"], 3)
        self.assertEqual(result["execution_span_minutes"], 2)
        self.assertEqual(result["jobs"][0]["steps"][0]["seconds"], 60)
        self.assertEqual(result["jobs"][2]["seconds"], 0)

    def test_log_metrics_deduplicate_sccache_snapshots_and_count_cargo_lines(self):
        stats = '{"stats":{"cache_hits":{"counts":{"Rust":2,"C/C++":9}},"cache_misses":{"counts":{"Rust":3}},"cache_write_errors":4,"cache_writes":1},"cache_location":"https://secret"}'
        log = "\n".join([
            "test\tUNKNOWN STEP\t2026-10-20T00:00:00Z ... Restoring cache ...",
            "test\tUNKNOWN STEP\t2026-10-20T00:00:01Z Received 50 of 100 (50%)",
            "test\tUNKNOWN STEP\t2026-10-20T00:00:03Z Cache restored successfully",
            "test\tUNKNOWN STEP\t2026-10-20T00:00:04Z \x1b[32m Compiling lvgl_rust_sys v0.1.0",
            "test\tUNKNOWN STEP\t2026-10-20T00:00:05Z     Checking task v0.1.0",
            "test\tUNKNOWN STEP\t2026-10-20T00:00:06Z " + stats,
            "test\tUNKNOWN STEP\t2026-10-20T00:00:07Z " + stats,
        ])
        result = metrics.log_metrics(log)["test"]
        self.assertEqual(result["cargo_compiling"], 1)
        self.assertEqual(result["cargo_checking"], 1)
        self.assertEqual(result["sccache"]["cache_hits"]["Rust"], 2)
        self.assertNotIn("cache_location", result["sccache"])
        self.assertEqual(result["cache_transfers"], [{"operation": "restore", "seconds": 3, "bytes": 100}])

    def test_sccache_errors_publish_only_allowlisted_categories(self):
        report = metrics.error_categories("\n".join([
            "Error executing cache write: 429 Too Many Requests authorization=secret",
            "Error executing cache write: status: 403 URL=https://secret",
            "Error executing cache write: AlreadyExists status 409 token=secret",
            "Error executing cache write: PermissionDenied: storage is read-only",
            "Error executing cache write: quota exceeded",
            "Error executing cache write: unexpected sensitive error secret",
            "DEBUG normal compiler command with secret",
        ]))
        self.assertEqual(report, {"rate_limited": 1, "authorization": 1, "already_exists": 1, "read_only": 1, "quota": 1, "other": 1})
        self.assertNotIn("secret", str(report))

    def test_startup_probe_failure_is_distinct_from_read_only_write_events(self):
        report = metrics.error_categories("\n".join([
            "storage write check failed: rate limited status429 token=secret",
            "Error executing cache write: Cannot write to read-only storage",
        ]))
        self.assertEqual(report, {"startup_rate_limited": 1, "read_only": 1})

    def test_quota_rejection_is_not_misclassified_as_generic_authorization(self):
        self.assertEqual(metrics.error_categories("storage write check failed: PermissionDenied status403 quota exceeded"), {"startup_quota": 1})

    def test_github_escaped_color_and_new_action_end_markers(self):
        log = "\n".join([
            "test\tUNKNOWN STEP\t2026-10-20T00:00:00Z ^[[1m^[[92m   Compiling^[[0m lvgl_rust_sys v0.1.0",
            "test\tUNKNOWN STEP\t2026-10-20T00:00:01Z ... Saving cache ...",
            "test\tUNKNOWN STEP\t2026-10-20T00:00:02Z Sent 100 of 100 (100.0%)",
            "test\tUNKNOWN STEP\t2026-10-20T00:00:04Z ##[end-action id=rust-cache;outcome=success;conclusion=success]",
        ])
        result = metrics.log_metrics(log)["test"]
        self.assertEqual(result["cargo_compiling"], 1)
        self.assertEqual(result["cache_transfers"], [{"operation": "save", "seconds": 3, "bytes": 100}])

    def test_standalone_cache_steps_end_at_bom_prefixed_next_step(self):
        log = "\n".join([
            "test\tPost Cache\t2026-10-20T00:00:01Z ... Saving cache ...",
            "test\tPost Cache\t2026-10-20T00:00:02Z Sent 100 of 100 (100.0%)",
            "test\tPost Checkout\t\ufeff2026-10-20T00:00:04Z Post job cleanup.",
        ])
        self.assertEqual(metrics.log_metrics(log)["test"]["cache_transfers"], [{"operation": "save", "seconds": 3, "bytes": 100}])

    def test_archive_key_match_distinguishes_exact_and_fallback_restore(self):
        for restored, expected in [("expected", True), ("older", False)]:
            log = "\n".join([
                f'test\tCache\t2026-10-20T00:00:00Z Restored from cache key "{restored}" full match: {str(expected).lower()}.',
            ])
            self.assertIs(metrics.log_metrics(log)["test"]["rust_cache_exact_hit"], expected)

    def test_post_restore_cache_up_to_date_means_exact_archive_hit(self):
        log = "test\tPost Cache\t2026-10-20T00:00:00Z Cache up-to-date."
        self.assertIs(metrics.log_metrics(log)["test"]["rust_cache_exact_hit"], True)

    def test_failed_and_running_jobs_are_not_successful_comparisons(self):
        for conclusion in ["failure", "cancelled", None]:
            result = metrics.job_metrics([{"name": "test", "conclusion": conclusion, "started_at": None, "completed_at": None, "steps": []}])
            self.assertFalse(result["successful_sample"])


if __name__ == "__main__":
    unittest.main()
