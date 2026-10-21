# Cross-chain transfers: NOT_AVAILABLE

Cross-chain asset transfers are **NOT_AVAILABLE** in the WRFCoin public release.
The release node exposes `/cross_chain` and `/cross_chain/status` only as
fail-closed feature-status routes; both return the standard disabled-feature
response. There is no supported bridge CLI, transfer API, generated client, or
provider package.

The `/bridge/safety/*` routes are authenticated operator controls for dormant
bridge-safety state. They do not list networks, initiate transfers, or prove
that a bridge is operational.

Historical experiments and design documents are not release instructions and
must not be packaged or presented as successful transfer examples. Activating
this feature requires a separate approved design, a real configured target,
and receipt verification against that target before any success response can
be published.
