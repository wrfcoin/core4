# Law Book Read API Contract (Issue #392)

Status: Phase-3 complete. Merged to `main` via PRs #438/#439.

## Purpose
Define read/query contracts for:
- concise plain-language rendering of laws,
- indexed search for active or scoped laws,
- historical version visibility derived from amendment history.

## Envelope
All responses use the same envelope:

```json
{
  "ok": true,
  "data": {},
  "error": null,
  "meta": {
    "request_id": "req_123",
    "timestamp": "2026-02-28T12:00:00Z",
    "api_version": "v1"
  }
}
```

Error envelope:

```json
{
  "ok": false,
  "data": null,
  "error": {
    "code": "VALIDATION_ERROR",
    "message": "status must be draft|active|deprecated"
  },
  "meta": {
    "request_id": "req_123",
    "timestamp": "2026-02-28T12:00:00Z",
    "api_version": "v1"
  }
}
```

## Endpoints

### 1) List laws
- Method: `GET`
- Path: `/governance/laws`
- Query params:
  - `status` (optional): `draft|active|deprecated`
  - `enforcement` (optional): `advisory|api|consensus`
  - `category` (optional): exact category string
  - `q` (optional): indexed search query over `law_id`, `title`, `category`, `plain_language_summary`, `amendments.summary`
  - `include_amendments` (optional): `true|false`, default `false`
  - `include_history` (optional): `true|false`, default `false`.
  - `limit` (optional): default `50`, max `200`
  - `cursor` (optional): pagination cursor (last seen `law_id`)

Validation rules:
- `include_history=true` requires `include_amendments=true`.

Behavior notes:
- Ordering is deterministic by `law_id` ascending.
- `cursor` pagination returns entries strictly after the provided `law_id`.
- `q` filtering is AND-based token matching on the indexed fields.

Success example:

```json
{
  "ok": true,
  "data": {
    "items": [
      {
        "law_id": "LAW-002-METAR-INLINE-PRIVILEGE",
        "title": "METAR inline privilege for calibrated stations",
        "category": "data_submission",
        "plain_language_summary": "Calibrated inline reveals must include plausible METAR; CID reveals bypass this check.",
        "plain_language_rendering": {
          "headline": "LAW-002-METAR-INLINE-PRIVILEGE: METAR inline privilege for calibrated stations",
          "rule": "Rule: Calibrated inline reveals must include plausible METAR; CID reveals bypass this check.",
          "enforcement": "Enforcement: Consensus enforced. Invalid transactions are rejected during state transition.",
          "status": "Status: Active. The law currently applies.",
          "version_note": "Version 2."
        },
        "enforcement_level": "consensus",
        "status": "active",
        "version": 2
      }
    ],
    "next_cursor": null,
    "search": {
      "query": "metar inline",
      "tokens": ["metar", "inline"],
      "indexed_fields": [
        "law_id",
        "title",
        "category",
        "plain_language_summary",
        "amendments.summary"
      ],
      "matched_law_count": 1
    }
  },
  "error": null,
  "meta": {
    "request_id": "req_123",
    "timestamp": "2026-02-28T12:00:00Z",
    "api_version": "v1"
  }
}
```

### 2) Get law by ID
- Method: `GET`
- Path: `/governance/laws/{law_id}`
- Path param:
  - `law_id` (required): canonical law ID (example `LAW-001-CID-MANDATORY`)
- Query params:
  - `include_amendments` (optional): `true|false`, default `true`
  - `include_history` (optional): `true|false`, default `true`

Validation rules:
- `include_history=true` requires `include_amendments=true`.
- malformed `law_id` returns `400` with `VALIDATION_ERROR`.

Not found:
- missing law returns `404` with `NOT_FOUND`.

Success example:

```json
{
  "ok": true,
  "data": {
    "law_id": "LAW-002-METAR-INLINE-PRIVILEGE",
    "title": "METAR inline privilege for calibrated stations",
    "category": "data_submission",
    "plain_language_summary": "Calibrated inline reveals must include plausible METAR; CID reveals bypass this check.",
    "plain_language_rendering": {
      "headline": "LAW-002-METAR-INLINE-PRIVILEGE: METAR inline privilege for calibrated stations",
      "rule": "Rule: Calibrated inline reveals must include plausible METAR; CID reveals bypass this check.",
      "enforcement": "Enforcement: Consensus enforced. Invalid transactions are rejected during state transition.",
      "status": "Status: Active. The law currently applies.",
      "version_note": "Version 2."
    },
    "enforcement_level": "consensus",
    "status": "active",
    "version": 2,
    "amendments": [
      {
        "proposal_id": "law-002-create",
        "updated_at_block": 6,
        "summary": "law created",
        "version": 1
      },
      {
        "proposal_id": "law-002-amend",
        "updated_at_block": 9,
        "summary": "summary amended",
        "version": 2
      }
    ],
    "historical_versions": [
      {
        "version": 1,
        "proposal_id": "law-002-create",
        "updated_at_block": 6,
        "summary": "law created",
        "plain_language_change": "Version 1 created by proposal 'law-002-create' at block 6."
      },
      {
        "version": 2,
        "proposal_id": "law-002-amend",
        "updated_at_block": 9,
        "summary": "summary amended",
        "plain_language_change": "Version 2 updated the plain-language summary via proposal 'law-002-amend' at block 9."
      }
    ]
  },
  "error": null,
  "meta": {
    "request_id": "req_456",
    "timestamp": "2026-02-28T12:00:01Z",
    "api_version": "v1"
  }
}
```

## Response shapes

`LawRecord` fields:
- `law_id: string`
- `title: string`
- `category: string`
- `plain_language_summary: string`
- `plain_language_rendering: LawPlainLanguageRendering`
- `enforcement_level: "advisory"|"api"|"consensus"`
- `status: "draft"|"active"|"deprecated"`
- `version: number`
- `amendments?: LawAmendment[]`
- `historical_versions?: LawHistoricalVersion[]`

`LawPlainLanguageRendering` fields:
- `headline: string`
- `rule: string`
- `enforcement: string`
- `status: string`
- `version_note: string`

`LawAmendment` fields:
- `proposal_id: string`
- `updated_at_block: number`
- `summary: string`
- `version: number`

`LawHistoricalVersion` fields:
- `version: number`
- `proposal_id: string`
- `updated_at_block: number`
- `summary: string`
- `plain_language_change: string`

`LawSearchMetadata` fields (list endpoint, only when `q` is provided):
- `query: string`
- `tokens: string[]`
- `indexed_fields: string[]`
- `matched_law_count: number`

## Production deployment

The Law Book read API is served by the native-chain process (default port 8556).
Clients query the native-chain host directly. The backend Express API (port 4001)
does not proxy these routes — canonical law state lives in the Rust blockchain,
not PostgreSQL.

## Backing source
Read model maps from shared-protocol governance law state:
- `state.governance.law_book.laws`
- `state.get_law(law_id)`

Historical output is derived from amendment records already stored in law metadata.
