/**
 * TypeScript helpers for minting and verifying WRFCoin `ComputeReceipt`
 * signatures from the shared sdk-extensions package.
 */

import { ed25519 } from "@noble/curves/ed25519.js";
import { blake3 } from "@noble/hashes/blake3.js";
import { bytesToHex, hexToBytes, randomBytes } from "@noble/hashes/utils.js";

import {
  DomainSigningAction,
  canonicalJsonBytes,
  canonicalSigningMessage as buildEnvelope,
  payloadDigestHex,
} from "./canonical-signing.js";

export {
  DomainSigningAction,
  DEFAULT_CHAIN_ID,
  DEFAULT_NETWORK_ID,
  SIGNING_SCHEME_VERSION,
} from "./canonical-signing.js";

export interface ComputeReceipt {
  task_id: string;
  worker_id: string;
  work_hash: string;
  work_factor: number;
  completed_at: number;
  nonce: string;
  signature?: string;
}

type SigningPayload = {
  task_id: string;
  worker_id: string;
  work_hash: string;
  work_factor: number;
  completed_at: number;
  nonce: string;
};

function buildSigningPayload(
  receipt: Omit<ComputeReceipt, "signature">,
): SigningPayload {
  return {
    task_id: receipt.task_id,
    worker_id: receipt.worker_id,
    work_hash: receipt.work_hash,
    work_factor: receipt.work_factor,
    completed_at: receipt.completed_at,
    nonce: receipt.nonce,
  };
}

function normalizeHex(hex: string): string {
  const stripped = hex.startsWith("0x") || hex.startsWith("0X")
    ? hex.slice(2)
    : hex;
  return stripped.toLowerCase();
}

function assertSafeU64(field: string, value: number): void {
  if (!Number.isFinite(value) || !Number.isInteger(value)) {
    throw new Error(`${field} must be a finite integer`);
  }
  if (value < 0) {
    throw new Error(`${field} must be >= 0`);
  }
  if (!Number.isSafeInteger(value)) {
    throw new Error(
      `${field} must be a safe integer (<= ${Number.MAX_SAFE_INTEGER})`,
    );
  }
}

function assertSafeI64(field: string, value: number): void {
  if (!Number.isFinite(value) || !Number.isInteger(value)) {
    throw new Error(`${field} must be a finite integer`);
  }
  if (!Number.isSafeInteger(value)) {
    throw new Error(
      `${field} must be a safe integer (>= ${Number.MIN_SAFE_INTEGER} and <= ${Number.MAX_SAFE_INTEGER})`,
    );
  }
}

export function canonicalSigningMessage(
  receipt: Omit<ComputeReceipt, "signature">,
): string {
  const payload = buildSigningPayload(receipt);
  const digest = payloadDigestHex(canonicalJsonBytes(payload));

  return buildEnvelope(
    {
      action: DomainSigningAction.ComputeReceipt,
      signerId: receipt.worker_id,
      timestamp: receipt.completed_at,
      nonce: receipt.nonce,
    },
    digest,
  );
}

export function computeWorkHash(input: Uint8Array, iterations: number): string {
  if (!Number.isInteger(iterations) || iterations < 1) {
    throw new Error("iterations must be a positive integer");
  }

  let acc = blake3(input);
  for (let i = 1; i < iterations; i += 1) {
    acc = blake3(acc);
  }
  return bytesToHex(acc);
}

interface SignComputeReceiptBaseArgs {
  taskId: string;
  workerPrivateKey: string;
  workFactor: number;
  completedAt?: number;
  nonce?: string;
}

export type SignComputeReceiptArgs = SignComputeReceiptBaseArgs & (
  | {
      workHash: string;
    }
  | {
      workInput: Uint8Array;
      workIterations: number;
    }
);

export function signComputeReceipt(args: SignComputeReceiptArgs): ComputeReceipt {
  const secretKey = hexToBytes(normalizeHex(args.workerPrivateKey));
  if (secretKey.length !== 32) {
    throw new Error(
      `workerPrivateKey must be 32 bytes (got ${secretKey.length})`,
    );
  }

  const publicKey = ed25519.getPublicKey(secretKey);
  const workerId = bytesToHex(publicKey);
  const completedAt = args.completedAt ?? Math.floor(Date.now() / 1000);
  const nonce = args.nonce ?? bytesToHex(randomBytes(16));

  assertSafeU64("workFactor", args.workFactor);
  assertSafeI64("completedAt", completedAt);
  const workHash = "workHash" in args
    ? normalizeHex(args.workHash)
    : computeWorkHash(args.workInput, args.workIterations);

  const unsigned: Omit<ComputeReceipt, "signature"> = {
    task_id: args.taskId,
    worker_id: workerId,
    work_hash: workHash,
    work_factor: args.workFactor,
    completed_at: completedAt,
    nonce,
  };

  const message = canonicalSigningMessage(unsigned);
  const signature = ed25519.sign(new TextEncoder().encode(message), secretKey);

  return {
    ...unsigned,
    signature: bytesToHex(signature),
  };
}

export function verifyComputeReceipt(receipt: ComputeReceipt): boolean {
  if (!receipt.signature) return false;

  let publicKey: Uint8Array;
  let signature: Uint8Array;
  try {
    publicKey = hexToBytes(normalizeHex(receipt.worker_id));
    signature = hexToBytes(normalizeHex(receipt.signature));
  } catch {
    return false;
  }

  if (publicKey.length !== 32 || signature.length !== 64) {
    return false;
  }

  const { signature: _ignored, ...unsigned } = receipt;
  const message = canonicalSigningMessage(unsigned);

  try {
    return ed25519.verify(
      signature,
      new TextEncoder().encode(message),
      publicKey,
    );
  } catch {
    return false;
  }
}
