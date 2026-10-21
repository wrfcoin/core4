# Constitution Hash Anchoring

Issue: #395

## Purpose

Define a deterministic canonical hash for the constitution markdown and anchor it in on-chain
governance state so constitutional legitimacy has an immutable protocol reference.

## Canonicalization Algorithm

Given constitution markdown content:

1. Convert line endings to `\n` (replace `\r\n` and `\r`).
2. Trim trailing whitespace from each line.
3. Join lines with `\n`.
4. Ensure exactly one terminal newline at EOF.
5. Hash normalized bytes with `sha3-256`.
6. Encode digest as lowercase hex.

## On-Chain Reference

Governance state stores:

- `content_hash` (canonical lowercase hex `sha3-256` digest)
- `hash_algorithm` (`sha3-256`)
- `content_version` (constitution version integer)
- `content_cid` (optional content-addressed storage pointer)
- `source_path` (`docs/governance/constitution.md`)

## Genesis/Default Anchoring

Default state computes the canonical hash from the committed constitution markdown and anchors it in
`governance.constitution`.

## Governance Update Path

Constitution reference updates use governance parameter key:

- `constitution.reference`

Payload JSON:

```json
{
  "content_hash": "<64-char-lowercase-hex>",
  "content_cid": "bafy... (optional)",
  "content_version": 2
}
```

These updates are treated as constitutional/protected governance updates and therefore use
constitutional thresholds.
