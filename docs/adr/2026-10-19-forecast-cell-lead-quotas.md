# POST-FREEZE proposal: forecast cell and lead-window quotas

Status: design-only, advisory proposal for owner approval. Not implemented.
Refs core4#3604 and core4#3114.

Current API quotas are wallet-wide and route-specific. Commit requests carry an
opaque hash and station id; neither proves a geographic cell or forecast lead.
A free-form manifest region is not a canonical cell. No current API response or
client should claim that cell or lead-window quotas are enforced.

Proposed spelling for review: `geo10:<lat_idx>:<lon_idx>`, using the existing
geo-grid 10-km grid (111 km per latitude degree, south-west origin), with latitude
indices 0–1997 and longitude indices 0–3995. Decimal indices have no leading
zeros, signs or whitespace. A lead is an integer hour from 1 through 720;
proposed window width is six hours (1–6, 7–12, and so on). These choices are
advisory, not approved product policy. Owner approval must settle resolution,
window boundaries, rolling quota duration and numerical budget before coding.

A coordinated post-freeze design must bind metadata to the forecast's existing
hash-covered data, validate canonical spelling and exact manifest correspondence,
and prevent a commit admitted under one cell or window from revealing under
another. Merely trusting a commit's declared label is insufficient. Define
admission persistence, retention, restart recovery, cross-node behavior and how
already-accepted legacy commits reveal before requiring new fields. The API
cannot independently attest the geographic truth of observations.

Coordinate the node, CLI, forecast-kit, documentation and migration examples in
one versioned rollout. Do not change today's request fields or hash-covered JSON
as part of the wallet-wide limiter. Protocol-wide enforcement is separate:
peer admission and block validation need identical height-gated rules, a
versioned preimage and schedule-extension activation, with default-OFF parity.

Before approval: demonstrate canonical-alias refusal, cell/window boundaries,
commit-to-reveal mismatch rejection, retries, restart/reorg recovery, legacy
transition behavior, bounded storage and concurrency; add replay differential
proof if consensus changes. No activation height is selected here.
