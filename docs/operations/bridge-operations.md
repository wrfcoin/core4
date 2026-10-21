# Bridge operations: NOT_AVAILABLE

Cross-chain transfer operation is not part of the public WRFCoin release.
There is no supported relayer, provider, transfer API, or operational receipt
verification path. Do not configure or monitor a bridge as active.

The authenticated `/bridge/safety/*` routes remain only so operators can keep
dormant bridge state halted and audit that safety posture. They do not establish
availability or authorize a transfer-success claim.
