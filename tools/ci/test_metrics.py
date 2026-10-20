import unittest

import metrics


class MetricsTests(unittest.TestCase):
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

    def test_failed_and_running_jobs_are_not_successful_comparisons(self):
        for conclusion in ["failure", "cancelled", None]:
            result = metrics.job_metrics([{"name": "test", "conclusion": conclusion, "started_at": None, "completed_at": None, "steps": []}])
            self.assertFalse(result["successful_sample"])


if __name__ == "__main__":
    unittest.main()
