# Security Policy

Public policy: https://wrfcoin.com/security. Scope: the public testnet.
No bounty or payment is promised for reports.

WRFCoin runs a **public testnet**. If you have found a vulnerability, we want to hear about it
privately before anyone else does.

## Reporting a vulnerability

**Email `security@wrfcoin.com`.** This is the same address as the Incident Commander channel in
[`docs/security/INCIDENT_RESPONSE_PLAN.md`](docs/security/INCIDENT_RESPONSE_PLAN.md), so a report
reaches the people who can act on it.

Encrypt if you are able to; if you do not have a key for us, send an unencrypted message asking for
one rather than posting details anywhere public.

**Please do not open a public GitHub issue for a security report.** Issues in this repository are
world-readable once the repository is public, and a security issue filed there is a disclosure, not a
report.

> **Note for maintainers:** this repository is currently private, so GitHub's *"Report a
> vulnerability"* button (private vulnerability reporting) is not available to outside researchers.
> **Enable private vulnerability reporting at the same time the repository is made public**, and add
> it here as a second channel. Until then, email is the only private route.

> **Reporting domain — resolved 2026-08-04.** `security@wrfcoin.com` is the single security contact.
> Every `security@wrfcoin.org` reference in this repository was moved to `.com`, and the RFC 2606
> `example.com` placeholders were removed.
>
> `wrfcoin.org` is a Google Workspace domain alias of `wrfcoin.com`, so both already delivered to the
> same mailbox — those addresses were inconsistent, not broken. The mailbox is verified working:
> inbound from an external non-Google sender delivers, and outbound passes SPF, DKIM (`d=wrfcoin.com`,
> 2048-bit) and DMARC.

### What to include

The more of this you can give us, the faster we can confirm it:

- what the issue is, and what an attacker gains
- the commit SHA or release you tested against
- reproduction steps, or a proof-of-concept
- affected component — consensus, P2P, RPC/`launch_api`, wallet, oracle path, or build/release
- whether you have disclosed it to anyone else, and any deadline you are working to

### What happens next

| Stage | Target |
|---|---|
| We acknowledge receipt | **2 business days** |
| We confirm or dispute the finding, with a severity | **5 business days** |
| We agree a remediation timeline with you | at confirmation |
| We tell you when a fix has landed, and where | at merge |

Severity uses the P1–P4 scale already defined in
[`docs/security/INCIDENT_RESPONSE_PLAN.md`](docs/security/INCIDENT_RESPONSE_PLAN.md) §2 — we do not
maintain a second scheme.

We will keep you updated even when the answer is slow or the answer is "we disagree". If we dispute a
finding we will say why, and you are free to disagree publicly once we have.

## Disclosure

We ask for **90 days** from acknowledgement before public disclosure, or until a fix ships — whichever
comes first. If a fix will take longer we will tell you and explain why, rather than let the date pass
silently.

If a vulnerability is already being exploited, tell us that in the first message. We will move
immediately and coordinate disclosure with you rather than ask you to wait.

We will credit you by name or handle in the fix and in any advisory, unless you would rather we did
not. Please tell us which you prefer.

## Scope

**In scope** — anything in this repository:

- consensus and block production, including finality and fork choice
- P2P networking, gossip and peer handling
- the node HTTP/WebSocket surface (`launch_api`) and its authentication
- cryptographic implementation, signing paths and key handling
- the oracle and weather-contribution paths
- state handling, storage and crash consistency
- build, release and upgrade provenance

**Out of scope:**

- the public testnet's *availability* — see the testnet caveat below
- findings that require a physical attack on an operator's machine or possession of their hardware token
- social engineering of maintainers or operators
- automated scanner output with no demonstrated impact
- vulnerabilities in third-party dependencies with no exploitable path in this repository — report
  those upstream, though we would still like to know

## Testing against the public testnet

Please **do not** run denial-of-service, sustained load, or spam tests against the live testnet. It is
a shared resource, and taking it down teaches us nothing we did not already assume.

Everything else — malformed input, protocol abuse, auth bypass attempts against your **own** node, and
anything at all against a local deployment — is welcome. If you need a target, run a node yourself; if
you need help doing that, ask.

Do not access, modify, or exfiltrate data belonging to other participants. If you encounter such data
by accident, stop, and tell us what you saw so we can assess exposure.

## Safe harbour

If you follow this policy in good faith, we will not pursue or support legal action against you for
your research, and we will say so publicly if asked. If you are unsure whether something is in scope,
ask first — asking is always in scope.

This is our commitment, not legal advice, and it cannot bind third parties whose systems you might
reach.

## The testnet caveat

This is a **testnet**. Tokens on it have **no monetary value**, and **the chain may be reset at any
time** without notice or migration of balances. Testnet balances will not be carried to any future
network.

That is not a disclaimer against reporting — a bug that would be critical on a real network is still
critical here, and finding it now is the entire point of running a testnet in public. It is a
disclaimer against *relying* on the chain.

## Related documents

- [`docs/security/TESTNET_GO_NO_GO.md`](docs/security/TESTNET_GO_NO_GO.md) — authoritative pre-testnet launch checklist and prohibited attachments
- [`docs/security/INCIDENT_RESPONSE_PLAN.md`](docs/security/INCIDENT_RESPONSE_PLAN.md) — severity scale and internal response
- [`docs/security/THREAT_MODEL.md`](docs/security/THREAT_MODEL.md) — the attacker model we design against
- [`docs/security/PROTOCOL_SPEC.md`](docs/security/PROTOCOL_SPEC.md) — protocol specification
- [`docs/security/UNSAFE_POLICY.md`](docs/security/UNSAFE_POLICY.md) — policy on `unsafe` Rust
- [`docs/security/BUILD_PROVENANCE.md`](docs/security/BUILD_PROVENANCE.md) — toolchain pinning and build reproducibility

## Understanding `.secrets.baseline`

The detect-secrets baseline records detector type, source location and a hash of each finding. It is an inventory used by the secret-scanning workflow, not a collection of usable credentials and not a declaration that every finding is safe. Test fixtures can contain synthetic keys, addresses and high-entropy test vectors that resemble credentials; their presence in the baseline does not establish that a value is synthetic or that it has been reviewed. Production findings still need individual investigation and any real credential needs rotation by its owner.

Run the metadata-only summary from this checkout:

```bash
python3 scripts/summarize_secret_baseline.py
python3 scripts/summarize_secret_baseline.py --format json
```

The report counts findings by top-level directory and detector type; JSON also provides their cross-tabulation. Counts are computed from the current baseline, not hard-coded. The script reads no source files, prints no hashes or matched values, and changes neither the baseline nor scanning policy. A metadata summary is not a fresh secret scan: retain the existing CI audit and report suspected exposures privately using the contact above.
