import { describe, expect, it } from "@jest/globals";
import { ed25519 } from "@noble/curves/ed25519.js";
import { bytesToHex, hexToBytes } from "@noble/hashes/utils.js";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import {
  canonicalBodyBytes,
  canonicalSigningMessage,
  type ComputeReceiptSigningInput,
  type ForecastVerificationReceiptSigningInput,
} from "../compute-receipt/canonical-signing.js";

const EXPECTED_SCHEMA = "wrfcoin.signing-preimage-v2.golden/4";
const __dirname = dirname(fileURLToPath(import.meta.url));
const FIXTURE_PATH = join(
  __dirname,
  "..",
  "..",
  "..",
  "..",
  "shared-protocol",
  "tests",
  "fixtures",
  "golden",
  "signing_preimage_v2.json",
);

interface GoldenCaseFields {
  frame: "composed_v2";
  tag: string;
  canonical_body_hex: string;
  preimage_v1_hex: string;
  preimage_v2_hex: string;
  signature_v2_hex: string;
  signer_public_key_hex: string;
  signer_secret_hex: string;
}

type GoldenCase =
  | (GoldenCaseFields & {
      kind: "ComputeReceipt";
      input: ComputeReceiptSigningInput;
      tampered_input: ComputeReceiptSigningInput;
    })
  | (GoldenCaseFields & {
      kind: "ForecastVerificationReceipt";
      input: ForecastVerificationReceiptSigningInput;
      tampered_input: ForecastVerificationReceiptSigningInput;
    });

interface GoldenFixture {
  schema: string;
  chain_id: string;
  network_id: string;
  cases: unknown[];
}

function isTargetCase(value: unknown): value is GoldenCase {
  if (typeof value !== "object" || value === null) return false;
  const kind = (value as { kind?: unknown }).kind;
  return kind === "ComputeReceipt" || kind === "ForecastVerificationReceipt";
}

const fixture = JSON.parse(
  readFileSync(FIXTURE_PATH, "utf8"),
) as GoldenFixture;
if (fixture.schema !== EXPECTED_SCHEMA) {
  throw new Error(
    `unsupported signing-preimage fixture schema ${fixture.schema}; expected ${EXPECTED_SCHEMA}`,
  );
}

const targetCases = fixture.cases.filter(isTargetCase);
if (
  targetCases.length !== 2 ||
  !targetCases.some(({ kind }) => kind === "ComputeReceipt") ||
  !targetCases.some(({ kind }) => kind === "ForecastVerificationReceipt")
) {
  throw new Error(
    "golden fixture must contain exactly one ComputeReceipt and one ForecastVerificationReceipt case",
  );
}

function flipHexNibble(hex: string): string {
  if (hex.length === 0) throw new Error("cannot corrupt empty hex");
  return `${hex[0] === "0" ? "1" : "0"}${hex.slice(1)}`;
}

function assertHex(label: string, actual: Uint8Array, expected: string): void {
  const actualHex = bytesToHex(actual);
  if (actualHex !== expected) {
    throw new Error(
      `${label} drifted from Rust: JS=${actualHex} Rust=${expected}`,
    );
  }
}

function canonicalBody(vector: GoldenCase, tampered: boolean): Uint8Array {
  const input = tampered ? vector.tampered_input : vector.input;
  return vector.kind === "ComputeReceipt"
    ? canonicalBodyBytes({ kind: vector.kind, input })
    : canonicalBodyBytes({ kind: vector.kind, input });
}

function preimage(
  vector: GoldenCase,
  body: Uint8Array,
  protocolVersion: 1 | 2,
): Uint8Array {
  return canonicalSigningMessage(
    {
      frame: "composed_v2",
      tag: vector.tag,
      chainId: fixture.chain_id,
      networkId: fixture.network_id,
      protocolVersion,
    },
    body,
  );
}

function assertVectorParity(vector: GoldenCase): void {
  if (vector.frame !== "composed_v2") {
    throw new Error(`${vector.kind}: unsupported frame ${vector.frame}`);
  }

  const body = canonicalBody(vector, false);
  assertHex(`${vector.kind} canonical body`, body, vector.canonical_body_hex);

  const preimageV1 = preimage(vector, body, 1);
  assertHex(`${vector.kind} V1 preimage`, preimageV1, vector.preimage_v1_hex);

  const preimageV2 = preimage(vector, body, 2);
  assertHex(`${vector.kind} V2 preimage`, preimageV2, vector.preimage_v2_hex);

  const signature = ed25519.sign(
    preimageV2,
    hexToBytes(vector.signer_secret_hex),
  );
  assertHex(`${vector.kind} V2 signature`, signature, vector.signature_v2_hex);
  if (
    !ed25519.verify(
      hexToBytes(vector.signature_v2_hex),
      preimageV2,
      hexToBytes(vector.signer_public_key_hex),
    )
  ) {
    throw new Error(`${vector.kind}: Rust V2 signature did not verify in JS`);
  }

  const tamperedPreimage = preimage(vector, canonicalBody(vector, true), 2);
  if (bytesToHex(tamperedPreimage) === vector.preimage_v2_hex) {
    throw new Error(`${vector.kind}: tampered input reproduced the golden preimage`);
  }
  if (
    ed25519.verify(
      hexToBytes(vector.signature_v2_hex),
      tamperedPreimage,
      hexToBytes(vector.signer_public_key_hex),
    )
  ) {
    throw new Error(`${vector.kind}: golden signature accepted tampered input`);
  }
}

describe("Rust signing-preimage v2 golden-vector parity", () => {
  it.each(targetCases)(
    "$kind is byte-identical for body, V1/V2 preimages, and V2 signature",
    (vector) => {
      expect(() => assertVectorParity(vector)).not.toThrow();
    },
  );

  it("fails when the Rust golden vector is corrupted", () => {
    const vector = targetCases[0];
    if (!vector) throw new Error("missing target golden vector");
    const corrupted = {
      ...vector,
      preimage_v2_hex: flipHexNibble(vector.preimage_v2_hex),
    } as GoldenCase;

    expect(() => assertVectorParity(corrupted)).toThrow(
      /V2 preimage drifted from Rust/,
    );
  });
});
