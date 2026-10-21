# Redis Eviction Policy — Security Invariant

**Status:** Authoritative runbook
**Tracks:** [wrfcoin/core4#1203](https://github.com/wrfcoin/core4/issues/1203)
**Last reviewed:** 2026-04-10

---

## 1. Purpose and the invariant

Redis instances that hold **security-critical keys** MUST use
`volatile-lru` or `noeviction` as their `maxmemory-policy`. They MUST NOT
use `allkeys-lru`, `allkeys-lfu`, `allkeys-random`, or any other
`allkeys-*` mode.

A key is **security-critical** if its disappearance changes a security
decision. In WRFCoin this includes, at minimum:

- **JWT revocation keys** — a revoked access token must stay revoked
  until its natural expiry. The revocation key has a TTL of 24h, matching
  the access token TTL.
- **Session revocation / logout markers** — same reasoning.
- **Rate-limit counters that gate authentication decisions** — e.g.,
  login attempt counters. Evicting these silently resets the brute-force
  budget.
- **Idempotency tokens** for state-changing operations.
- **Short-term OTP / magic-link nonces.**

If the same Redis instance also holds pure cache entries, the cache
entries MUST be given TTLs so they are eligible for eviction under
`volatile-lru`; otherwise the cache grows unbounded and the instance OOMs.

---

## 2. Why `volatile-lru`, not `allkeys-lru`

| Policy | Eligible for eviction | Safe for security-critical keys? |
|--------|-----------------------|----------------------------------|
| `noeviction` | nothing — writes fail when OOM | Yes — safest, but requires capacity planning |
| `volatile-lru` | only keys with a TTL | **Yes** — recommended default |
| `volatile-ttl` | only keys with a TTL, shortest first | Yes, but less predictable |
| `volatile-lfu` | only keys with a TTL, least frequent first | Yes |
| `allkeys-lru` | **everything, including TTL'd keys** | **No — do not use** |
| `allkeys-lfu` | everything | No |
| `allkeys-random` | everything | No |
| `volatile-random` | only keys with a TTL | Yes, but non-deterministic |

Under `volatile-lru`, revocation keys (which carry a 24h TTL) remain
evictable **in principle**, but they are evicted only when memory
pressure exceeds `maxmemory` and no non-TTL candidate exists. In
practice, cache entries created with TTL churn through the eviction
pool far faster than revocation keys, because:

1. Revocation keys are written only on logout or token invalidation — a
   rare event relative to cache writes.
2. Revocation keys have a long TTL (24h) relative to typical cache
   entries (seconds to minutes), so LRU ranks them as recently-created
   and less likely to be chosen.
3. Cache entries are read-write churning; revocation keys are
   write-once, read-many.

Contrast with `allkeys-lru`: **any** key — including revocation keys —
is eligible. Under sustained write pressure to the cache namespace, a
revocation key can be evicted purely because it is older in LRU order
than newly minted cache entries, even though its TTL has hours left.
When that happens, the middleware that checks revocation (see
`backend/server/middleware/auth.ts` — the canonical JWT revocation
check path) treats the revoked token as valid. This is a silent
authentication bypass: the token works again, and nothing logs an
error.

---

## 3. What this protects against

**Threat model:** an attacker holds a stolen JWT. The legitimate user
(or an admin) revokes it, writing `revoked:<jti>` to Redis with the
token's residual TTL. Under `allkeys-lru`, sustained cache write volume
evicts `revoked:<jti>` before its TTL. The attacker retries the stolen
token; the revocation check returns "not revoked"; access is granted.

There is **no monitoring signal** for this unless you are specifically
watching eviction counts against the revocation namespace. Requests
succeed normally. Logs show legitimate-looking `200`s. The only trace
is an unexplained bump in `evicted_keys` on the Redis instance — a
metric that is almost never alerted on by default.

---

## 4. Current core4 Redis configurations

Table is authoritative as of the merge of this runbook; update when
adding a new Redis instance or changing a policy.

| File | Policy (post #1203) | Scope | Notes |
|------|---------------------|-------|-------|
| `tests/docker-compose.test.yml` | `volatile-lru` | test profiles `testing`, `full`, `caching` | Fixed in #1203 (was `allkeys-lru`). 256mb cap. |
| `tests/contract/docker-compose.test.yml` | `volatile-lru` | Pact contract tests | Fixed in #1203 (was `allkeys-lru`). 256mb cap. |
| `docker/docker-compose.oracle.yml` | `noeviction` | Oracle service Redis | **Fixed in this PR.** The oracle Redis is dual-use: the `wrfcoin-oracle-api` service injects `JWT_SECRET` and connects to this same instance for both caching and session state. `noeviction` is the conservative choice — writes fail loudly under memory pressure rather than silently evicting JWT revocation or session keys. Operators must plan capacity accordingly (alert on `used_memory > 85% of maxmemory`). |
| `.taskmaster/docs/infrastructure-hardening/secure-os-service-configuration.md` line 721 | **⚠ `allkeys-lru`** | Hardening reference doc | This is example config in a hardening guide, not live infra. Flagged as a follow-up: the doc should either scope the recommendation to pure-cache instances or update to `volatile-lru`. Tracked in #1203. |

Non-core4 (documented for cross-repo traceability, not modified by this
PR):

- `wrfcoin/backend` — production `docker-compose.yml` audit and
  TestContainers regression test. Sibling PR in progress (agent halted
  mid-flight on usage cap; will resume). Cross-reference
  `wrfcoin/core4#1203`.
- `wrfcoin/infra` — production testnet Redis configs. The real
  production instance at `maxmemory 512mb` was running `allkeys-lru` in
  `testnet/docker-compose.testnet.yml` (shared by 11+ backend services,
  all injecting `JWT_SECRET`). Fixed in
  [`wrfcoin/infra#355`](https://github.com/wrfcoin/infra/pull/355)
  alongside 4 sibling configs (offline dev stack, enterprise-testing
  load harness, seed-test-data generator). AWS ElastiCache Terraform
  was already safe via the AWS-managed `default.redis7` parameter
  group.

---

## 5. How the regression test enforces this

The regression test lives in the sibling backend PR
(`wrfcoin/backend#TBD — see wrfcoin/core4#1203 for coordination`). It:

1. Spins up a Redis container via TestContainers with the production
   eviction policy (`volatile-lru`) and a deliberately small
   `maxmemory` cap.
2. Seeds N revocation keys via the **real** auth middleware code path
   (not a hand-rolled `SET`). This guarantees the test exercises the
   same TTL, key prefix, and serialization the runtime uses.
3. Floods the cache namespace with keys until Redis reports
   `evicted_keys > 0` in `INFO memory`.
4. Asserts that **every** seeded revocation key is still present.
5. Repeats the flood with `allkeys-lru` to confirm the test would
   fail under the unsafe policy — this is the negative control. If
   this negative-control path ever stops failing, the test has drifted
   and is no longer load-bearing.

Test name (placeholder): `test_volatile_lru_preserves_revocation_keys`.

---

## 6. Memory profile estimate for 48h sustained test

The acceptance criteria on #1203 asks for a back-of-envelope memory
estimate for a 48h sustained-load run. Because exact revocation and
cache rates are environment-specific, this is written as a formula
with named variables. Operators running the 48h test must fill in the
variables from their own production telemetry (backend logs,
Prometheus `redis_commands_total`, or a short pilot run).

### 6.1 Variables

| Variable | Meaning | How to measure |
|----------|---------|----------------|
| `R` | Revocation events per hour | Count of `auth.revoke` log lines / hour, or Prometheus `auth_token_revoked_total` rate |
| `k_rev` | Bytes per revocation key (key + serialized value + Redis overhead) | `redis-cli MEMORY USAGE revoked:<jti>` on a live sample |
| `T_rev` | Revocation key TTL in hours | Hardcoded in auth middleware; currently 24h |
| `C` | Cache writes per hour | Prometheus `redis_commands_total{cmd="set"}` rate minus revocation rate |
| `k_cache` | Bytes per cache entry (average) | Sample `MEMORY USAGE` across the cache namespace |
| `T_cache` | Average cache entry TTL in hours | From application code; varies by namespace |
| `H` | Test duration in hours | 48 for the acceptance run |
| `O_fixed` | Fixed Redis overhead (client tables, replication buffer, jemalloc fragmentation) | Start empty instance, read `used_memory` from `INFO memory` |

### 6.2 Steady-state revocation footprint

Revocation keys accumulate for at most `T_rev` hours before expiring.
Steady-state count is `R × min(T_rev, H)`. For `H ≥ T_rev` (a 48h run
with 24h TTL), this saturates at `R × T_rev`:

```
mem_rev = R × T_rev × k_rev
```

### 6.3 Steady-state cache footprint (with TTL)

Cache entries under TTL also saturate at `C × T_cache`, assuming the
application sets TTL on every cache write (which is a precondition for
`volatile-lru` to work):

```
mem_cache = C × T_cache × k_cache
```

### 6.4 Total

```
mem_total(H) = O_fixed
             + R × min(T_rev, H) × k_rev
             + C × min(T_cache, H) × k_cache
```

For the 48h acceptance run (`H = 48`, `T_rev = 24`):

```
mem_total(48) = O_fixed + R × 24 × k_rev + C × T_cache × k_cache
```

### 6.5 Capacity margin

`maxmemory` must be set such that:

```
maxmemory ≥ mem_total(H) × safety_factor
```

Recommended `safety_factor` is **1.5×** to absorb burst traffic,
fragmentation drift, and jemalloc growth between compactions. If
`maxmemory < mem_total(H) × 1.5`, the test will trigger eviction —
which is the exact condition the regression test probes. Operators
should **expect** eviction in a sufficiently small test instance; the
invariant is that eviction must land on cache entries, not revocation
keys.

### 6.6 Worked placeholder

As a placeholder to illustrate the shape of the answer — **do not
treat these numbers as authoritative**:

- `R = 100/h`, `k_rev = 200 B`, `T_rev = 24h`
  → `mem_rev ≈ 480 KB`
- `C = 50,000/h`, `k_cache = 1 KB`, `T_cache = 1h`
  → `mem_cache ≈ 50 MB`
- `O_fixed ≈ 10 MB`
- `mem_total(48) ≈ 60.5 MB`
- `maxmemory` at 1.5× ≈ 91 MB

The current test cap of **256 MB** comfortably fits this profile; the
test instance is not memory-bound under its nominal workload. The
regression test forces pressure by either (a) shrinking `maxmemory`
below `mem_total` or (b) inflating `C` until eviction triggers.
Operators running the 48h acceptance test on production-class hardware
must re-derive these numbers from production telemetry before trusting
any capacity number.

---

## 7. Monitoring and alerts

### 7.1 Metrics to watch

- **`evicted_keys`** from `INFO memory`. Any non-zero rate on an
  instance that holds security-critical state is an incident.
  Instrument as a Prometheus counter and alert on
  `rate(redis_evicted_keys_total[5m]) > 0`.
- **Revocation namespace key count** from
  `redis-cli --scan --pattern 'revoked:*' | wc -l`, or via an
  application-exported Prometheus gauge. A sudden drop that is not
  explained by natural TTL expiry is an incident.
- **`maxmemory-policy`** value itself — export as a label on a
  Prometheus gauge and alert if it ever equals `allkeys-*`.

### 7.2 Existing coverage

The `docs/runbooks/DATABASE_OPERATIONS_RUNBOOK.md` file has a
**"Redis Memory Pressure"** section that already documents:

- `redis-cli INFO memory` to check `used_memory_human`
- `redis-cli CONFIG GET maxmemory-policy` to verify the active policy
- `redis-cli --bigkeys` for capacity investigation

Cross-reference that section when responding to a memory-pressure
incident; this runbook adds the invariant layer above the operational
commands.

### 7.3 Recommended Grafana alert

```promql
# Alert: revocation keys evicted under memory pressure
(
  rate(redis_evicted_keys_total{instance=~".*auth.*|.*session.*"}[5m]) > 0
) and on(instance) (
  redis_config_maxmemory_policy{policy=~"volatile.*|noeviction"} == 1
)
```

If `policy` is not `volatile-*` or `noeviction`, the alert fires
earlier via a **separate** config-drift alert — catching the
misconfiguration before it causes a security regression.

---

## 8. Change control

Any change to `maxmemory-policy` on an instance listed in Section 4
requires:

1. A PR that updates this runbook's Section 4 table **in the same
   commit** as the config change.
2. A note in the PR description explaining why the new policy is safe
   for every namespace stored on that instance.
3. Review from one of the security owners listed in
   `docs/security/README.md`.

Adding a **new** Redis instance to core4 requires adding a row to the
Section 4 table and selecting the policy using the decision tree:

```
Does the instance ever hold a key whose absence changes a security
decision?
├── Yes  → volatile-lru (default) or noeviction (if capacity allows)
└── No   → allkeys-lru is permitted; document why in the table's Notes column
```

---

## 9. References

- Issue: [wrfcoin/core4#1203](https://github.com/wrfcoin/core4/issues/1203)
- Sibling backend PR (TestContainers regression test): in progress —
  agent halted on usage cap, resumes when reset; will land as
  `wrfcoin/backend#TBD`. See `wrfcoin/core4#1203`.
- Sibling infra PR (production policy fix):
  [`wrfcoin/infra#355`](https://github.com/wrfcoin/infra/pull/355) —
  flipped `testnet/docker-compose.testnet.yml` (the real production
  config with `maxmemory 512mb`) plus 4 sibling configs to
  `volatile-lru`.
- Canonical JWT revocation code path (read, don't modify from core4):
  `wrfcoin/backend` → `server/middleware/auth.ts` — cited here to
  identify **what** the invariant protects, not as a fix location.
- Troubleshooting entry: `docs/troubleshooting/database.md` §
  "Configure Memory Limits"
- Operational commands:
  `docs/runbooks/DATABASE_OPERATIONS_RUNBOOK.md` § "Redis Memory
  Pressure"
- Hardening reference (flagged for follow-up):
  `.taskmaster/docs/infrastructure-hardening/secure-os-service-configuration.md`
  line 721
- Redis docs: <https://redis.io/docs/latest/operate/oss_and_stack/management/config-file/#memory-management>
