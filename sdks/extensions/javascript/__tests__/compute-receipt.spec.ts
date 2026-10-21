import { describe, expect, it } from "@jest/globals";
import { ed25519 } from "@noble/curves/ed25519.js";
import { bytesToHex } from "@noble/hashes/utils.js";
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import {
  canonicalSigningMessage,
  computeWorkHash,
  signComputeReceipt,
  verifyComputeReceipt,
  type ComputeReceipt,
} from "../compute-receipt/index.js";

const TEST_SECRET_KEY_HEX =
  "0101010101010101010101010101010101010101010101010101010101010101";

const __dirname = dirname(fileURLToPath(import.meta.url));
const VECTOR_DIR = join(__dirname, "..", "compute-receipt", "test-vectors");

interface FixtureFile {
  description: string;
  fixture_version: number;
  receipt: ComputeReceipt;
  signing_message: string;
}

function loadFixtures(): { name: string; data: FixtureFile }[] {
  return readdirSync(VECTOR_DIR)
    .filter((name) => name.endsWith(".json"))
    .sort()
    .map((name) => ({
      name,
      data: JSON.parse(
        readFileSync(join(VECTOR_DIR, name), "utf-8"),
      ) as FixtureFile,
    }));
}

describe("compute receipt signing", () => {
  it("produces a valid signature that round-trips", () => {
    const receipt = signComputeReceipt({
      taskId: "task-001",
      workerPrivateKey: TEST_SECRET_KEY_HEX,
      workHash: "c".repeat(64),
      workFactor: 1_000,
      completedAt: 1_700_000_000,
      nonce: "nonce-001",
    });

    expect(receipt.signature).toBeDefined();
    expect(receipt.signature).toMatch(/^[0-9a-f]{128}$/);
    expect(verifyComputeReceipt(receipt)).toBe(true);
  });

  it("can derive work_hash from workInput and workIterations", () => {
    const workInput = new TextEncoder().encode("collector-reading");
    const workIterations = 3;
    const receipt = signComputeReceipt({
      taskId: "task-input-iterations",
      workerPrivateKey: TEST_SECRET_KEY_HEX,
      workInput,
      workIterations,
      workFactor: workIterations,
      completedAt: 1_700_000_000,
      nonce: "nonce-input",
    });

    expect(receipt.work_hash).toBe(computeWorkHash(workInput, workIterations));
    expect(verifyComputeReceipt(receipt)).toBe(true);
  });

  it("derives worker_id from the signing key", () => {
    const receipt = signComputeReceipt({
      taskId: "worker-id-check",
      workerPrivateKey: TEST_SECRET_KEY_HEX,
      workHash: "0".repeat(64),
      workFactor: 1,
    });

    const expectedWorkerId = bytesToHex(
      ed25519.getPublicKey(Buffer.from(TEST_SECRET_KEY_HEX, "hex")),
    );
    expect(receipt.worker_id).toBe(expectedWorkerId);
  });

  it("rejects tampered receipts", () => {
    const receipt = signComputeReceipt({
      taskId: "tamper-check",
      workerPrivateKey: TEST_SECRET_KEY_HEX,
      workHash: "a".repeat(64),
      workFactor: 50,
      completedAt: 1_700_000_001,
      nonce: "nonce-tamper",
    });

    const tampered: ComputeReceipt = { ...receipt, work_factor: 51 };
    expect(verifyComputeReceipt(tampered)).toBe(false);
  });

  it("rejects unsigned receipts", () => {
    const unsigned: ComputeReceipt = {
      task_id: "x",
      worker_id: "00".repeat(32),
      work_hash: "0".repeat(64),
      work_factor: 1,
      completed_at: 0,
      nonce: "n",
    };

    expect(verifyComputeReceipt(unsigned)).toBe(false);
  });

  it("matches the Rust envelope format", () => {
    const message = canonicalSigningMessage({
      task_id: "task-001",
      worker_id: "ab".repeat(32),
      work_hash: "c".repeat(64),
      work_factor: 1_000,
      completed_at: 1_700_000_000,
      nonce: "nonce-001",
    });

    const parts = message.split("|");
    expect(parts).toHaveLength(8);
    expect(parts[0]).toBe("wrfcoin-sign-v1");
    expect(parts[1]).toBe("wrfcoin.compute_receipt.v1");
    expect(parts[2]).toBe("wrfcoin-main");
    expect(parts[3]).toBe("mainnet");
    expect(parts[4]).toBe("ab".repeat(32));
    expect(parts[5]).toBe("1700000000");
    expect(parts[6]).toBe("nonce-001");
    expect(parts[7]).toMatch(/^[0-9a-f]{64}$/);
  });

  it("fills default completedAt and nonce", () => {
    const before = Math.floor(Date.now() / 1000);
    const receipt = signComputeReceipt({
      taskId: "task-defaults",
      workerPrivateKey: TEST_SECRET_KEY_HEX,
      workHash: "0".repeat(64),
      workFactor: 1,
    });
    const after = Math.floor(Date.now() / 1000);

    expect(receipt.completed_at).toBeGreaterThanOrEqual(before);
    expect(receipt.completed_at).toBeLessThanOrEqual(after);
    expect(receipt.nonce).toMatch(/^[0-9a-f]{32}$/);
    expect(verifyComputeReceipt(receipt)).toBe(true);
  });

  it("rejects invalid workFactor numeric values", () => {
    expect(() =>
      signComputeReceipt({
        taskId: "task-invalid-workfactor-fraction",
        workerPrivateKey: TEST_SECRET_KEY_HEX,
        workHash: "a".repeat(64),
        workFactor: 1.5,
        completedAt: 1_700_000_000,
        nonce: "nonce-1",
      })
    ).toThrow(/workFactor must be a finite integer/);

    expect(() =>
      signComputeReceipt({
        taskId: "task-invalid-workfactor-negative",
        workerPrivateKey: TEST_SECRET_KEY_HEX,
        workHash: "a".repeat(64),
        workFactor: -1,
        completedAt: 1_700_000_000,
        nonce: "nonce-2",
      })
    ).toThrow(/workFactor must be >= 0/);

    expect(() =>
      signComputeReceipt({
        taskId: "task-invalid-workfactor-range",
        workerPrivateKey: TEST_SECRET_KEY_HEX,
        workHash: "a".repeat(64),
        workFactor: Number.MAX_SAFE_INTEGER + 1,
        completedAt: 1_700_000_000,
        nonce: "nonce-3",
      })
    ).toThrow(/workFactor must be a safe integer/);
  });

  it("rejects invalid completedAt numeric values", () => {
    expect(() =>
      signComputeReceipt({
        taskId: "task-invalid-completedAt-fraction",
        workerPrivateKey: TEST_SECRET_KEY_HEX,
        workHash: "a".repeat(64),
        workFactor: 1,
        completedAt: 1_700_000_000.1,
        nonce: "nonce-4",
      })
    ).toThrow(/completedAt must be a finite integer/);

    expect(() =>
      signComputeReceipt({
        taskId: "task-invalid-completedAt-infinity",
        workerPrivateKey: TEST_SECRET_KEY_HEX,
        workHash: "a".repeat(64),
        workFactor: 1,
        completedAt: Number.POSITIVE_INFINITY,
        nonce: "nonce-5",
      })
    ).toThrow(/completedAt must be a finite integer/);

    expect(() =>
      signComputeReceipt({
        taskId: "task-invalid-completedAt-range",
        workerPrivateKey: TEST_SECRET_KEY_HEX,
        workHash: "a".repeat(64),
        workFactor: 1,
        completedAt: Number.MAX_SAFE_INTEGER + 1,
        nonce: "nonce-6",
      })
    ).toThrow(/completedAt must be a safe integer/);
  });
});

describe("computeWorkHash", () => {
  it("returns deterministic hex for fixed input and iterations", () => {
    const input = new TextEncoder().encode("seed");
    const left = computeWorkHash(input, 5);
    const right = computeWorkHash(input, 5);

    expect(left).toBe(right);
    expect(left).toMatch(/^[0-9a-f]{64}$/);
  });

  it("changes when iterations change", () => {
    const input = new TextEncoder().encode("seed");
    expect(computeWorkHash(input, 1)).not.toBe(computeWorkHash(input, 2));
  });

  it("rejects non-positive iterations", () => {
    expect(() => computeWorkHash(new Uint8Array(), 0)).toThrow();
    expect(() => computeWorkHash(new Uint8Array(), -1)).toThrow();
  });
});

describe("checked-in compute receipt fixtures", () => {
  const fixtures = loadFixtures();

  it("keeps at least three vectors checked in", () => {
    expect(fixtures.length).toBeGreaterThanOrEqual(3);
  });

  it.each(fixtures)("verifies signature for $name", ({ data }) => {
    expect(verifyComputeReceipt(data.receipt)).toBe(true);
  });

  it.each(fixtures)(
    "rebuilds the same signing message for $name",
    ({ data }) => {
      const { signature: _ignored, ...unsigned } = data.receipt;
      expect(canonicalSigningMessage(unsigned)).toBe(data.signing_message);
    },
  );
});
