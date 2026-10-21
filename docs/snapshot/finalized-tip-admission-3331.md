# Snapshot admission when source finality is active

Refs #3331, #3288.

A snapshot carries state only at height H. Its block window cannot reconstruct
state below H, because it carries no earlier state or WAL. The recipient must
retain finalized=H, including after restart. Lowering that anchor would permit
incorrect replay.

The signed snapshot payload already carries the source's finalized height.
When that signed claim is nonzero, ingest requires it to equal H, and signed
export refuses a source whose current tip is still unfinalized. HTTP returns 409 with
`SNAPSHOT_TIP_NOT_FINALIZED`. This is a snapshot admission rule; block validity,
state transitions, finality processing, signed payload encoding and schedules
retain their existing rules.

The export route retains its unsigned diagnostic response when no snapshot
signer is configured or signing fails, including when F<H. Such a response
has `signed_payload: null` and cannot be ingested. The finality restriction
applies only to a successfully signed export; signing failure still follows
the existing observable unsigned fallback.

Current production PoP nodes have BFT finality INERT and report F=0. They retain
signed export and ingest. The nonzero source claim is the activation proxy
available in the existing signed payload; it is not an independent BFT signal
or certificate. In particular, a snapshot-installed source reports its local
irreversible anchor, which does not establish network finality.

For F=0 sources, confirm the source tip hash at H with at least two independent
peers before ingest (issue option 2). This reduces exposure to a transient
source branch but is not finality proof: a later reorg below H can still leave
the recipient anchored on an orphan and require operator-directed bootstrap.
The admission fence is inert on this path; it does not eliminate that risk.

A source with tip H and active finality 0<F<H cannot provide an installable
snapshot of its current state. Retry once its tip is finalized, use a trusted
source whose tip is finalized, or bootstrap through ordinary block sync.
Serving historical state at F needs a separate historical-state export path;
this change never labels state at H as state at F.

The source's claim is authenticated by the existing trusted snapshot signer,
not an independently verified BFT certificate. Honest finality reporting is
part of that existing trust boundary. An incorrect or compromised trusted
signer can still assert false finality. Block hashes, genesis, state roots,
entries and signatures retain their independent validation.

Existing installed snapshots are unaffected. An already orphaned recipient
still needs an operator-directed bootstrap with fresh storage and a verified
source. Retain the old data for diagnosis; no automatic deletion, state rollback
or replacement occurs. A network conflict below genuinely finalized H is a
finality/trusted-source incident, not a routine reorg this node can safely apply.

Local tests reproduce the old asymmetry using an unfinalized source, actual
recipient install and persisted reload: the source plans a fork below H, while
the recipient refuses it. The new guard refuses that source before install.
The zero-finality controls preserve signed export and actual HTTP install,
replay refusal and database reopen with the same tip/root and anchor at H.
The shared snapshot fixtures and P2P source keep their original zero-finality
claims. A fixture-parity control compares the whole payload against an actual
signed HTTP export, then ingests that exact export with the fixture entries.
Finalized snapshots still install with the same state root and tip, remain
finalized at H, and permit extension from H. HTTP tests cover refusal without
state/history/replay mutation, signature tampering, retry of the same entries
without blacklist poisoning, replay after acceptance and signed export retry.

## Bootstrap-trust disposition (SOL-R story 8)

Every admitted snapshot now carries one explicit disposition, `BootstrapTrust`, derived only from the signed
`(finalized_height, height)` pair after the existing refusals have run. It changes no acceptance rule.

| signed pair | outcome | disposition (label) |
|---|---|---|
| `0 < finalized < height` | refused (`TipNotFinalized`, HTTP 409 `SNAPSHOT_TIP_NOT_FINALIZED`) | none |
| `finalized == height > 0` | admitted | `SignerAttestedFinalTip` (`signer_attested_final_tip`) |
| `finalized == 0` (inert finality) | admitted: the owner-permitted bootstrap path | `SignerTrustedBootstrap` (`signer_trusted_bootstrap`) |
| `finalized > height` | refused (`FinalizedAboveTip`) | none |

Neither label is fork-final certification: the claim is authenticated only by the trusted snapshot signer, never by an
independently verified BFT certificate, and the installed anchor is local. The ingest response reports the label as
`bootstrap_trust`. A same-height alternate fork gets the disposition its own signed fields declare; the label is not
persisted, so a restarted node keeps its local anchor at the snapshot height but claims no disposition, and verifying the
same signed snapshot again yields the same one. The inert bootstrap path is not removed and no finality schedule changes.
