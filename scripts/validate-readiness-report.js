#!/usr/bin/env node
/**
 * validate-readiness-report.js
 *
 * Validates a CI readiness report JSON file against the canonical schema,
 * checks freshness, and confirms all gates pass.
 *
 * Usage:
 *   node scripts/validate-readiness-report.js [path/to/readiness-report.json]
 *
 * Exit codes:
 *   0 - Report is valid, current, and all gates passed.
 *   1 - Validation failure (schema mismatch, stale report, or failing gate).
 */

import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import Ajv from "ajv";
import addFormats from "ajv-formats";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const projectRoot = resolve(__dirname, "..");

const STALE_THRESHOLD_HOURS = 4;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function loadJSON(filePath) {
  const raw = readFileSync(filePath, "utf-8");
  return JSON.parse(raw);
}

function formatHoursAgo(ms) {
  const hours = ms / (1000 * 60 * 60);
  return `${hours.toFixed(1)}h`;
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

function main() {
  const reportPath = resolve(process.argv[2] || "readiness-report.json");
  const schemaPath = resolve(
    projectRoot,
    ".github",
    "readiness-report.schema.json",
  );

  // 1. Load the report --------------------------------------------------
  let report;
  try {
    report = loadJSON(reportPath);
  } catch {
    console.error(`FAIL: Cannot read readiness report at ${reportPath}`);
    process.exit(1);
  }

  // 2. Load the schema --------------------------------------------------
  let schema;
  try {
    schema = loadJSON(schemaPath);
  } catch {
    console.error(`FAIL: Cannot read schema at ${schemaPath}`);
    process.exit(1);
  }

  // 3. Schema validation -------------------------------------------------
  const ajv = new Ajv({ allErrors: true });
  addFormats(ajv);
  const validate = ajv.compile(schema);
  const valid = validate(report);

  if (!valid) {
    console.error("FAIL: Readiness report does not match schema:");
    for (const err of validate.errors) {
      const location = err.instancePath || "(root)";
      console.error(`  - ${location}: ${err.message}`);
    }
    process.exit(1);
  }

  // 4. Staleness check ---------------------------------------------------
  const generatedAt = new Date(report.generated_at);
  const now = new Date();
  const ageMs = now.getTime() - generatedAt.getTime();
  const thresholdMs = STALE_THRESHOLD_HOURS * 60 * 60 * 1000;

  if (ageMs > thresholdMs) {
    console.error(
      `FAIL: Report is stale (generated ${report.generated_at}, ${formatHoursAgo(ageMs)} ago)`,
    );
    process.exit(1);
  }

  // 5. Overall pass check ------------------------------------------------
  if (report.overall !== "pass") {
    console.error(
      "FAIL: Readiness report overall status is not 'pass'",
    );

    // List failing layers
    for (const [name, layer] of Object.entries(report.layers)) {
      if (layer.status !== "pass") {
        const failingChecks = Object.entries(layer)
          .filter(([k, v]) => k !== "status" && v !== "pass")
          .map(([k]) => k);
        console.error(
          `  - layer/${name}: status=${layer.status}` +
            (failingChecks.length
              ? ` (failing: ${failingChecks.join(", ")})`
              : ""),
        );
      }
    }

    // List failing gates
    for (const [name, status] of Object.entries(report.gates)) {
      if (status !== "pass") {
        console.error(`  - gate/${name}: ${status}`);
      }
    }

    process.exit(1);
  }

  // 6. Success -----------------------------------------------------------
  console.log(
    "PASS: Readiness report is valid, current, and all gates passed",
  );
  console.log(
    `  commit: ${report.commit}  branch: ${report.branch}  generated: ${report.generated_at}`,
  );
  process.exit(0);
}

main();
