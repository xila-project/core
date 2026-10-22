#!/usr/bin/env python3
"""Analyze completed Actions runs, including cache post steps. Requires gh auth.

No raw diagnostic lines, environment values, cache URLs or credentials are emitted.
Durations are observed runner occupancy, NOT billed/rounded GitHub minutes.
"""

import argparse
from collections import Counter
from datetime import datetime
import json
import pathlib
import re
import subprocess


def timestamp(value):
    return datetime.fromisoformat(value.replace("Z", "+00:00")) if value else None


def seconds(start, end):
    return max(0, (timestamp(end) - timestamp(start)).total_seconds()) if start and end else 0


def job_metrics(jobs):
    rows = []
    active = [job for job in jobs if job["conclusion"] != "skipped"]
    for job in jobs:
        rows.append({
            "name": job["name"], "conclusion": job["conclusion"],
            "seconds": seconds(job.get("started_at"), job.get("completed_at")) if job in active else 0,
            "steps": [{"name": step["name"], "conclusion": step["conclusion"],
                       "seconds": seconds(step.get("started_at"), step.get("completed_at"))}
                      for step in job.get("steps", [])],
        })
    starts = [job["started_at"] for job in active if job.get("started_at")]
    ends = [job["completed_at"] for job in active if job.get("completed_at")]
    return {
        "successful_sample": bool(active) and all(job["conclusion"] == "success" for job in active),
        "runner_minutes": round(sum(row["seconds"] for row in rows) / 60, 3),
        "execution_span_minutes": round(seconds(min(starts), max(ends)) / 60, 3) if starts and ends else 0,
        "jobs": rows,
    }


def error_categories(text):
    """Allowlist counts only. Never return any part of an arbitrary log line."""
    counts = Counter()
    for line in text.splitlines():
        if "error executing cache write" not in line.lower():
            continue
        line = line.lower()
        if "read-only" in line or "read only" in line:
            category = "read_only"
        elif re.search(r"\b429\b|too many requests|ratelimit", line):
            category = "rate_limited"
        elif re.search(r"\b409\b|alreadyexists|already exists", line):
            category = "already_exists"
        elif re.search(r"\b401\b|\b403\b|unauthorized|forbidden|permissiondenied", line):
            category = "authorization"
        elif "quota" in line or "storage limit" in line or "payment" in line:
            category = "quota"
        elif "timeout" in line or "timed out" in line:
            category = "timeout"
        else:
            category = "other"
        counts[category] += 1
    return dict(counts)


def sccache_metrics(stats):
    result = {key: stats.get(key, 0) for key in (
        "compile_requests", "cache_writes", "cache_write_errors", "cache_read_errors", "cache_timeouts", "compilations", "compile_fails")}
    for key in ("cache_hits", "cache_misses"):
        counts = stats.get(key, {}).get("counts", {})
        result[key] = {language: counts.get(language, 0) for language in ("Rust", "C/C++")}
    return result


def log_metrics(log):
    jobs = {}
    transfers = {}
    ansi = re.compile(r"(?:\x1b|\^\[)\[[0-9;]*m")
    for raw in log.splitlines():
        parts = raw.split("\t", 2)
        if len(parts) != 3:
            continue
        name, _, line = parts
        match = re.match(r"(\d{4}-\S+) (.*)", line)
        if not match:
            continue
        time, line = match.groups()
        line = ansi.sub("", line)
        row = jobs.setdefault(name, {"cargo_compiling": 0, "cargo_checking": 0, "cache_transfers": []})
        exact_hit = re.fullmatch(r"Rust cache exact hit: (true|false)", line)
        if exact_hit:
            row["rust_cache_exact_hit"] = exact_hit[1] == "true"
        if re.match(r"\s*Compiling \S+ v", line):
            row["cargo_compiling"] += 1
        if re.match(r"\s*Checking \S+ v", line):
            row["cargo_checking"] += 1
        # Last snapshot wins: sccache-action can print stats multiple times.
        if line.startswith('{"stats":'):
            try:
                row["sccache"] = sccache_metrics(json.loads(line)["stats"])
            except (ValueError, KeyError, TypeError):
                pass
        if "CI_SCCACHE_ERRORS " in line:
            try:
                data = json.loads(line.split("CI_SCCACHE_ERRORS ", 1)[1])
                row["sccache_write_error_categories"] = {key: value for key, value in data.items()
                    if key in {"read_only", "rate_limited", "already_exists", "authorization", "quota", "timeout", "other"}
                    and isinstance(value, int)}
            except (ValueError, AttributeError):
                pass
        if "... Restoring cache ..." in line:
            transfers[name] = {"operation": "restore", "start": time}
        elif "... Saving cache ..." in line:
            transfers[name] = {"operation": "save", "start": time}
        transfer = transfers.get(name)
        if transfer:
            size = re.search(r"(?:Received|Sent) \d+ of (\d+)", line)
            if size:
                transfer["bytes"] = int(size[1])
            size = re.search(r"Cache Size: ~\d+ MB \((\d+) B\)", line)
            if size:
                transfer["bytes"] = int(size[1])
            if "Failed to save:" in line:
                transfer["failed"] = True
            if "Cache restored successfully" in line or "Cache saved successfully" in line or line.startswith("##[end-action"):
                row["cache_transfers"].append({"operation": transfer["operation"],
                    "seconds": round(seconds(transfer["start"], time), 3), "bytes": transfer.get("bytes"),
                    **({"failed": True} if transfer.get("failed") else {})})
                del transfers[name]
    return jobs


def gh(*args):
    return subprocess.check_output(["gh", *args], text=True)


def collect(repo, run_id):
    run = json.loads(gh("api", f"repos/{repo}/actions/runs/{run_id}"))
    attempt = run["run_attempt"]
    base = f"repos/{repo}/actions/runs/{run_id}/attempts/{attempt}"
    attempt_run = json.loads(gh("api", base))
    pages = json.loads(gh("api", "--paginate", "--slurp", f"{base}/jobs?per_page=100"))
    result = job_metrics([job for page in pages for job in page["jobs"]])
    result.update({key: run[key] for key in ("id", "head_sha", "event", "conclusion", "run_attempt", "created_at")})
    result["url"] = run["html_url"]
    # Scope jobs and logs to one attempt. Original created_at includes potentially
    # days of inter-attempt delay; updated_at changes with artifacts/metadata.
    result["attempt_started_at"] = attempt_run["run_started_at"]
    ends = [job["completed_at"] for page in pages for job in page["jobs"] if job.get("completed_at") and job["conclusion"] != "skipped"]
    result["wall_minutes"] = round(seconds(result["attempt_started_at"], max(ends)) / 60, 3) if ends else None
    result["logs"] = log_metrics(gh("run", "view", str(run_id), "--repo", repo, "--attempt", str(attempt), "--log"))
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", default="xila-project/core")
    parser.add_argument("--run", type=int, action="append", required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    args = parser.parse_args()
    results = [collect(args.repo, run) for run in args.run]
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(results, indent=2) + "\n")
    for result in results:
        print(f"{result['id']}: {result['conclusion']}; {result['wall_minutes']} wall min; {result['runner_minutes']} runner min")


if __name__ == "__main__":
    main()
