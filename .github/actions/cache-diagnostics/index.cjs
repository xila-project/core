const fs = require("node:fs");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

// Register before cache actions, so reporting runs after their post steps.
const log = path.join(process.env.RUNNER_TEMP, "xila-sccache-private.log");
if (process.env.STATE_report !== "true") {
  fs.appendFileSync(process.env.GITHUB_STATE, "report=true\n");
  // The write failure in sccache 0.18.0 is logged at server DEBUG, not WARN.
  // Never print/upload this file: backend errors can contain signed URLs.
  fs.appendFileSync(
    process.env.GITHUB_ENV,
    `SCCACHE_LOG=sccache::server=debug\nSCCACHE_ERROR_LOG=${log}\n`,
  );
} else {
  try {
    const script = path.resolve(__dirname, "../../../tools/ci/report_errors.py");
    const result = spawnSync("python3", [script, log], { encoding: "utf8" });
    if (result.status === 0) {
      // report_errors.py emits ONLY fixed category names and integer counts.
      const counts = JSON.parse(result.stdout);
      console.log(`CI_SCCACHE_ERRORS ${JSON.stringify(counts)}`);
      fs.appendFileSync(
        process.env.GITHUB_STEP_SUMMARY,
        `\n### sccache write diagnostics\n\n\`\`\`json\n${JSON.stringify(counts, null, 2)}\n\`\`\`\n\nCounts are diagnostic log events, not necessarily unique requests. Read-only PR errors are expected.\n`,
      );
    } else {
      // Do not echo stderr or raw log content, even on parser failures.
      console.log("::warning::sccache diagnostic classification unavailable");
    }
  } finally {
    fs.rmSync(log, { force: true });
  }
}
