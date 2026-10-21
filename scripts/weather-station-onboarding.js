#!/usr/bin/env node
/**
 * Weather Station Onboarding — Verification Helper (STUB)
 *
 * Referenced by: docs/runbooks/WEATHER_STATION_VERIFICATION_RUNBOOK.md
 * Tracked:       wrfcoin/core4#724 (testnet oracle trust bootstrap)
 *
 * Status: SCAFFOLD — every real verification operation is a TODO.
 * This file exists so the runbook's reference resolves and so operators
 * can import `assessStationVerificationEvidence` without their tooling
 * breaking. Replace the TODOs with real implementations before relying
 * on this for onboarding decisions.
 *
 * ------------------------------------------------------------------
 * Evidence model (from the runbook §Evidence Model):
 *   1. Geotagged installation photo (required)
 *   2. Map pin proof: Google Maps or OpenStreetMap URL (required)
 *   3. Satellite cross-check with distance proof + reference hash (optional)
 *
 * This helper scores confidence across six steps and returns a
 * decision in {`approve`, `manual_review`, `reject`}.
 * ------------------------------------------------------------------
 */

import rosterVerifier from '../tests/security/lib/ambient-roster-verifier.cjs';

const {
  validateStationRoster,
  selectExactDevice,
  verifyObservationFreshness,
} = rosterVerifier;

/**
 * @typedef {object} StationApplication
 * @property {string} stationId                - Canonical station identifier
 * @property {string} ownerIdentity            - Human-readable operator identity (PGP fingerprint preferred)
 * @property {number} claimedLatitude          - Claimed latitude, WGS84
 * @property {number} claimedLongitude         - Claimed longitude, WGS84
 * @property {object} geotaggedPhoto           - Evidence item 1
 * @property {string} geotaggedPhoto.hash      - Content hash of the image file (sha256 hex)
 * @property {number} [geotaggedPhoto.exifLat] - Latitude parsed from EXIF, if present
 * @property {number} [geotaggedPhoto.exifLon] - Longitude parsed from EXIF, if present
 * @property {object} mapPin                   - Evidence item 2
 * @property {string} mapPin.url               - Google Maps or OpenStreetMap URL
 * @property {object} [satelliteProof]         - Evidence item 3 (optional)
 * @property {string} [satelliteProof.referenceHash] - Content hash of satellite tile
 * @property {number} [satelliteProof.distanceMeters] - Computed distance proof
 */

/**
 * @typedef {object} VerificationResult
 * @property {number} confidence                               - 0.0 to 1.0
 * @property {'approve' | 'manual_review' | 'reject'} decision - Final decision
 * @property {string[]} findings                               - Human-readable audit log
 * @property {Record<string, 'pass' | 'fail' | 'skip' | 'todo'>} steps - Per-step outcomes
 */

/**
 * Per-step minimum confidence required for `approve`.
 * If the aggregate score drops below this the result escalates.
 */
const APPROVE_THRESHOLD = 0.85;
const REVIEW_THRESHOLD = 0.5;

/**
 * Verify the credential-free portion of onboarding.
 *
 * This is intentionally separate from the legacy geotag/photo evidence
 * helper below. It proves only the checks that need no provider credential:
 * strict roster shape, exact private selector/model binding, and freshness.
 * Provider authentication and any physical-location proof remain separate
 * runtime/manual gates and are never represented as a synthetic pass here.
 *
 * @param {{ rosterEntry: object, devices: object[], observation: object, nowMs?: number, maxAgeMinutes?: number }} packet
 * @returns {{ ok: boolean, stage?: string, codes?: string[], stationId?: string, model?: string, ageMinutes?: number }}
 */
export function verifyCredentialFreeStation(packet) {
  const roster = validateStationRoster([packet?.rosterEntry]);
  if (!roster.ok) {
    return { ok: false, stage: 'roster', codes: roster.errors.map((error) => error.code) };
  }

  const entry = roster.entries[0];
  const selected = selectExactDevice(entry, packet?.devices);
  if (!selected.ok) return { ok: false, stage: 'device', codes: [selected.code] };

  const fresh = verifyObservationFreshness(packet?.observation, {
    nowMs: packet?.nowMs,
    maxAgeMinutes: packet?.maxAgeMinutes,
  });
  if (!fresh.ok) return { ok: false, stage: 'freshness', codes: [fresh.code] };

  return {
    ok: true,
    stationId: entry.stationId,
    model: selected.model,
    // #2736: whether the provider corroborated the model (provider_verified)
    // or the value rests on the roster alone (operator_asserted).
    modelBinding: selected.modelBinding,
    ageMinutes: fresh.ageMinutes,
  };
}

/**
 * Step 1 — Metadata presence and well-formedness.
 * Enforces Approval Rule #1 from the runbook: station ID, owner identity,
 * and claimed lat/lon are all required before any other check runs.
 *
 * @param {StationApplication} app
 * @returns {{ ok: boolean, findings: string[] }}
 */
function step1ValidateMetadata(app) {
  const findings = [];
  let ok = true;
  if (!app || typeof app !== 'object') {
    return { ok: false, findings: ['metadata: application is null or not an object'] };
  }
  if (!app.stationId || typeof app.stationId !== 'string') {
    findings.push('metadata: stationId is missing or not a string');
    ok = false;
  }
  if (!app.ownerIdentity || typeof app.ownerIdentity !== 'string') {
    findings.push('metadata: ownerIdentity is missing or not a string');
    ok = false;
  }
  if (typeof app.claimedLatitude !== 'number' || app.claimedLatitude < -90 || app.claimedLatitude > 90) {
    findings.push('metadata: claimedLatitude is missing or out of range');
    ok = false;
  }
  if (typeof app.claimedLongitude !== 'number' || app.claimedLongitude < -180 || app.claimedLongitude > 180) {
    findings.push('metadata: claimedLongitude is missing or out of range');
    ok = false;
  }
  return { ok, findings };
}

/**
 * Step 2 — Geotagged photo verification.
 * Checks content hash presence and (if EXIF is present) that the embedded
 * coordinates agree with the claimed location within a tolerance.
 *
 * @param {StationApplication} app
 * @returns {{ score: number, findings: string[] }}
 */
function step2VerifyGeotaggedPhoto(app) {
  const findings = [];
  const photo = app.geotaggedPhoto;
  if (!photo || !photo.hash) {
    findings.push('photo: geotaggedPhoto.hash is required');
    return { score: 0, findings };
  }
  // TODO(#724): re-hash the referenced file from the evidence bundle and
  // verify the hash matches `photo.hash`. This stub cannot see the file
  // contents, so it trusts the caller's provided hash.
  findings.push('photo: hash accepted as provided (TODO: re-hash source file)');
  let score = 0.6;
  if (typeof photo.exifLat === 'number' && typeof photo.exifLon === 'number') {
    // TODO(#724): compute haversine distance between claimed and EXIF
    // coordinates; reject if > 100m. For now, only check presence.
    findings.push('photo: EXIF coordinates present (TODO: haversine tolerance check)');
    score = 0.8;
  } else {
    findings.push('photo: EXIF coordinates absent — downgraded confidence');
  }
  return { score, findings };
}

/**
 * Step 3 — Map-pin URL proof.
 * Parses the map URL (Google Maps or OpenStreetMap) and extracts its
 * embedded coordinates, then compares to the claimed location.
 *
 * @param {StationApplication} app
 * @returns {{ score: number, findings: string[] }}
 */
function step3VerifyMapPin(app) {
  const findings = [];
  const pin = app.mapPin;
  if (!pin || !pin.url) {
    findings.push('mapPin: mapPin.url is required');
    return { score: 0, findings };
  }
  // TODO(#724): parse the URL and extract `@lat,lng` or `?mlat=&mlon=`
  // (Google Maps and OSM formats respectively). Reject if the URL is
  // malformed or the coordinates disagree with the claimed location.
  // For now, accept any http(s) URL with a `maps` or `openstreetmap` host.
  let score = 0;
  try {
    const u = new URL(pin.url);
    const host = u.hostname.toLowerCase();
    if (host.includes('google') || host.includes('openstreetmap') || host.includes('osm.org')) {
      findings.push(`mapPin: URL host accepted (${host}) (TODO: parse coords and compare to claimed)`);
      score = 0.7;
    } else {
      findings.push(`mapPin: URL host not recognized (${host}) — manual review required`);
      score = 0.3;
    }
  } catch (e) {
    findings.push(`mapPin: URL is malformed — ${e.message}`);
    score = 0;
  }
  return { score, findings };
}

/**
 * Step 4 — Satellite cross-check (optional).
 * If a satellite proof is provided, verifies the distance proof and
 * reference hash. Absence of this step is not a failure — it caps the
 * achievable confidence but does not reject.
 *
 * @param {StationApplication} app
 * @returns {{ score: number, findings: string[], skipped: boolean }}
 */
function step4VerifySatelliteProof(app) {
  const findings = [];
  const sat = app.satelliteProof;
  if (!sat) {
    findings.push('satellite: proof absent (optional) — confidence capped');
    return { score: 0, findings, skipped: true };
  }
  if (!sat.referenceHash) {
    findings.push('satellite: referenceHash missing but proof object present — treated as skip');
    return { score: 0, findings, skipped: true };
  }
  if (typeof sat.distanceMeters !== 'number' || sat.distanceMeters < 0) {
    findings.push('satellite: distanceMeters invalid — ignoring satellite proof');
    return { score: 0, findings, skipped: true };
  }
  // TODO(#724): fetch the reference satellite tile by hash from the
  // onboarding store, verify it matches `referenceHash`, and confirm the
  // distance proof is consistent with the claimed coordinates. For now,
  // a present-and-plausible proof contributes a fixed bonus.
  findings.push('satellite: proof present and plausible (TODO: fetch tile + verify distance)');
  return { score: 0.2, findings, skipped: false };
}

/**
 * Step 5 — Aggregate confidence score.
 * Combines per-evidence scores into a single 0..1 number. Weighted so
 * that photo + map-pin together can reach 1.0, and the satellite bonus
 * rewards but does not require the optional third evidence item.
 *
 * @param {{ score: number }} photoResult
 * @param {{ score: number }} mapResult
 * @param {{ score: number, skipped: boolean }} satResult
 * @returns {number}
 */
function step5ScoreConfidence(photoResult, mapResult, satResult) {
  // TODO(#724): replace this arithmetic with a calibrated scoring
  // function once real data exists. Current weights are placeholders
  // chosen to make the decision boundaries testable, not field-tuned.
  const base = 0.5 * photoResult.score + 0.5 * mapResult.score;
  const bonus = satResult.skipped ? 0 : satResult.score;
  return Math.min(1.0, base + bonus);
}

/**
 * Step 6 — Decision emission.
 * Maps the aggregate confidence to a decision and returns the full
 * verification result including a human-readable findings log.
 *
 * @param {number} confidence
 * @param {string[]} findings
 * @param {Record<string, 'pass' | 'fail' | 'skip' | 'todo'>} steps
 * @returns {VerificationResult}
 */
function step6EmitDecision(confidence, findings, steps) {
  /** @type {'approve' | 'manual_review' | 'reject'} */
  let decision;
  if (confidence >= APPROVE_THRESHOLD) {
    decision = 'approve';
  } else if (confidence >= REVIEW_THRESHOLD) {
    decision = 'manual_review';
  } else {
    decision = 'reject';
  }
  return { confidence, decision, findings, steps };
}

/**
 * Assess a station verification evidence bundle and return a decision.
 * This is the function named by WEATHER_STATION_VERIFICATION_RUNBOOK.md.
 *
 * The function is STUB-QUALITY: every real check is a TODO. It will
 * return a well-formed result for any well-formed input, but operators
 * MUST NOT treat `approve` as authoritative until the TODOs are
 * replaced with real implementations.
 *
 * @param {StationApplication} application
 * @returns {VerificationResult}
 */
export function assessStationVerificationEvidence(application) {
  /** @type {Record<string, 'pass' | 'fail' | 'skip' | 'todo'>} */
  const steps = {};
  const findings = [];

  const step1 = step1ValidateMetadata(application);
  findings.push(...step1.findings);
  steps.metadata = step1.ok ? 'pass' : 'fail';
  if (!step1.ok) {
    return step6EmitDecision(0, findings, steps);
  }

  const step2 = step2VerifyGeotaggedPhoto(application);
  findings.push(...step2.findings);
  steps.geotaggedPhoto = step2.score > 0 ? 'todo' : 'fail';

  const step3 = step3VerifyMapPin(application);
  findings.push(...step3.findings);
  steps.mapPin = step3.score > 0 ? 'todo' : 'fail';

  const step4 = step4VerifySatelliteProof(application);
  findings.push(...step4.findings);
  steps.satellite = step4.skipped ? 'skip' : 'todo';

  const confidence = step5ScoreConfidence(step2, step3, step4);
  steps.score = 'todo';

  const result = step6EmitDecision(confidence, findings, steps);
  findings.push(
    `decision: ${result.decision} (confidence=${confidence.toFixed(2)}, approve>=${APPROVE_THRESHOLD}, review>=${REVIEW_THRESHOLD})`,
  );
  findings.push('WARNING: stub scoring — every verification step is a TODO. Do not rely on `approve` for production onboarding.');
  return result;
}

// -------------------------------------------------------------------
// CLI usage — running this file directly surfaces the stub status.
// -------------------------------------------------------------------
const isMain = import.meta.url === `file://${process.argv[1]}`;
if (isMain) {
  const args = process.argv.slice(2);
  if (args.includes('--help') || args.includes('-h')) {
    console.log(`weather-station-onboarding.js — verification helper (STUB)

Usage:
  node scripts/weather-station-onboarding.js --help
  node scripts/weather-station-onboarding.js --self-test
  node scripts/weather-station-onboarding.js < application.json

Exports:
  assessStationVerificationEvidence(application) -> VerificationResult

Status: SCAFFOLD. Every verification step is a TODO tagged #724.
See docs/runbooks/WEATHER_STATION_VERIFICATION_RUNBOOK.md.
`);
    process.exit(0);
  }
  if (args.includes('--self-test')) {
    const sample = {
      stationId: 'wrfs-test-001',
      ownerIdentity: 'testnet-operator-a <pgp:DEADBEEF>',
      claimedLatitude: 40.7128,
      claimedLongitude: -74.006,
      geotaggedPhoto: {
        hash: '0'.repeat(64),
        exifLat: 40.7128,
        exifLon: -74.006,
      },
      mapPin: { url: 'https://www.openstreetmap.org/?mlat=40.7128&mlon=-74.006' },
      satelliteProof: { referenceHash: '0'.repeat(64), distanceMeters: 12 },
    };
    const result = assessStationVerificationEvidence(sample);
    console.log(JSON.stringify(result, null, 2));
    process.exit(result.decision === 'reject' ? 1 : 0);
  }
  // Default: read JSON from stdin.
  let raw = '';
  process.stdin.setEncoding('utf8');
  process.stdin.on('data', (chunk) => {
    raw += chunk;
  });
  process.stdin.on('end', () => {
    try {
      const app = JSON.parse(raw);
      const result = assessStationVerificationEvidence(app);
      console.log(JSON.stringify(result, null, 2));
      process.exit(result.decision === 'reject' ? 1 : 0);
    } catch (e) {
      console.error(`error: ${e.message}`);
      process.exit(2);
    }
  });
}
