# Law Book Plain-Language Style Guide (Issue #392)

## Goal
Make every law readable to an average participant without legal training.

## Writing rules
- Use direct sentences with one idea per sentence.
- Use common words. Avoid legal or policy jargon unless required.
- Say who is affected, what must happen, and what happens if not.
- Keep summaries short: target 1-2 sentences, max 220 characters preferred.
- Prefer active voice.

## Required law summary pattern
Use this pattern for `plain_language_summary`:

`<Who> must <required behavior>. <Optional exception or condition>.`

Examples:
- `Uncalibrated stations must submit reveals with CID references.`
- `Calibrated stations may submit inline METAR data only when the payload is plausible.`

## Avoid
- "hereinafter", "whereas", "notwithstanding", "pursuant to"
- multi-clause run-on sentences
- passive or ambiguous subjects ("it is required that...")

## Rendering format (UI/API)
All law displays should follow this order:
1. Headline: `LAW-XXX: Title`
2. Rule: one short sentence from `plain_language_summary`
3. Enforcement: advisory/api/consensus meaning in one sentence
4. Status: draft/active/deprecated in one sentence
5. Version note: `Version N`
6. History timeline: one-line changes by version (proposal + block)

## Tone and copy constraints
- Neutral and factual.
- No blame language.
- No threats; describe outcomes plainly.
- No unexplained acronyms, except known weather format names like METAR.

## Consistency checklist
- Subject is explicit (`untrusted contributors`, `calibrated stations`, `API clients`).
- Action verb is explicit (`must submit`, `may submit`, `cannot exceed`).
- Boundary is explicit (threshold, format, tier, or condition).
- Enforcement level is reflected accurately.
- Historical change text names proposal id and block number.

## UX copy snippets
- Advisory: `Advisory only. This guides behavior but does not block transactions.`
- API: `API enforced. Requests can be rejected at API validation time.`
- Consensus: `Consensus enforced. Invalid transactions are rejected during state transition.`
- Draft: `Draft. The law is recorded but not active.`
- Active: `Active. The law currently applies.`
- Deprecated: `Deprecated. The law is retained for history and should not be used for new policy.`

## Frontend display notes
- Default to showing active laws first.
- Keep category and enforcement badges visible in list cards.
- Collapse history by default, expand on demand.
- Show exact proposal id and block for every history item.

## Review gate before merge
- Law summary passes readability check by a non-author reviewer.
- Summary still matches deterministic enforcement behavior.
- API rendering strings match this guide exactly unless change is approved.
