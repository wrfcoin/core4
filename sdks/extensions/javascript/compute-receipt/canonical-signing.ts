/**
 * JavaScript port of `shared-protocol/src/crypto/domain_signing.rs`.
 *
 * Constants and `canonicalSigningMessage()` must stay byte-compatible with the
 * Rust implementation. If you change this file, re-run the cross-language
 * compute receipt compatibility tests.
 */

import { sha3_256 } from "@noble/hashes/sha3.js";
import { bytesToHex } from "@noble/hashes/utils.js";

export const SIGNING_SCHEME_VERSION = "wrfcoin-sign-v1";
export const DEFAULT_CHAIN_ID = "wrfcoin-main";
export const DEFAULT_NETWORK_ID = "mainnet";

export const DomainSigningAction = {
  OracleAttestation: "wrfcoin.oracle_attestation.v1",
  CalibrationAttestation: "wrfcoin.calibration_attestation.v1",
  BridgeAction: "wrfcoin.bridge_action.v1",
  GovernanceAction: "wrfcoin.governance_action.v1",
  KeyRotation: "wrfcoin.key_rotation.v1",
  SnapshotSync: "wrfcoin.snapshot_sync.v1",
  ForecastVerification: "wrfcoin.forecast_verification.v1",
  ComputeReceipt: "wrfcoin.compute_receipt.v1",
  SelectiveDisclosure: "wrfcoin.selective_disclosure.v1",
  WeatherDataGossip: "wrfcoin.weather_data_gossip.v1",
  SlashingReport: "wrfcoin.slashing_report.v1",
} as const;

export type DomainSigningActionValue =
  (typeof DomainSigningAction)[keyof typeof DomainSigningAction];

export interface DomainSigningContext {
  action: DomainSigningActionValue;
  signerId: string;
  timestamp: number;
  nonce?: string | null;
  chainId?: string;
  networkId?: string;
}

export interface ComposedSigningContext {
  frame: "composed_v2";
  tag: string;
  chainId: string;
  networkId: string;
  protocolVersion: 1 | 2;
}

export interface ComputeReceiptSigningInput {
  task_id: string;
  worker_id: string;
  work_hash: string;
  work_factor: number;
  completed_at: number;
  nonce: string;
}

export interface ForecastVerificationReceiptSigningInput {
  manifest_digest: string;
  commit_id: string;
  oracle_id: string;
  metrics: {
    mae: number;
    rmse: number;
    bias: number;
    skill_score: number;
  };
  verified_at: number;
  nonce: string;
  zk_proof?: string | null;
}

export type ComposedSigningObject =
  | {
      kind: "ComputeReceipt";
      input: ComputeReceiptSigningInput;
    }
  | {
      kind: "ForecastVerificationReceipt";
      input: ForecastVerificationReceiptSigningInput;
    };

const textEncoder = new TextEncoder();

function concatBytes(...parts: Uint8Array[]): Uint8Array {
  const length = parts.reduce((sum, part) => sum + part.length, 0);
  const out = new Uint8Array(length);
  let offset = 0;
  for (const part of parts) {
    out.set(part, offset);
    offset += part.length;
  }
  return out;
}

function assertSafeInteger(field: string, value: number): void {
  if (!Number.isSafeInteger(value)) {
    throw new Error(`${field} must be a safe integer`);
  }
}

function u64Le(field: string, value: number): Uint8Array {
  assertSafeInteger(field, value);
  if (value < 0) {
    throw new Error(`${field} must be >= 0`);
  }
  const out = new Uint8Array(8);
  new DataView(out.buffer).setBigUint64(0, BigInt(value), true);
  return out;
}

function i64Le(field: string, value: number): Uint8Array {
  assertSafeInteger(field, value);
  const out = new Uint8Array(8);
  new DataView(out.buffer).setBigInt64(0, BigInt(value), true);
  return out;
}

function u32Le(field: string, value: number): Uint8Array {
  if (!Number.isInteger(value) || value < 0 || value > 0xffff_ffff) {
    throw new Error(`${field} must fit in u32`);
  }
  const out = new Uint8Array(4);
  new DataView(out.buffer).setUint32(0, value, true);
  return out;
}

function lengthPrefixed(value: Uint8Array): Uint8Array {
  return concatBytes(u64Le("field length", value.length), value);
}

function stringField(value: string): Uint8Array {
  return lengthPrefixed(textEncoder.encode(value));
}

function optionalStringField(value: string | null | undefined): Uint8Array {
  if (value === undefined || value === null) {
    return Uint8Array.of(0);
  }
  return concatBytes(Uint8Array.of(1), stringField(value));
}

function computeReceiptCanonicalBody(
  input: ComputeReceiptSigningInput,
): Uint8Array {
  return concatBytes(
    stringField(input.task_id),
    stringField(input.worker_id),
    stringField(input.work_hash),
    u64Le("work_factor", input.work_factor),
    i64Le("completed_at", input.completed_at),
    stringField(input.nonce),
  );
}

function forecastReceiptCanonicalBody(
  input: ForecastVerificationReceiptSigningInput,
): Uint8Array {
  for (const [field, value] of Object.entries(input.metrics)) {
    if (!Number.isFinite(value)) {
      throw new Error(`metrics.${field} must be finite`);
    }
  }

  // Rust serializes ForecastAccuracyMetrics in struct declaration order.
  const metricsBytes = canonicalJsonBytes({
    mae: input.metrics.mae,
    rmse: input.metrics.rmse,
    bias: input.metrics.bias,
    skill_score: input.metrics.skill_score,
  });

  return concatBytes(
    stringField(input.manifest_digest),
    stringField(input.commit_id),
    stringField(input.oracle_id),
    lengthPrefixed(metricsBytes),
    i64Le("verified_at", input.verified_at),
    stringField(input.nonce),
    optionalStringField(input.zk_proof),
  );
}

export function canonicalBodyBytes(object: ComposedSigningObject): Uint8Array {
  switch (object.kind) {
    case "ComputeReceipt":
      return computeReceiptCanonicalBody(object.input);
    case "ForecastVerificationReceipt":
      return forecastReceiptCanonicalBody(object.input);
  }
}

export function canonicalSigningMessage(
  context: DomainSigningContext,
  payloadDigestHexValue: string,
): string;
export function canonicalSigningMessage(
  context: ComposedSigningContext,
  canonicalBody: Uint8Array,
): Uint8Array;
export function canonicalSigningMessage(
  context: DomainSigningContext | ComposedSigningContext,
  payload: string | Uint8Array,
): string | Uint8Array {
  if ("frame" in context) {
    if (!(payload instanceof Uint8Array)) {
      throw new Error("composed_v2 canonical body must be bytes");
    }

    const tag = stringField(context.tag);
    const body = lengthPrefixed(payload);
    if (context.protocolVersion === 1) {
      return concatBytes(tag, body);
    }
    return concatBytes(
      tag,
      stringField(context.chainId),
      stringField(context.networkId),
      u32Le("protocolVersion", context.protocolVersion),
      body,
    );
  }

  if (typeof payload !== "string") {
    throw new Error("legacy payload digest must be hex text");
  }
  const chainId = context.chainId ?? DEFAULT_CHAIN_ID;
  const networkId = context.networkId ?? DEFAULT_NETWORK_ID;
  const nonce = context.nonce ?? "";

  return [
    SIGNING_SCHEME_VERSION,
    context.action,
    chainId,
    networkId,
    context.signerId,
    String(context.timestamp),
    nonce,
    payload,
  ].join("|");
}

export function payloadDigestHex(canonicalJson: Uint8Array): string {
  return bytesToHex(sha3_256(canonicalJson));
}

export function canonicalJsonBytes(value: unknown): Uint8Array {
  return textEncoder.encode(JSON.stringify(value));
}
